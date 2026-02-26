/// Humminbird DAT + SON multi-file sonar parser.
///
/// File layout
/// -----------
/// A recording session consists of:
///   <name>.DAT   – session metadata (64, 96 or 100 bytes depending on device generation)
///   B000.SON     – primary channel (83 kHz)
///   B001.SON     – secondary channel (200 kHz)
///   B002.SON     – starboard sidescan
///   B003.SON     – port sidescan
///   B004.SON     – downscan
///
/// The parser accepts any file extension (.DAT or .SON) and locates sibling files
/// automatically; pass the .DAT for best results or any single .SON file.
///
/// DAT format
/// ----------
///   64 bytes  → Helix / pre-Solix, big-endian
///   96 bytes  → Solix, little-endian
///   100 bytes → Solix variant, little-endian
///
/// SON ping record
/// ---------------
///   Magic:   C0 DE AB 21  (u32 BE = 3235818273)
///   Header:  variable length (67, 72 or 152 bytes), detected by scanning for
///            the end-of-header sentinel byte 0x21 (33) after spacer 0xA0 (160).
///   After the header:  `record_length` bytes of sonar samples.
///
/// GPS coordinate encoding
/// -----------------------
///   International 1924 ellipsoid  (equatorial radius 6378388.0 m).
///   Humminbird stores lat/lon as degrees * 10,000,000 (i32 LE), but uses the
///   1924 ellipsoid conversion factor in their original firmware computations.
///   For normal display purposes dividing by 10,000,000 gives WGS84-close values.
///
/// Beam mapping (spacer 0x50 / byte 80 in the header)
///   0  → 83 kHz   primary
///   1  → 200 kHz  secondary
///   2  → port sidescan
///   3  → starboard sidescan
///   4  → downscan

use crate::garmin_rsd_parser::{ChannelInfo, ParseResult, Ping};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// SON record magic (big-endian u32)
const SON_MAGIC: u32 = 0xC0DEAB21;

// ── byte helpers ──────────────────────────────────────────────────────────────

#[inline]
fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
#[inline]
fn be_u32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
#[inline]
fn le_i32(b: &[u8]) -> i32 {
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
#[inline]
fn be_i32(b: &[u8]) -> i32 {
    i32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
#[inline]
fn le_f32(b: &[u8]) -> f32 {
    f32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

// ── sibling file discovery ─────────────────────────────────────────────────────

/// Given any path inside the recording folder, return the list of .SON files to
/// process.  Also return whether the DAT is big-endian (Helix) or little-endian
/// (Solix).
fn find_son_files(path: &Path) -> (Vec<PathBuf>, bool) {
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();

    // Determine endianness from the DAT file if we can find it
    let mut big_endian = true; // Helix default
    if let Some(dat_path) = find_dat_sibling(path) {
        if let Ok(dat) = std::fs::read(&dat_path) {
            big_endian = dat.len() == 64; // 64 = Helix/BE, 96/100 = Solix/LE
        }
    }

    // If we were given a .SON file, also look for siblings B000-B004 in the same dir
    if stem.starts_with('b') && stem.len() == 4 {
        let sons: Vec<PathBuf> = (0u8..=4)
            .map(|i| dir.join(format!("B{:03}.SON", i)))
            .filter(|p| p.exists())
            .collect();
        return (sons, big_endian);
    }

    // Given a .DAT or anything else: look for B000-B004.SON
    let sons: Vec<PathBuf> = (0u8..=4)
        .map(|i| dir.join(format!("B{:03}.SON", i)))
        .filter(|p| p.exists())
        .collect();

    if sons.is_empty() {
        // Try case-insensitive directory scan
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut found: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    name.starts_with('b') && name.ends_with(".son") && name.len() == 8
                })
                .map(|e| e.path())
                .collect();
            found.sort();
            return (found, big_endian);
        }
    }

    (sons, big_endian)
}

fn find_dat_sibling(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    if ext == "dat" {
        return Some(path.to_path_buf());
    }
    // Walk the directory looking for a .DAT file
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.extension().and_then(|x| x.to_str())
               .map(|s| s.eq_ignore_ascii_case("dat")).unwrap_or(false) {
                return Some(p);
            }
        }
    }
    None
}

// ── header length detection ────────────────────────────────────────────────────

/// Scan the first SON record to detect header length (67, 72, or 152 bytes).
/// The end of the header is marked by spacer 0xA0 followed by byte 0x21.
fn detect_header_length(bytes: &[u8], magic_pos: usize) -> usize {
    let record_start = magic_pos + 4; // skip 4-byte magic
    // Try scanning for the sentinel pair (0xA0, 0x21) within a reasonable window
    let scan_end = (record_start + 200).min(bytes.len());
    for i in record_start..scan_end.saturating_sub(1) {
        if bytes[i] == 0xA0 && bytes[i + 1] == 0x21 {
            // Header length is from magic to end of sentinel (inclusive both bytes)
            let hlen = (i + 2) - magic_pos;
            // Known valid lengths; clamp to nearest known value
            if hlen <= 70 { return 67; }
            if hlen <= 74 { return 72; }
            return 152;
        }
    }
    67 // fallback
}

// ── SOB record field extraction ───────────────────────────────────────────────

/// `spacer_value` locates: within the header `start..start+hlen`, scan for the
/// byte that equals `spacer`, return the byte that follows it (if any).
fn spacer_byte(hdr: &[u8], spacer: u8) -> Option<u8> {
    for i in 0..hdr.len().saturating_sub(1) {
        if hdr[i] == spacer {
            return Some(hdr[i + 1]);
        }
    }
    None
}

/// Read i32 at a fixed offset within the header, handling big-endian vs LE.
fn read_i32_at(hdr: &[u8], offset: usize, big_endian: bool) -> Option<i32> {
    if offset + 4 > hdr.len() { return None; }
    Some(if big_endian { be_i32(&hdr[offset..offset+4]) } else { le_i32(&hdr[offset..offset+4]) })
}

fn read_u32_at(hdr: &[u8], offset: usize, big_endian: bool) -> Option<u32> {
    if offset + 4 > hdr.len() { return None; }
    Some(if big_endian { be_u32(&hdr[offset..offset+4]) } else { le_u32(&hdr[offset..offset+4]) })
}

fn read_f32_at(hdr: &[u8], offset: usize) -> Option<f32> {
    if offset + 4 > hdr.len() { return None; }
    Some(le_f32(&hdr[offset..offset+4]))
}

// ── beam info ─────────────────────────────────────────────────────────────────

/// Map Humminbird beam ID → (channel_id, mapped_type)
fn beam_to_channel(beam: u8) -> (u32, &'static str) {
    match beam {
        0 => (0, "primary"),
        1 => (1, "secondary"),
        2 => (2, "port_sidescan"),
        3 => (3, "starboard_sidescan"),
        4 => (4, "chirp_downscan"),
        _ => (99, "unknown"),
    }
}

/// Guess beam from filename: B000→0, B001→1, B002→2, B003→3, B004→4
fn beam_from_filename(path: &Path) -> u8 {
    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
    if let Some(digits) = name.strip_prefix('b') {
        if let Ok(n) = digits.parse::<u8>() {
            return n;
        }
    }
    0
}

// ── parse a single SON file ────────────────────────────────────────────────────

fn parse_son_file(
    son_path: &Path,
    big_endian: bool,
    channel_counts: &mut BTreeMap<u32, usize>,
    pings: &mut Vec<Ping>,
    sequence: &mut u32,
) {
    let bytes = match std::fs::read(son_path) {
        Ok(b) => b,
        Err(_) => return,
    };

    if bytes.len() < 8 { return; }

    // Detect header length from first record
    let Some(first_magic) = find_son_magic(&bytes, 0) else { return };
    let hlen = detect_header_length(&bytes, first_magic);
    let default_beam = beam_from_filename(son_path);

    let mut pos = first_magic;

    while pos + hlen <= bytes.len() {
        // Verify magic
        if pos + 4 > bytes.len() { break; }
        let magic = u32::from_be_bytes([bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3]]);
        if magic != SON_MAGIC {
            // Re-sync
            match find_son_magic(&bytes, pos + 1) {
                Some(next) => { pos = next; continue; }
                None => break,
            }
        }

        let record_start = pos;
        let hdr_start = pos + 4; // skip 4-byte magic
        if hdr_start + hlen > bytes.len() { break; }
        let hdr = &bytes[hdr_start..hdr_start + hlen];

        // ── extract navigation fields ─────────────────────────────────────
        // Known offsets (from PINGVerter / Humminbird reverse-engineering):
        //   +4  record_length u32 (bytes of sonar data)
        //   +8  packet_size? same in many versions
        //   +12 spacer?
        //   Beam: spacer 0x50 (80), next byte is beam id
        //   Lat:  spacer 0x22 (34), next 4 bytes i32 (/10_000_000 → degrees)
        //   Lon:  spacer 0x26 (38), next 4 bytes i32 (/10_000_000 → degrees)
        //   Depth: spacer 0x28 (40), next 4 bytes f32 (cm → m)
        //   Time:  spacer 0x26? — need to cross-ref; use record index as timestamp

        // Record length 
        let record_length = if hlen >= 8 {
            read_u32_at(hdr, 0, big_endian).unwrap_or(0) as usize
        } else {
            0
        };

        // Beam from spacer 0x50 or fallback from filename
        let beam = spacer_byte(hdr, 0x50).unwrap_or(default_beam);
        let (ch_id, _mapped_type) = beam_to_channel(beam);

        // GPS: Humminbird stores lat/lon as degrees * 10_000_000 (i32)
        // Locate them by spacer scanning – spacer 0x22 → lat, 0x26 → lon
        // (Some firmwares differ; use fixed offsets as fallback)
        let lat = if let Some(raw) = spacer_i32(hdr, 0x22, big_endian) {
            raw as f64 / 10_000_000.0
        } else if hlen >= 44 {
            read_i32_at(hdr, 36, big_endian).unwrap_or(0) as f64 / 10_000_000.0
        } else {
            0.0
        };
        let lon = if let Some(raw) = spacer_i32(hdr, 0x26, big_endian) {
            raw as f64 / 10_000_000.0
        } else if hlen >= 48 {
            read_i32_at(hdr, 40, big_endian).unwrap_or(0) as f64 / 10_000_000.0
        } else {
            0.0
        };

        // Depth in cm → m
        let depth_cm = spacer_f32(hdr, 0x28).unwrap_or(0.0);
        let depth_m  = depth_cm / 100.0;
        let depth_ft = depth_m * 3.28084;

        // Sonar data follows the header
        let sonar_start = hdr_start + hlen;
        let sonar_end   = (sonar_start + record_length).min(bytes.len());
        let sonar_bytes = &bytes[sonar_start..sonar_end];

        let ping = Ping {
            file_offset:    record_start,
            sequence:       *sequence,
            timestamp_ms:   *sequence as u64 * 40, // ~25 Hz; real time needs firmware-specific parsing
            latitude:       lat,
            longitude:      lon,
            depth_m,
            depth_ft,
            altitude_m:     0.0,
            temp_c:         None, // temperature parsing varies by firmware version
            beam_angle_deg: 0.0,
            channel:        ch_id,
            sample_count:   sonar_bytes.len(),
            sonar_offset:   sonar_start,
            sonar_size:     sonar_bytes.len(),
            sample_format:  "humminbird/u8".to_string(),
            samples:        sonar_bytes.iter().map(|&b| (b as u16) * 257).collect(),
        };

        *channel_counts.entry(ch_id).or_insert(0) += 1;
        pings.push(ping);
        *sequence += 1;

        // Advance: skip magic + header + record_length
        let step = 4 + hlen + record_length;
        if step == 0 { break; }
        pos = record_start + step;
    }
}

/// Scan for SON_MAGIC starting at `from`.
fn find_son_magic(bytes: &[u8], from: usize) -> Option<usize> {
    let magic_bytes = [0xC0u8, 0xDE, 0xAB, 0x21];
    let limit = bytes.len().saturating_sub(4);
    for i in from..=limit {
        if bytes[i..i+4] == magic_bytes {
            return Some(i);
        }
    }
    None
}

/// Scan the header for spacer `s`, read the following 4 bytes as i32.
fn spacer_i32(hdr: &[u8], spacer: u8, big_endian: bool) -> Option<i32> {
    for i in 0..hdr.len().saturating_sub(4) {
        if hdr[i] == spacer {
            let b = &hdr[i+1..i+5];
            return Some(if big_endian { be_i32(b) } else { le_i32(b) });
        }
    }
    None
}

/// Scan the header for spacer `s`, read the following 4 bytes as f32 LE.
fn spacer_f32(hdr: &[u8], spacer: u8) -> Option<f32> {
    for i in 0..hdr.len().saturating_sub(4) {
        if hdr[i] == spacer {
            return Some(le_f32(&hdr[i+1..i+5]));
        }
    }
    None
}

// ── public entry point ────────────────────────────────────────────────────────

/// Parse a Humminbird recording given any file in the recording folder (.DAT
/// preferred; any .SON also works).  All sibling SON files are processed.
pub fn parse_file(path: &Path) -> ParseResult {
    if !path.exists() {
        return err(&format!("File not found: {}", path.display()));
    }

    let (son_files, big_endian) = find_son_files(path);

    if son_files.is_empty() {
        return err(&format!(
            "No Humminbird SON files found near: {}",
            path.display()
        ));
    }

    let endian_label = if big_endian { "Helix/BE" } else { "Solix/LE" };

    let mut pings:          Vec<Ping>                   = Vec::new();
    let mut channel_counts: BTreeMap<u32, usize>        = BTreeMap::new();
    let mut sequence:       u32                         = 0;

    for son_path in &son_files {
        parse_son_file(son_path, big_endian, &mut channel_counts, &mut pings, &mut sequence);
    }

    if pings.is_empty() {
        return err("Humminbird SON files found but no valid ping records decoded");
    }

    let channels = build_channel_list(&channel_counts, endian_label);
    let record_count = pings.len();

    ParseResult {
        record_count,
        recovered_records:    0,
        dropped_bytes:        0,
        parser_magic:         format!("Humminbird/{endian_label}"),
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

fn build_channel_list(counts: &BTreeMap<u32, usize>, variant: &str) -> Vec<ChannelInfo> {
    counts.keys().map(|&id| {
        let (name, mtype) = match id {
            0 => ("Primary 83 kHz",       Some("primary")),
            1 => ("Secondary 200 kHz",    Some("secondary")),
            2 => ("Port Sidescan",         Some("port_sidescan")),
            3 => ("Starboard Sidescan",    Some("starboard_sidescan")),
            4 => ("Downscan",              Some("chirp_downscan")),
            _ => ("Unknown",               None),
        };
        ChannelInfo {
            id,
            name:         format!("Humminbird {name}"),
            detected:     true,
            mapped_type:  mtype.map(str::to_string),
            generation:   Some(format!("humminbird-{variant}")),
        }
    }).collect()
}

fn err(msg: &str) -> ParseResult {
    ParseResult {
        record_count:          0,
        recovered_records:     0,
        dropped_bytes:         0,
        parser_magic:          "Humminbird".into(),
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
