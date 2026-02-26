/// Lowrance SL2 / SL3 sidescan & downscan parser.
///
/// File layout
/// -----------
///   8-byte file header  (format u16 LE, version u16 LE, bytes_per_sounding u16 LE, 2 unused bytes)
///   N frames, each frame beginning at the offset stored in `frame_offset` (SL2) or the
///   running offset computed from `frame_size`.
///
/// SL2  – 144-byte frame header  + `packet_size` sonar bytes  (format id 2)
/// SL3  – 168-byte frame header  + `packet_size` sonar bytes  (format id 3)
///
/// GPS coordinate encoding
/// -----------------------
///   utm_e / utm_n are NOT standard UTM – they are Mercator metres on a sphere
///   with radius 6356752.3142 m (same as Lowrance's legacy spheroid).
///   lat = (2 * atan(exp(utm_n / R)) - π/2) * (180/π)
///   lon = utm_e / R * (180/π)
///
/// Survey types (beam numbering)
///   0  primary   (83 kHz side-facing)
///   1  secondary (200 kHz side-facing or 2nd freq)
///   2  downlonscan / StructureScan DS
///   3  port sidescan
///   4  starboard sidescan
///   5  merged sidescan (port+star interleaved) – we split internally

use crate::garmin_rsd_parser::{ChannelInfo, ParseResult, Ping};
use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::path::Path;

const LOWRANCE_SPHERE_R: f64 = 6_356_752.3142;

// SL2 magic in the first two bytes of the file header (format field)
const SL2_FORMAT: u16 = 2;
const SL3_FORMAT: u16 = 3;

// ── frame geometry ───────────────────────────────────────────────────────────

const SL2_FRAME_HDR: usize = 144;
const SL3_FRAME_HDR: usize = 168;
const FILE_HDR_SIZE: usize = 8;

/// Frequency labels matching Lowrance's frequency_type field.
fn freq_label(ft: u8) -> &'static str {
    match ft {
        0 => "200kHz",
        1 => "50kHz",
        2 => "83kHz",
        3 => "455kHz",
        4 => "800kHz",
        5 => "38kHz",
        6 => "28kHz",
        7 => "130kHz-210kHz",
        8 => "90kHz-150kHz",
        9 => "40kHz-60kHz",
        10 => "25kHz-45kHz",
        _ => "unknown",
    }
}

/// Survey-type → (channel_id_we_assign, beam label, mapped_type)
fn survey_channel(survey_type: u8) -> (u32, &'static str, &'static str) {
    match survey_type {
        0 => (0, "primary", "primary"),
        1 => (1, "secondary", "secondary"),
        2 => (2, "downscan", "chirp_downscan"),
        3 => (3, "port_sidescan", "port_sidescan"),
        4 => (4, "starboard_sidescan", "starboard_sidescan"),
        5 => (3, "merged_sidescan", "port_sidescan"), // first half port, second half star
        _ => (99, "unknown", "unknown"),
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

#[inline]
fn le_u16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
#[inline]
fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
#[inline]
fn le_i32(b: &[u8]) -> i32 {
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
#[inline]
fn le_f32(b: &[u8]) -> f32 {
    f32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn lowrance_to_wgs84(utm_e: f64, utm_n: f64) -> (f64, f64) {
    let lat = (2.0 * ((utm_n / LOWRANCE_SPHERE_R).exp()).atan() - PI / 2.0) * (180.0 / PI);
    let lon = utm_e / LOWRANCE_SPHERE_R * (180.0 / PI);
    (lat, lon)
}

// ── public entry point ────────────────────────────────────────────────────────

/// Parse a Lowrance SL2 or SL3 file.  Returns a `ParseResult` using the same
/// schema as the Garmin parser so the rest of the pipeline is format-agnostic.
pub fn parse_file(path: &Path) -> ParseResult {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return err(&format!("Cannot read file: {e}")),
    };

    if bytes.len() < FILE_HDR_SIZE + 16 {
        return err("File too small to be a Lowrance SL2/SL3 log");
    }

    // ── file header ──────────────────────────────────────────────────────────
    let format_id = le_u16(&bytes[0..2]);
    let frame_hdr_size = match format_id {
        SL2_FORMAT => SL2_FRAME_HDR,
        SL3_FORMAT => SL3_FRAME_HDR,
        _ => return err(&format!("Unrecognised Lowrance format id {format_id:#06x} (expected 0x0002 SL2 or 0x0003 SL3)")),
    };
    let format_name = if format_id == SL2_FORMAT { "SL2" } else { "SL3" };

    let mut pings: Vec<Ping> = Vec::new();
    let mut channel_counts: BTreeMap<u32, usize> = BTreeMap::new();
    let mut dropped_bytes = 0usize;
    let mut sequence = 0u32;

    let mut offset = FILE_HDR_SIZE;

    while offset + frame_hdr_size <= bytes.len() {
        let fh = &bytes[offset..offset + frame_hdr_size];

        // ── frame header fields (same offsets for SL2 and SL3 up to byte 143) ──
        // Offset  0: frame_offset u32 (absolute byte offset of this frame – sanity check)
        // Offset  4: last_primary_channel_frame_offset u32
        // Offset  8: last_secondary_channel_frame_offset u32
        // Offset 12: last_downscan_channel_frame_offset u32
        // Offset 16: last_left_sidescan_channel_frame_offset u32
        // Offset 20: last_right_sidescan_channel_frame_offset u32
        // Offset 24: last_composite_sidescan_channel_frame_offset u32
        // Offset 28: frame_size u16
        // Offset 30: packet_size u16   (sonar sample bytes that follow the header)
        // …
        // Offset 34: id u32            (sequential ping ID from device)
        // Offset 38: min_range u32     (cm)
        // Offset 42: max_range u32     (cm)
        // Offset 50: hardware_time u32 (ms since boot)
        // Offset 54: (unused)
        // Offset 58: depth (f32 feet)
        // Offset 62: (unused)
        // Offset 66: frequency_type u8
        // Offset 80: utm_e i32
        // Offset 84: utm_n i32
        // Offset 88: gps_speed f32 (knots)
        // Offset 92: temperature f32 (°C)  — may be 0x7FC00000 (NaN) when absent
        // Offset 96: track_cog u16  (heading * 100, degrees * 100)
        // Offset 100: altitude f32 (feet)
        // Offset 104: heading u16  (degrees * 100?)
        // Offset 108: time_s u32   (seconds since epoch or device start)
        // Offset 128: survey_type u8  (channel/beam selector)
        //
        // SL3 adds fields at 144-167 (extra nav/attitude data)

        let frame_size    = le_u16(&fh[28..30]) as usize;
        let packet_size   = le_u16(&fh[30..32]) as usize;
        let hardware_time = le_u32(&fh[50..54]);
        let depth_ft      = le_f32(&fh[58..62]);
        let freq_type     =  fh[66];
        let utm_e         = le_i32(&fh[80..84]) as f64;
        let utm_n         = le_i32(&fh[84..88]) as f64;
        let temp_f32      = le_f32(&fh[92..96]);
        let survey_type   = fh[128];

        let (lat, lon) = lowrance_to_wgs84(utm_e, utm_n);

        // Convert NaN / obviously-bad temperature to None
        let temp_c = if temp_f32.is_nan() || temp_f32 < -50.0 || temp_f32 > 50.0 {
            None
        } else {
            Some(temp_f32)
        };

        let depth_m = depth_ft * 0.3048;

        let (ch_id, _ch_label, mapped_type) = survey_channel(survey_type);

        // Handle merged sidescan: split the sample block down the middle assigning
        // the first half to port (ch3) and the second half to starboard (ch4).
        let sonar_start = offset + frame_hdr_size;
        let sonar_end   = (sonar_start + packet_size).min(bytes.len());
        let sonar_bytes = &bytes[sonar_start..sonar_end];

        let base_ping = Ping {
            file_offset:   offset,
            sequence,
            timestamp_ms:  hardware_time as u64,
            latitude:      lat,
            longitude:     lon,
            depth_m,
            depth_ft,
            altitude_m:    0.0,
            temp_c,
            beam_angle_deg: 0.0,
            channel:       ch_id,
            sample_count:  sonar_bytes.len(),
            sonar_offset:  sonar_start,
            sonar_size:    sonar_bytes.len(),
            sample_format: format!("{format_name}/{}", freq_label(freq_type)),
            samples:       sonar_to_u16(sonar_bytes),
        };

        if survey_type == 5 && !sonar_bytes.is_empty() {
            // Split merged sidescan into port (ch3) and starboard (ch4)
            let half = sonar_bytes.len() / 2;
            let port_bytes = &sonar_bytes[..half];
            let star_bytes = &sonar_bytes[half..];

            let mut port = base_ping.clone();
            port.channel      = 3;
            port.sample_count = port_bytes.len();
            port.sonar_size   = port_bytes.len();
            port.sample_format = format!("{format_name}/{}/port_split", freq_label(freq_type));
            port.samples      = sonar_to_u16(port_bytes);
            *channel_counts.entry(3).or_insert(0) += 1;
            pings.push(port);

            let mut star = base_ping.clone();
            star.sequence    += 1;
            star.channel      = 4;
            star.sample_count = star_bytes.len();
            star.sonar_size   = star_bytes.len();
            star.sample_format = format!("{format_name}/{}/star_split", freq_label(freq_type));
            star.samples      = sonar_to_u16(star_bytes);
            *channel_counts.entry(4).or_insert(0) += 1;
            pings.push(star);
        } else {
            let _ = mapped_type; // used for channel table below
            *channel_counts.entry(ch_id).or_insert(0) += 1;
            pings.push(base_ping);
        }

        sequence += 1;

        // advance to next frame – use frame_size if sensible, else step by header alone
        let step = if frame_size >= frame_hdr_size && frame_size <= 8192 {
            frame_size
        } else {
            // frame_size is occasionally 0 in older firmware; use header + sonar
            let fallback = frame_hdr_size + packet_size;
            if fallback == 0 {
                dropped_bytes += 1;
                break;
            }
            fallback
        };

        // Sanity: make sure we always advance
        if step == 0 {
            dropped_bytes += bytes.len() - offset;
            break;
        }
        offset += step;
    }

    // ── build channel list ────────────────────────────────────────────────────
    let channels = build_channel_list(&channel_counts, format_name);
    let record_count = pings.len();

    ParseResult {
        record_count,
        recovered_records: 0,
        dropped_bytes,
        parser_magic: format!("Lowrance/{format_name}"),
        channels,
        channel_counts,
        field_channel_counts: BTreeMap::new(),
        unique_field_values:  BTreeMap::new(),
        unknown_channels:     Vec::new(),
        healing_actions:      Vec::new(),
        error_message:        None,
        pings,
        crc_mismatch_count:   0,
    }
}

fn sonar_to_u16(raw: &[u8]) -> Vec<u16> {
    raw.iter().map(|&b| (b as u16) * 257).collect()
}

fn build_channel_list(counts: &BTreeMap<u32, usize>, format: &str) -> Vec<ChannelInfo> {
    counts.keys().map(|&id| {
        let (name, mtype, gen) = match id {
            0 => ("Primary",          Some("primary"),           "lowrance"),
            1 => ("Secondary",        Some("secondary"),          "lowrance"),
            2 => ("Downscan",         Some("chirp_downscan"),     "lowrance"),
            3 => ("Port Sidescan",    Some("port_sidescan"),      "lowrance"),
            4 => ("Starboard SS",     Some("starboard_sidescan"), "lowrance"),
            _ => ("Unknown",          None,                       "lowrance"),
        };
        ChannelInfo {
            id,
            name:         format!("{format} {name}"),
            detected:     true,
            mapped_type:  mtype.map(str::to_string),
            generation:   Some(format!("{gen}-{format}")),
        }
    }).collect()
}

fn err(msg: &str) -> ParseResult {
    ParseResult {
        record_count:          0,
        recovered_records:     0,
        dropped_bytes:         0,
        parser_magic:          "Lowrance".into(),
        channels:              Vec::new(),
        channel_counts:        BTreeMap::new(),
        field_channel_counts:  BTreeMap::new(),
        unique_field_values:   BTreeMap::new(),
        unknown_channels:      Vec::new(),
        healing_actions:       Vec::new(),
        error_message:         Some(msg.to_string()),
        pings:                 Vec::new(),
        crc_mismatch_count:    0,
    }
}
