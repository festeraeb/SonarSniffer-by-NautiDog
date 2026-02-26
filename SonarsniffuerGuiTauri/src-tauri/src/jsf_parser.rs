/// EdgeTech JSF (Jacobs Sonar Format) sidescan and sub-bottom sonar parser.
///
/// JSF is the native recording format for EdgeTech side-scan and sub-bottom profiler
/// systems (3100-P, 4125, 4200 series, etc.).  
///
/// File structure
/// --------------
/// The file is a stream of variable-length records.  Every record begins with
/// a 16-byte message header:
///
///   Offset 0-1   u16  start_marker    0x1601  (always)
///   Offset 2     u8   version         (typically 0 or 1)
///   Offset 3     u8   session_id      (0 normally)
///   Offset 4-5   u16  message_type    see list below
///   Offset 6     u8   command_type    (0 = data)
///   Offset 7     u8   subsystem_id    (channel/subsystem: 0 = port 75/120kHz,
///                                       1 = star 75/120kHz, 3 = SBP, etc.)
///   Offset 8     u8   channel         (0 = low-freq side, 1 = high-freq side)
///   Offset 9     u8   sequence_number
///   Offset 10-11 u16  reserved
///   Offset 12-15 u32  message_size    (bytes of the message body that follow)
///
/// Then `message_size` bytes of message body.
///
/// Message types of interest
/// -------------------------
///   80  (0x0050)  Sonar Data Message   – sidescan or SBP trace data
///   128 (0x0080)  Navigation Data      – real-time nav
///   182 (0x00B6)  Attitude Data
///
/// Sonar Data Message body (type 80)
/// -----------------------------------
/// (offsets into the body following the 16-byte header)
///   Offset 0-3   u32  time_s          seconds since 1/1/1970
///   Offset 4-7   u32  time_ms         milliseconds
///   Offset 8-11  f32  sampling_interval  (s)
///   Offset 12-15 u32  num_samples     number of sonar samples
///   Offset 16-19 f32  range_scale     (m)
///   Offset 20-23 f32  fish_depth      (m)
///   Offset 24-27 f32  fish_altitude   (m)
///   Offset 28-31 f32  sound_velocity  (m/s)
///   Offset 32-35 f32  lat             WGS84 decimal degrees (may be 0 if no GPS embedded)
///   Offset 36-39 f32  lon
///   Offset 40-43 f32  speed           (m/s)
///   Offset 44-47 f32  heading         (degrees)
///   …  (more fields up to offset 240)
///   Offset 240.. u8[] sonar data      (num_samples × 1 byte or 2 bytes depending on data_format)
///
/// NOTE: The exact body layout depends on the JSF version.  Older receivers use
/// a 240-byte fixed body prefix; newer use padding to 256 bytes.  This parser
/// reads the minimal navigation + sample data from the known-stable fields and
/// is conservative about bounds checks.
///
/// Status: **FUNCTIONAL STUB** — navigation and ping metadata are decoded; raw
/// sonar samples are stored as u8 bytes.  Full gain / TVG compensation is
/// not yet applied.

use crate::garmin_rsd_parser::{ChannelInfo, ParseResult, Ping};
use std::collections::BTreeMap;
use std::path::Path;

const JSF_START_MARKER: u16 = 0x1601;
const JSF_HDR_SIZE: usize   = 16;

// Message types
const MSG_SONAR_DATA: u16   = 80;   // 0x0050
const MSG_NAVIGATION: u16   = 128;  // 0x0080

// Minimum body size for sonar data to have the fields we need
const SONAR_BODY_MIN: usize = 48;

#[inline] fn le_u16(b: &[u8]) -> u16 { u16::from_le_bytes([b[0],b[1]]) }
#[inline] fn le_u32(b: &[u8]) -> u32 { u32::from_le_bytes([b[0],b[1],b[2],b[3]]) }
#[inline] fn le_f32(b: &[u8]) -> f32 { f32::from_le_bytes([b[0],b[1],b[2],b[3]]) }

fn safe_le_f32(b: &[u8], off: usize) -> f32 {
    if off + 4 <= b.len() { le_f32(&b[off..off+4]) } else { 0.0 }
}
fn safe_le_u32(b: &[u8], off: usize) -> u32 {
    if off + 4 <= b.len() { le_u32(&b[off..off+4]) } else { 0 }
}

// ── subsystem → channel mapping ───────────────────────────────────────────────
fn subsystem_channel(subsystem_id: u8) -> (u32, &'static str, &'static str) {
    match subsystem_id {
        0 => (0, "Port Low-Freq",  "port_sidescan"),
        1 => (1, "Star Low-Freq",  "starboard_sidescan"),
        2 => (2, "Port High-Freq", "port_sidescan"),
        3 => (3, "Star High-Freq", "starboard_sidescan"),
        4 => (4, "SBP",            "chirp_downscan"),
        5 => (5, "Port SS 2",      "port_sidescan"),
        6 => (6, "Star SS 2",      "starboard_sidescan"),
        _ => (99,"Unknown",        "unknown"),
    }
}

// ── public entry point ────────────────────────────────────────────────────────

pub fn parse_file(path: &Path) -> ParseResult {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return err(&format!("Cannot read JSF file: {e}")),
    };

    if bytes.len() < JSF_HDR_SIZE {
        return err("File too small for EdgeTech JSF format");
    }

    // Verify at least the first record looks like a JSF message
    let first_marker = le_u16(&bytes[0..2]);
    if first_marker != JSF_START_MARKER {
        return err(&format!(
            "JSF start marker 0x1601 not found at start (got 0x{first_marker:04X})"
        ));
    }

    let mut pings:          Vec<Ping>              = Vec::new();
    let mut channel_counts: BTreeMap<u32, usize>   = BTreeMap::new();
    let mut dropped_bytes:  usize                  = 0;
    let mut sequence:       u32                    = 0;

    let mut pos = 0usize;

    while pos + JSF_HDR_SIZE <= bytes.len() {
        let hdr = &bytes[pos..pos + JSF_HDR_SIZE];

        let start_marker  = le_u16(&hdr[0..2]);
        if start_marker != JSF_START_MARKER {
            // Re-sync
            match find_jsf_marker(&bytes, pos + 1) {
                Some(next) => {
                    dropped_bytes += next - pos;
                    pos = next;
                    continue;
                }
                None => break,
            }
        }

        let msg_type      = le_u16(&hdr[4..6]);
        let subsystem_id  = hdr[7];
        let msg_size      = le_u32(&hdr[12..16]) as usize;

        let body_start    = pos + JSF_HDR_SIZE;
        let body_end      = body_start + msg_size;
        if body_end > bytes.len() {
            // Truncated — stop
            dropped_bytes += bytes.len() - pos;
            break;
        }

        if msg_type == MSG_SONAR_DATA && msg_size >= SONAR_BODY_MIN {
            let body = &bytes[body_start..body_end];

            let time_s      = safe_le_u32(body,  0);
            let time_ms_u32 = safe_le_u32(body,  4);
            let num_samples = safe_le_u32(body, 12) as usize;
            let range_m     = safe_le_f32(body, 16);
            let fish_depth  = safe_le_f32(body, 20);
            let _fish_alt   = safe_le_f32(body, 24);
            let lat         = safe_le_f32(body, 32) as f64;
            let lon         = safe_le_f32(body, 36) as f64;
            let heading     = safe_le_f32(body, 44);

            let timestamp_ms = (time_s as u64) * 1000 + (time_ms_u32 as u64);

            // Sonar samples start after fixed 240-byte prefix (common JSF layout)
            let sonar_off   = 240.min(body.len());
            let sonar_end   = (sonar_off + num_samples).min(body.len());
            let sonar_bytes = &body[sonar_off..sonar_end];

            let (ch_id, _name, _mtype) = subsystem_channel(subsystem_id);
            let depth_m  = fish_depth.abs();
            let depth_ft = depth_m * 3.28084;

            let ping = Ping {
                file_offset:    pos,
                sequence,
                timestamp_ms,
                latitude:       lat,
                longitude:      lon,
                depth_m,
                depth_ft,
                altitude_m:     0.0,
                temp_c:         None,
                beam_angle_deg: heading,
                channel:        ch_id,
                sample_count:   sonar_bytes.len(),
                sonar_offset:   body_start + sonar_off,
                sonar_size:     sonar_bytes.len(),
                sample_format:  format!("jsf/u8/{}m_range", range_m as u32),
                samples:        sonar_bytes.iter().map(|&b| (b as u16) * 257).collect(),
            };

            *channel_counts.entry(ch_id).or_insert(0) += 1;
            pings.push(ping);
            sequence += 1;
        }
        // MSG_NAVIGATION (128): could update cur_lat/lon but JSF embeds GPS in sonar message

        pos = body_end;
    }

    if pings.is_empty() {
        return err("No JSF sonar data messages decoded.  File may use an unsupported version.");
    }

    let channels = build_channel_list(&channel_counts);
    let record_count = pings.len();

    ParseResult {
        record_count,
        recovered_records:    0,
        dropped_bytes,
        parser_magic:         "JSF/0x1601".into(),
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

fn find_jsf_marker(bytes: &[u8], from: usize) -> Option<usize> {
    let limit = bytes.len().saturating_sub(2);
    for i in from..=limit {
        if le_u16(&bytes[i..i+2]) == JSF_START_MARKER {
            return Some(i);
        }
    }
    None
}

fn build_channel_list(counts: &BTreeMap<u32, usize>) -> Vec<ChannelInfo> {
    counts.keys().map(|&id| {
        let (_, name, mtype) = subsystem_channel(id as u8);
        ChannelInfo {
            id,
            name:         format!("JSF {name}"),
            detected:     true,
            mapped_type:  Some(mtype.to_string()),
            generation:   Some("jsf".to_string()),
        }
    }).collect()
}

fn err(msg: &str) -> ParseResult {
    ParseResult {
        record_count:          0,
        recovered_records:     0,
        dropped_bytes:         0,
        parser_magic:          "JSF".into(),
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
