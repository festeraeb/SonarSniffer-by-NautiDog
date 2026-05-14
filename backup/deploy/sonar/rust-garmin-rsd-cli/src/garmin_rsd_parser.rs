/// Garmin RSD — Reverse Engineering Notes & Pro Build White Paper (Draft v0.3)
///
/// Scope: Garmin .RSD UHD / UHD2 logging, record structure, field mapping, parsing strategy, image assembly, georeferencing (KMZ), and GUI pipeline.
///
/// 1) Executive summary
/// - Pro pipeline parses Garmin .RSD logs, assembles sidescan rows, tonemaps, exports preview/MP4, builds KMZ mosaic using gx:LatLonQuad.
/// - Parser uses tolerant varstructure reader with custom CRC and resync to next record.
/// - Key fields: channel ID, time, lat/lon (Garmin mapunits), sample count, depth, optional beam angle.
///
/// 2) File & record format
/// - Record header magic: 0xB7E9DA86 (LE)
/// - Record trailer magic: 0xD9264B7C (LE)
/// - Structure: Two varstruct blocks (header → body), each with CRC. Header: magic/seq/time/data_size. Body: channel, geo, depth, layout hints.
/// - CRC: custom CRC32 (poly 0x04C11DB7, init 0, reflect in/out, xorout 0xFFFFFFFF). Modes: strict/warn/off.
///
/// 2.2 Field map (fn = field number)
/// Header varstruct:
///   fn=0 → magic (0xB7E9DA86)
///   fn=2 → seq (u32)
///   fn=4 → data_size (u16/u32)
///   fn=5 → time_ms (u32)
/// Body varstruct:
///   fn=0 → channel_id (u32 LE; DS/Port/Star)
///   fn=1 → depth_mm_varint (zigzag varint; depth_m = value/1000.0)
///   fn=7 → sample_cnt (u32)
///   fn=9 → lat_mapunits (s32; deg = value * 360 / 2^32)
///   fn=10 → lon_mapunits (s32; deg = value * 360 / 2^32)
///   fn=12 → beam_angle_deg (f32; optional)
///
/// Ping payload: starts at sonar_ofs, length = data_size - bytes_consumed_by_body.
/// Layout inference: blob_len/sample_cnt ratio infers channel type (u8/u16, 1/2channel).
///
/// 3) Geometry & geodesy
/// - Lat/lon from Garmin mapunits (signed int scaled to 360° over 2^32).
/// - Row footprint: LatLonQuad for each ping, swath userconfigurable (meters).
/// - Orientation: [port[::-1] | gap | starboard], seam heuristic for water column.
///
/// 4) Image assembly & tone mapping
/// - Layout: split port/starboard, insert water column gap, seam with auto/manual flips.
/// - Normalize u16→u8, clip, invert, gamma. Preview: waterfall.png, MP4.
///
/// 5) KMZ generation
/// - GroundOverlay per row with gx:LatLonQuad. Rows without lat/lon are gapfilled/interpolated.
/// - Packaging: doc.kml + row PNGs in *_sidescan.kmz.
///
/// 6) GPS gapfill
/// - Interpolation for short gaps, deadreckon for longer gaps. Tagging for overlays.
///
/// 7) GUI / Pro pipeline
/// - CRC mode, row height, water gap, clip, gamma, stride, swath, MP4, FPS, video height, max frames, orientation overrides, KMZ toggle, GPS fill.
/// - Outputs: CSV, row PNGs, waterfall.png, MP4, *_sidescan.kmz.
///
/// 8) Data dictionary
/// Header: 0 (magic), 2 (seq), 4 (data_size), 5 (time_ms)
/// Body: 0 (channel_id), 1 (depth), 7 (sample_cnt), 9 (lat), 10 (lon), 12 (beam_angle)
/// Payload: ping intensities, layout inferred.
///
/// 9) Validation checklist
/// - Depth scale, port/star ID, layout inference, KMZ overlay alignment, GPS gapfill QA.
///
/// 10) Limitations & edge cases
/// - Extra body fields, UHD2 beamaux blocks, GPS outages, layout variants.
///
/// 11) Roadmap
/// - Object detection, SuperOverlay KMZ, autoorientation, beamaware layout, trackline layer.
///
/// 12) Algorithms
/// - VarUInt/VarInt (zigzag), mapunits→degrees, layout inference, seam autoorientation, LatLonQuad placement.
///
/// 13) References
/// - Core pipeline, GUI, Garmin helper.
// Garmin RSD parser logic in Rust, with self-healing and channel mapping
// This is a skeleton for porting the Python logic to Rust

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use serde::{Serialize, Deserialize};
// ...existing code...


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    pub name: String,
    pub detected: bool,
    pub mapped_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RSDParseResult {
    pub success: bool,
    pub error_message: Option<String>,
    pub record_count: usize,
    pub channels: Vec<ChannelInfo>,
    pub unknown_channels: Vec<String>,
}

pub struct GarminRSDParser {
    known_channels: HashSet<String>,
    channel_map: HashMap<String, String>,
    dynamic_channel_map: HashMap<String, String>, // For self-healing and new mappings
}

impl GarminRSDParser {
            /// Attempt to auto-map unknown channel IDs using firmware float hits and known patterns
            pub fn auto_map_channel_id(&self, raw_values: &[u32], float_hits: &[(usize, f32)]) -> Option<String> {
                // Example: match raw value to known float-based unit identifiers (rounded for tolerance)
                let known_map = [
                    (8.43, "GT20/GT21/GT22/GT23/GT24/GT52/GT54/GT56 DownScan"),
                    (10.09, "SideVu/SideScan Port"),
                    (11.79, "SideVu/SideScan Starboard"),
                    (24.72, "CHIRP High"),
                    (14.10, "CHIRP Medium"),
                    (29.01, "CHIRP Low"),
                    (40.05, "UHD SideScan"),
                    (13.50, "UHD DownScan"),
                    (13.30, "DownScan Alt"),
                    (13.40, "DownScan Alt2"),
                    (4.20, "Temperature"),
                    (4.10, "Depth"),
                    (6.80, "GPS"),
                    (40.04, "UHD2 SideScan"),
                    (40.06, "UHD2 SideScan Alt"),
                    (9.99, "SideVu/SideScan Port Alt"),
                    (11.80, "SideVu/SideScan Starboard Alt"),
                    (12.00, "SideVu/SideScan Unknown"),
                    (0.00, "Unknown/Zero"),
                    // Additional floats for edge mapping
                    (8.44, "GT20/GT21/GT22/GT23/GT24/GT52/GT54/GT56 DownScan Alt"),
                    (10.10, "SideVu/SideScan Port Alt2"),
                    (11.78, "SideVu/SideScan Starboard Alt2"),
                    (13.60, "DownScan Alt3"),
                    (13.20, "DownScan Alt4"),
                    (14.00, "CHIRP Medium Alt"),
                    (24.70, "CHIRP High Alt"),
                    (29.00, "CHIRP Low Alt"),
                    (40.00, "UHD SideScan Alt"),
                    (40.10, "UHD2 SideScan Alt2"),
                    (4.30, "Temperature Alt"),
                    (4.00, "Depth Alt"),
                    (6.81, "GPS Alt"),
                    (7.00, "Unknown/Other"),
                    (15.00, "Unknown/Other2"),
                    (20.00, "Unknown/Other3"),
                    (25.00, "Unknown/Other4"),
                    (30.00, "Unknown/Other5"),
                    (50.00, "Unknown/Other6"),
                ];
                for &raw in raw_values {
                    // Try to match raw value as f32 (bitwise reinterpret)
                    let fval = f32::from_bits(raw);
                    for &(target, label) in &known_map {
                        if (fval - target).abs() < 0.05 {
                            return Some(label.to_string());
                        }
                    }
                }
                // Try to match against firmware float hits (offsets/values)
                for &raw in raw_values {
                    let fval = f32::from_bits(raw);
                    for &(_ofs, val) in float_hits {
                        if (fval - val).abs() < 0.05 {
                            return Some(format!("Firmware float match: {:.2}", val));
                        }
                    }
                }
                None
            }
        /// Parse variable struct with CRC validation (port of Python _parse_varstruct)
        fn parse_varstruct(slice: &[u8], crc_mode: &str) -> Option<(HashMap<u32, Vec<u8>>, usize)> {
            let mut pos = 0;
            // Read varuint (field count)
            let (n, p) = Self::read_varuint(slice, pos)?;
            pos = p;
            if n > 10000 {
                return None;
            }
            let mut fields = HashMap::new();
            for _ in 0..n {
                let (key, p) = Self::read_varuint(slice, pos)?;
                pos = p;
                let fn_id = key >> 3;
                let lc = key & 7;
                let vlen = if lc == 7 {
                    let (vlen, p) = Self::read_varuint(slice, pos)?;
                    pos = p;
                    if vlen > (slice.len() - pos) as u32 { return None; }
                    vlen
                } else {
                    lc
                } as usize;
                if pos + vlen > slice.len() { return None; }
                fields.insert(fn_id, slice[pos..pos+vlen].to_vec());
                pos += vlen;
            }
            if pos + 4 > slice.len() { return None; }
            let crc_read = u32::from_be_bytes([slice[pos], slice[pos+1], slice[pos+2], slice[pos+3]]);
            let data = &slice[0..pos];
            let crc_calc = Self::crc32_custom(data);
            if crc_mode == "strict" && crc_calc != crc_read {
                return None;
            } else if crc_mode == "warn" && crc_calc != crc_read {
                // Optionally log warning
            }
            pos += 4;
            Some((fields, pos))
        }

        /// Read variable-length unsigned integer (port of Python _read_varuint_from)
        fn read_varuint(slice: &[u8], mut pos: usize) -> Option<(u32, usize)> {
            let mut result = 0u32;
            let mut shift = 0;
            while pos < slice.len() {
                let b = slice[pos];
                pos += 1;
                result |= ((b & 0x7F) as u32) << shift;
                if b & 0x80 == 0 { return Some((result, pos)); }
                let crc_mode = "off";
                if shift > 28 { return None; }
            }
            None
        }

        /// CRC-32 custom (port of Python _crc32_custom)
        fn crc32_custom(data: &[u8]) -> u32 {
            use crc::{Crc, CRC_32_ISO_HDLC};
            let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
            crc.checksum(data)
        }
    pub fn new() -> Self {
        let known_channels = ["port", "starboard", "down", "temp", "depth", "gps"].iter().map(|s| s.to_string()).collect();
        let mut channel_map = HashMap::new();
        channel_map.insert("port".to_string(), "SideScan Port".to_string());
        channel_map.insert("starboard".to_string(), "SideScan Starboard".to_string());
        channel_map.insert("down".to_string(), "DownScan".to_string());
        channel_map.insert("temp".to_string(), "Temperature".to_string());
        channel_map.insert("depth".to_string(), "Depth".to_string());
        channel_map.insert("gps".to_string(), "GPS".to_string());
        Self { known_channels, channel_map, dynamic_channel_map: HashMap::new() }
    }


    /// Parse RSD file with firmware float hits for auto-mapping
    pub fn parse_file_with_firmware<P: AsRef<Path>>(&mut self, file_path: P, float_hits: &[(usize, f32)]) -> RSDParseResult {
        // Combined mapping logic: signature probe, pattern lab, firmware float analysis
        let mut detected_channels = HashMap::<u32, usize>::new();
        let mut unknown_channels: Vec<String> = vec![];
        let mut record_count = 0;
        let success = true;
        let error_message = None;
        let mut diagnostics: Vec<String> = Vec::new();
        let path = file_path.as_ref();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        // Add UHD/UHD2 magics and expand candidate list
        let mut candidate_magics = vec![
            0xB7E9DA86u32, // Pre-UHD
            0xB7E9DA87u32, // UHD
            0xB7E9DA88u32, // UHD2
            0xB7E9DA89u32, // Future/variant
        ];
        if ext == "rsd" {
            if let Ok(lines) = std::fs::read_to_string("garmin_magic_variants.txt") {
                for line in lines.lines() {
                    let l = line.trim().to_lowercase();
                    if l.is_empty() || l.starts_with('#') { continue; }
                    let l = l.strip_prefix("0x").unwrap_or(&l);
                    if let Ok(val) = u32::from_str_radix(l, 16) {
                        if !candidate_magics.contains(&val) {
                            candidate_magics.push(val);
                        }
                    }
                }
            }
        }
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                diagnostics.push(format!("Failed to open file: {}", e));
                return RSDParseResult {
                    success: false,
                    error_message: Some(format!("Failed to open file: {}", e)),
                    record_count: 0,
                    channels: vec![],
                    unknown_channels: vec![],
                }
            }
        };
        let mut reader = BufReader::new(file);
        let mut buf = vec![];
        if let Err(e) = reader.read_to_end(&mut buf) {
            diagnostics.push(format!("Failed to read file: {}", e));
            return RSDParseResult {
                success: false,
                error_message: Some(format!("Failed to read file: {}", e)),
                record_count: 0,
                channels: vec![],
                unknown_channels: vec![],
            }
        }
        // Signature probe: scan for recurring 4-byte signatures
        let mut signature_counts = HashMap::<u32, usize>::new();
        let mut pos = 0;
        while pos + 4 <= buf.len() {
            let word = u32::from_le_bytes([buf[pos], buf[pos+1], buf[pos+2], buf[pos+3]]);
            *signature_counts.entry(word).or_insert(0) += 1;
            pos += 4;
        }
        // Pattern lab: scan for record headers/trailers and float signatures
        let mut header_offsets = vec![];
        let mut trailer_offsets = vec![];
        let mut float_hits_found = vec![];
        let known_floats = [8.43, 10.09, 11.79, 24.72, 14.10, 29.01, 40.05, 13.50, 13.30, 13.40, 4.20, 4.10, 6.80, 40.04];
        pos = 0;
        while pos + 4 <= buf.len() {
            let word = u32::from_le_bytes([buf[pos], buf[pos+1], buf[pos+2], buf[pos+3]]);
            if candidate_magics.contains(&word) {
                header_offsets.push(pos);
            }
            if word == 0xF98EACBC {
                trailer_offsets.push(pos);
            }
            let val = f32::from_le_bytes([buf[pos], buf[pos+1], buf[pos+2], buf[pos+3]]);
            for &target in &known_floats {
                if (val - target).abs() < 0.0001 {
                    float_hits_found.push((pos, val));
                }
            }
            pos += 4;
        }
        // Parse records and map channels
        // Dynamic record parsing inspired by Python next-gen parser
        pos = 0x5000;
        while pos + 32 < buf.len() {
            let magic = u32::from_le_bytes([buf[pos], buf[pos+1], buf[pos+2], buf[pos+3]]);
            if candidate_magics.contains(&magic) {
                // Try to parse header/body as variable struct
                let mut channel_id = None;
                let mut auto_mapped_label: Option<String> = None;
                // Try field 0 (channel_id) at offset +8, +12, +16, +20 (UHD/UHD2 may shift)
                let offsets = [8, 12, 16, 20];
                let mut raw_values = vec![];
                for &off in &offsets {
                    if pos + off + 4 <= buf.len() {
                        let val = u32::from_le_bytes([
                            buf[pos+off], buf[pos+off+1], buf[pos+off+2], buf[pos+off+3]
                        ]);
                        raw_values.push(val);
                        // Heuristic: channel_id is usually < 1000
                        if channel_id.is_none() && val < 1000 {
                            channel_id = Some(val);
                        }
                    }
                }
                // Fallback: try to parse as signed int (for negative IDs)
                if channel_id.is_none() && pos + 8 + 4 <= buf.len() {
                    let val = i32::from_le_bytes([
                        buf[pos+8], buf[pos+9], buf[pos+10], buf[pos+11]
                    ]);
                    if val >= 0 && val < 1000 {
                        channel_id = Some(val as u32);
                    }
                }
                // Auto-mapping logic: try to match floats at offsets to known/fmw hits
                if channel_id.is_none() {
                    // Use both firmware float hits (argument) and floats found in file
                    let mut all_float_hits = float_hits.to_vec();
                    all_float_hits.extend_from_slice(&float_hits_found);
                    if let Some(mapped) = self.auto_map_channel_id(&raw_values, &all_float_hits) {
                        auto_mapped_label = Some(mapped.clone());
                        diagnostics.push(format!("Auto-mapped channel at offset 0x{:X}: {}", pos, mapped));
                    } else {
                        let mut values = vec![];
                        for (i, &raw) in raw_values.iter().enumerate() {
                            values.push(format!("off {}: u32={} f32={}", [8,12,16,20][i], raw, f32::from_bits(raw)));
                        }
                        diagnostics.push(format!("Could not determine channel_id at offset 0x{:X} | possible values: {}", pos, values.join(", ")));
                    }
                }
                if let Some(chid) = channel_id {
                    *detected_channels.entry(chid).or_insert(0) += 1;
                    record_count += 1;
                } else if let Some(label) = auto_mapped_label {
                    // Use a synthetic channel id for auto-mapped
                    let synth_id = 10000 + (pos as u32 % 10000);
                    *detected_channels.entry(synth_id).or_insert(0) += 1;
                    record_count += 1;
                    unknown_channels.push(label);
                }
                // Move to next record (UHD/UHD2 records may be larger)
                pos += 1024;
            } else {
                pos += 4;
            }
        }
        // Map channel IDs to names using signature probe, float hits, and known tables
        let mut channels = vec![];
        for (ch_id, _count) in detected_channels.iter() {
            // Expanded mapping for UHD/UHD2
            let name = match ch_id {
                0 => "port",
                1 => "starboard",
                2 => "down",
                3 => "temp",
                4 => "depth",
                5 => "gps",
                6 => "uhd_side",
                7 => "uhd_down",
                8 => "uhd2_side",
                9 => "uhd2_down",
                _ => "unknown",
            };
            let mapped_type = if self.known_channels.contains(name) {
                self.channel_map.get(name).cloned()
            } else {
                if let Some(sig_count) = signature_counts.get(ch_id) {
                    Some(format!("Signature-mapped: {} (count {})", ch_id, sig_count))
                } else {
                    match ch_id {
                        6 => Some("UHD SideScan".to_string()),
                        7 => Some("UHD DownScan".to_string()),
                        8 => Some("UHD2 SideScan".to_string()),
                        9 => Some("UHD2 DownScan".to_string()),
                        _ => None,
                    }
                }
            };
            if name == "unknown" {
                unknown_channels.push(format!("channel_{}", ch_id));
            }
            channels.push(ChannelInfo {
                name: name.to_string(),
                detected: true,
                mapped_type,
            });
        }
        // Self-healing: try to map unknown channels
        for ch in &unknown_channels {
            if let Some(mapped) = Self::heuristic_channel_mapping(ch) {
                self.dynamic_channel_map.insert(ch.clone(), mapped.clone());
                for chan in channels.iter_mut() {
                    if &chan.name == ch {
                        chan.mapped_type = Some(mapped.clone());
                    }
                }
            }
        }
        if !diagnostics.is_empty() {
            println!("Diagnostics callback: {} issues detected", diagnostics.len());
            for msg in diagnostics.iter() {
                println!("[DIAGNOSTIC] {}", msg);
            }
        }
        RSDParseResult {
            success,
            error_message,
            record_count,
            channels,
            unknown_channels,
        }
    }
    /// Legacy parse_file for compatibility (calls parse_file_with_firmware with empty float_hits)
    pub fn parse_file<P: AsRef<Path>>(&mut self, file_path: P) -> RSDParseResult {
        self.parse_file_with_firmware(file_path, &[])
    }

    /// Heuristic mapping for unknown channels (expand as needed)
        fn heuristic_channel_mapping(channel: &str) -> Option<String> {
            // Expanded heuristic for port/starboard/down/uhd/side/left/right
            let lc = channel.to_lowercase();
            if lc.contains("port") || lc.contains("left") ||
                (lc.contains("side") && (lc.contains("port") || lc.contains("left"))) ||
                lc.contains("sidevu port") || lc.contains("side scan port") ||
                (lc.contains("gt") && (lc.contains("port") || lc.contains("left"))) {
                Some("Port (Heuristic)".to_string())
            } else if lc.contains("starboard") || lc.contains("right") ||
                (lc.contains("side") && (lc.contains("starboard") || lc.contains("right"))) ||
                lc.contains("sidevu starboard") || lc.contains("side scan starboard") ||
                (lc.contains("gt") && (lc.contains("starboard") || lc.contains("right"))) {
                Some("Starboard (Heuristic)".to_string())
            } else if lc.contains("down") || lc.contains("downscan") ||
                (lc.contains("uhd") && lc.contains("down")) ||
                lc.contains("uhd down") || lc.contains("uhd2 down") {
                Some("DownScan (Heuristic)".to_string())
            } else if (lc.contains("uhd") && lc.contains("side")) || lc.contains("uhd side") || lc.contains("uhd2 side") {
                Some("UHD SideScan (Heuristic)".to_string())
            } else if lc.contains("sidevu") || (lc.contains("side") && !lc.contains("port") && !lc.contains("starboard")) {
                Some("SideScan (Heuristic)".to_string())
            } else if lc.contains("temp") {
                Some("Temperature (Heuristic)".to_string())
            } else if lc.contains("depth") {
                Some("Depth (Heuristic)".to_string())
            } else if lc.contains("gps") {
                Some("GPS (Heuristic)".to_string())
            } else if lc.contains("chirp") {
                Some("CHIRP (Heuristic)".to_string())
            } else if lc.contains("unknown") || lc.contains("newch") {
                Some("Unknown Channel (Heuristic)".to_string())
            } else {
                None
            }
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use serde_json;

    #[test]
    fn test_parser_real_file() {
        let mut parser = GarminRSDParser::new();
        // Use a real RSD file path for testing (update as needed)
        let test_path = "D:/Temp/cesarops_repo_tmp/Garminjunk/HummSucker/Garmin-Rsd/R00012/R00012_000.RSD";
        let result = parser.parse_file(test_path);
        assert!(result.success, "Parser failed: {:?}", result.error_message);
        assert!(result.record_count > 0, "No records detected");
        // Output JSON summary for inspection
        let json = serde_json::to_string_pretty(&result).unwrap();
        let _ = fs::write("test_rsd_summary.json", &json);
        println!("{}", json);
    }
}
