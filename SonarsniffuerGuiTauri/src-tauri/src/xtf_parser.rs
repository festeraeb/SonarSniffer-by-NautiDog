/// Triton XTF (eXtended Triton Format) sonar file parser.
///
/// XTF is a professional survey-grade format used by sidescan sonars, single/multi-
/// beam echo sounders, and navigation systems.  It is maintained by Exail (formerly
/// ECA Group / Triton Imaging).
///
/// File structure
/// --------------
///   1 × 1024-byte XTF file header (contains up to 6 CHANINFO entries)
///   Followed by variable-length packets, each starting with a 14-byte sub-header:
///
/// Packet sub-header (14 bytes, all little-endian)
/// ------------------------------------------------
///   Offset 0-1   u16  magic_number  0xFACE
///   Offset 2     u8   header_type   packet type (0 = sidescan ping, 1 = notes, etc.)
///   Offset 3     u8   sub_channel   sub-channel number
///   Offset 4-5   u16  num_chans_to_follow  (sonar: number of ping channels in this packet)
///   Offset 6-7   u16  bytes_reserved (0)
///   Offset 8-9   u16  bytes_reserved (0)
///   Offset 10-13 u32  bytes_in_packet (total size of whole packet including this header)
///
/// Packet types of interest
///   0   XTFPINGHDR  sidescan / bathymetry ping  (256-byte header)
///   1   XTFNOTESHEADER  text notes
///   42  XTFATTITUDEDATA  attitude / navigation
///   100 XTFRAWCUSTOMHEADER  raw/custom
///
/// Ping header (256 bytes following the 14-byte sub-header, little-endian)
/// -------------------------------------------------------------------------
/// Selected fields (offsets relative to the start of the 256-byte block):
///   Offset  0-1   u16  year
///   Offset  2     u8   month
///   Offset  3     u8   day
///   Offset  4     u8   hour
///   Offset  5     u8   minute
///   Offset  6     u8   second
///   Offset  7     u8   hSeconds (hundredths of second)
///   Offset  8-15  (reserved)
///   Offset 10-11  u16  num_samples_port   (from CHANINFO[0])
///   Offset 12-13  u16  num_samples_stbd
///   Offset 14-15  u16  num_raw_samples_port
///   Offset 16-17  u16  num_raw_samples_stbd
///   Offset 18-19  u16  num_bytes_channel  (currently unused)
///   Offset 20-23  f32  slant_range           (metres)
///   Offset 24-27  f32  ground_range          (metres)
///   Offset 28-31  f32  time_duration
///   Offset 32-35  f32  seconds_per_ping
///   Offset 36-39  f32  sound_velocity        (m/s, usually 1500.0)
///   Offset 40-43  f32  altitude              (m)
///   Offset 44-47  f32 course_over_ground     (degrees)
///   Offset 48-51  f32  heading               (degrees)
///   Offset 52-55  f32  pitch                 (degrees)
///   Offset 56-59  f32  roll                  (degrees)
///   Offset 60-63  f32  heave                 (m)
///   Offset 64-67  f32  yaw                   (degrees)
///   Offset 68-71  u32  record_number
///   Offset 72-75  f32  water_depth           (m)
///   Offset 76-77  u16  reserved1
///   Offset 78-83  (reserved)
///   Offset 84-91  f64  sensor_primary_lat    (decimal degrees, WGS84)
///   Offset 92-99  f64  sensor_primary_lon
///   Offset 100-107 f64 sensor_x_coordinate   (UTM X if not using lat/lon)
///   Offset 108-115 f64 sensor_y_coordinate
///   Offset 116-119 f32 ship_speed            (m/s)
///   Offset 120-123 f32 ship_gyro             (degrees)
///   Offset 124-131 f64 ship_y_coordinate
///   Offset 132-139 f64 ship_x_coordinate
///   Offset 140-141 u16 ship_altitude
///   Offset 142-143 u16 ship_depth
///   Offset 144     u8  fix_time_hour
///   Offset 145     u8  fix_time_minute
///   Offset 146     u8  fix_time_second
///   Offset 147     u8  fix_time_hseconds
///   Offset 148-151 f32 sensor_speed
///   Offset 152-155 f32 camera_tilt
///   Offset 156-159 f32 cable_out
///   Offset 160-163 f32 lay_back
///   Offset 164-167 f32 cable_tension
///   Offset 168-171 f32 sensor_depth          (m)
///   Offset 172-175 f32 sample_frequency      (Hz – sensor-specific)
///   Offset 176-179 f32 vehicle_altitude      (m)
///   Offset 180-183 f32 forward_looking_sonar_range
///   Offset 184-187 f32 qf
///   Offset 188-191 f32 fish_light
///   Offset 192-195 f32 mag_compass
///   Offset 196-199 f32 reserved_f32
///   …
///   Offset 240-241 u16 num_samples (actual)
///   Offset 242-243 u16 reserved
///
/// NOTE: This parser is implemented from the public specification (Rev 36-42).
/// Some fields vary between manufacturer implementations.  This implementation
/// extracts navigation, timing, and geometry data and stores raw sonar samples
/// as-is for downstream rendering.
///
/// Status: **FUNCTIONAL STUB** — navigation, timing, and channel identification
/// are fully parsed.  Sonar sample decoding reads raw bytes but does not apply
/// per-manufacturer gain compensation.

use crate::garmin_rsd_parser::{ChannelInfo, ParseResult, Ping};
use std::collections::BTreeMap;
use std::path::Path;

const XTF_MAGIC: u16 = 0xFACE;
const XTF_FILE_HDR_SIZE: usize = 1024;
const XTF_PACKET_SUBHDR_SIZE: usize = 14;
const XTF_PING_HDR_SIZE: usize = 256;

// Packet header_type values
const PKT_SONAR_PING: u8 = 0;
const PKT_ATTITUDE: u8 = 42;

#[inline] fn le_u16(b: &[u8]) -> u16 { u16::from_le_bytes([b[0], b[1]]) }
#[inline] fn le_u32(b: &[u8]) -> u32 { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) }
#[inline] fn le_f32(b: &[u8]) -> f32 { f32::from_le_bytes([b[0], b[1], b[2], b[3]]) }
#[inline] fn le_f64(b: &[u8]) -> f64 { f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) }

fn safe_le_u16(b: &[u8], off: usize) -> u16 {
    if off + 2 <= b.len() { le_u16(&b[off..off+2]) } else { 0 }
}
fn safe_le_u32(b: &[u8], off: usize) -> u32 {
    if off + 4 <= b.len() { le_u32(&b[off..off+4]) } else { 0 }
}
fn safe_le_f32(b: &[u8], off: usize) -> f32 {
    if off + 4 <= b.len() { le_f32(&b[off..off+4]) } else { 0.0 }
}
fn safe_le_f64(b: &[u8], off: usize) -> f64 {
    if off + 8 <= b.len() { le_f64(&b[off..off+8]) } else { 0.0 }
}

// ── channel numbering ─────────────────────────────────────────────────────────
// XTF sub_channel 0 = port, 1 = starboard (for sidescan)
fn xtf_channel_id(sub_channel: u8) -> u32 { sub_channel as u32 }

fn xtf_channel_type(sub_channel: u8) -> &'static str {
    match sub_channel {
        0 => "port_sidescan",
        1 => "starboard_sidescan",
        _ => "unknown",
    }
}

// ── public entry point ────────────────────────────────────────────────────────

pub fn parse_file(path: &Path) -> ParseResult {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return err(&format!("Cannot read XTF file: {e}")),
    };

    if bytes.len() < XTF_FILE_HDR_SIZE {
        return err("File too small; XTF requires a 1024-byte file header");
    }

    // ── file header sanity ────────────────────────────────────────────────────
    // File header byte 0-1 contains 0x00 (not the packet magic) in older revisions;
    // newer revisions store 0xFACE at the very first two bytes only in packets, not
    // the file header.  The reliable identifier is the format type at offset 2.
    // We accept the file as XTF if EITHER the file-header magic is 0xFACE OR the
    // first packet magic (at offset 1024) is 0xFACE.
    let file_hdr_magic = le_u16(&bytes[0..2]);
    let first_pkt_magic = if bytes.len() >= XTF_FILE_HDR_SIZE + 2 {
        le_u16(&bytes[XTF_FILE_HDR_SIZE..XTF_FILE_HDR_SIZE + 2])
    } else {
        0
    };

    if file_hdr_magic != XTF_MAGIC && first_pkt_magic != XTF_MAGIC {
        return err(&format!(
            "XTF magic 0xFACE not found (file hdr: 0x{file_hdr_magic:04X}, first pkt: 0x{first_pkt_magic:04X})"
        ));
    }

    let mut pings:          Vec<Ping>              = Vec::new();
    let mut channel_counts: BTreeMap<u32, usize>   = BTreeMap::new();
    let mut dropped_bytes:  usize                  = 0;
    let mut sequence:       u32                    = 0;
    let mut pos = XTF_FILE_HDR_SIZE;

    while pos + XTF_PACKET_SUBHDR_SIZE <= bytes.len() {
        let sub = &bytes[pos..pos + XTF_PACKET_SUBHDR_SIZE];

        let magic       = le_u16(&sub[0..2]);
        if magic != XTF_MAGIC {
            // Look ahead for next packet magic
            match find_xtf_magic(&bytes, pos + 1) {
                Some(next) => {
                    dropped_bytes += next - pos;
                    pos = next;
                    continue;
                }
                None => break,
            }
        }

        let header_type      = sub[2];
        let sub_channel      = sub[3];
        let _num_chans       = le_u16(&sub[4..6]);
        let bytes_in_packet  = le_u32(&sub[10..14]) as usize;

        if bytes_in_packet == 0 || pos + bytes_in_packet > bytes.len() {
            // Try to re-sync
            match find_xtf_magic(&bytes, pos + 2) {
                Some(next) => {
                    dropped_bytes += next - pos;
                    pos = next;
                    continue;
                }
                None => break,
            }
        }

        if header_type == PKT_SONAR_PING {
            let ping_hdr_start = pos + XTF_PACKET_SUBHDR_SIZE;
            if ping_hdr_start + XTF_PING_HDR_SIZE <= bytes.len() {
                let ph = &bytes[ping_hdr_start..ping_hdr_start + XTF_PING_HDR_SIZE];

                let year_u16  = safe_le_u16(ph, 0);
                let month     = ph[2];
                let day       = ph[3];
                let hour      = ph[4];
                let minute    = ph[5];
                let second    = ph[6];
                let hseconds  = ph[7];

                // Build a millisecond timestamp from date/time fields.
                // Use chrono if available; otherwise estimate from sequence.
                let timestamp_ms = timestamp_to_ms(
                    year_u16, month, day, hour, minute, second, hseconds,
                );

                let slant_range   = safe_le_f32(ph, 20);
                let _ground_range = safe_le_f32(ph, 24);
                let heading       = safe_le_f32(ph, 48);
                let water_depth   = safe_le_f32(ph, 72);
                let lat           = safe_le_f64(ph, 84);
                let lon           = safe_le_f64(ph, 92);
                let sensor_depth  = safe_le_f32(ph, 168);
                let num_samples   = safe_le_u16(ph, 240) as usize;

                let depth_m = if water_depth != 0.0 { water_depth } else { sensor_depth };

                // Sonar data follows the 256-byte ping header
                let sonar_start = ping_hdr_start + XTF_PING_HDR_SIZE;
                let sonar_bytes_avail = bytes_in_packet
                    .saturating_sub(XTF_PACKET_SUBHDR_SIZE + XTF_PING_HDR_SIZE);
                let sonar_end = (sonar_start + sonar_bytes_avail).min(bytes.len());
                let sonar_bytes = &bytes[sonar_start..sonar_end];

                let ch_id = xtf_channel_id(sub_channel);

                let ping = Ping {
                    file_offset:    pos,
                    sequence,
                    timestamp_ms,
                    latitude:       lat,
                    longitude:      lon,
                    depth_m:        depth_m as f32,
                    depth_ft:       depth_m * 3.28084,
                    altitude_m:     0.0,
                    temp_c:         None,
                    beam_angle_deg: heading,
                    channel:        ch_id,
                    sample_count:   num_samples.max(sonar_bytes.len()),
                    sonar_offset:   sonar_start,
                    sonar_size:     sonar_bytes.len(),
                    sample_format:  format!("xtf/u8/sub{sub_channel}"),
                    samples:        sonar_bytes.iter().map(|&b| (b as u16) * 257).collect(),
                };

                *channel_counts.entry(ch_id).or_insert(0) += 1;
                pings.push(ping);
                sequence += 1;
            }
        }
        // Skip all other packet types (notes, attitude, custom)

        pos += bytes_in_packet.max(XTF_PACKET_SUBHDR_SIZE);
    }

    let channels = build_channel_list(&channel_counts);
    let record_count = pings.len();

    ParseResult {
        record_count,
        recovered_records:    0,
        dropped_bytes,
        parser_magic:         "XTF/0xFACE".into(),
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

fn find_xtf_magic(bytes: &[u8], from: usize) -> Option<usize> {
    let limit = bytes.len().saturating_sub(2);
    for i in from..=limit {
        if le_u16(&bytes[i..i+2]) == XTF_MAGIC {
            return Some(i);
        }
    }
    None
}

fn timestamp_to_ms(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8, hsec: u8) -> u64 {
    // Simple approximation: days since unix epoch (1970-01-01) × 86_400_000 ms
    // Good enough for track-line display without pulling in a full calendar crate.
    let y = year as i64;
    let m = month as i64;
    let d = day as i64;
    if y < 1970 || y > 2100 || m == 0 || d == 0 {
        return 0;
    }
    // Julian Day Number (Meeus)
    let a = (14 - m) / 12;
    let yr = y + 4800 - a;
    let mo = m + 12 * a - 3;
    let jdn = d + (153 * mo + 2) / 5 + 365 * yr + yr / 4 - yr / 100 + yr / 400 - 32045;
    let epoch_jdn: i64 = 2_440_588; // JDN of 1970-01-01
    let days = jdn - epoch_jdn;
    if days < 0 { return 0; }
    let ms_day = days as u64 * 86_400_000;
    let ms_time = (hour as u64) * 3_600_000
        + (minute as u64) * 60_000
        + (second as u64) * 1_000
        + (hsec as u64) * 10;
    ms_day + ms_time
}

fn build_channel_list(counts: &BTreeMap<u32, usize>) -> Vec<ChannelInfo> {
    counts.keys().map(|&id| {
        let ch_type = xtf_channel_type(id as u8);
        let name = match id {
            0 => "XTF Port",
            1 => "XTF Starboard",
            _ => "XTF Aux",
        };
        ChannelInfo {
            id,
            name:         name.to_string(),
            detected:     true,
            mapped_type:  Some(ch_type.to_string()),
            generation:   Some("xtf".to_string()),
        }
    }).collect()
}

fn err(msg: &str) -> ParseResult {
    ParseResult {
        record_count:          0,
        recovered_records:     0,
        dropped_bytes:         0,
        parser_magic:          "XTF".into(),
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
