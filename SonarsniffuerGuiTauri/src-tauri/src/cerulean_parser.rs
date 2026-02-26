/// Blue Robotics Cerulean sonar parser — Ping Protocol (.svlog / raw serial)
///
/// Protocol overview
/// -----------------
/// All messages share a common 8-byte header + variable payload + 2-byte checksum.
///
///   Byte 0-1  u8   start1='B' (0x42), start2='R' (0x52)
///   Byte 2-3  u16  payload_length   (LE)
///   Byte 4-5  u16  message_id       (LE)
///   Byte 6    u8   src_device_id
///   Byte 7    u8   dst_device_id
///   Byte 8..n u8[] payload          (payload_length bytes)
///   Byte n+1..n+2  u16  checksum    = sum of all bytes 0..n (LE wrapping)
///
/// Message IDs of interest (Ping1D / Ping360)
/// -------------------------------------------
///   1300  distance        – Ping1D: range ping with depth + confidence
///   1212  profile         – Ping1D: single-beam with sample data
///   2300  device_data     – Ping360: sector scan data
///   5      protocol_version
///   1201  distance_simple – compact Ping1D distance
///
/// Ping1D profile (msg 1212) payload
///   Offset 0-3   u32  distance_mm        (mm to strongest return)
///   Offset 4-7   u32  confidence         (0-1000)
///   Offset 8-11  u32  transmit_duration  (µs)
///   Offset 12-15 u32  ping_number
///   Offset 16-19 u32  scan_start_mm      (start of scan window)
///   Offset 20-23 u32  scan_length_mm     (length of scan window)
///   Offset 24-27 u32  gain_setting       (0 = auto)
///   Offset 28-31 u32  profile_data_length  (N samples)
///   Offset 32..  u8[] profile_data         (N bytes, 0-255 intensity)
///
/// Ping360 device_data (msg 2300) payload
///   Offset 0-1   u16  mode               (1 = normal)
///   Offset 2-3   u16  gain_setting
///   Offset 4-5   u16  angle              (gradians × 25, 0-399)
///   Offset 6-7   u16  transmit_duration  (µs)
///   Offset 8-9   u16  sample_period       (25 ns units)
///   Offset 10-11 u16  frequency          (Hz, 0 = auto, 750000 = 750 kHz)
///   Offset 12-13 u16  number_of_samples
///   Offset 14-15 u16  data_length        (bytes, same as number_of_samples for u8)
///   Offset 16..  u8[] data               (data_length bytes)
///
/// .svlog files:
/// Blue Robotics Ping Viewer stores recordings as raw serial byte dumps
/// (the protocol bytes as-would-be sent over USB/UART).  This parser scans
/// for 'B','R' start bytes and decodes each message.
///
/// GPS / position:
/// The Ping Protocol does not include GPS in the sonar message itself.
/// Many .svlog recordings interleave a "position" message (msg 6) that carries
/// lat/lon as doubles; or there is a companion NMEA log.  When no GPS is found,
/// we output (0.0, 0.0) so the track line still renders (with no geographic context).

use crate::garmin_rsd_parser::{ChannelInfo, ParseResult, Ping};
use std::collections::BTreeMap;
use std::path::Path;

const BR_START1: u8 = 0x42; // 'B'
const BR_START2: u8 = 0x52; // 'R'
const MIN_MSG_SIZE: usize = 10; // 8-byte header + 2-byte checksum (zero-payload)

// Message IDs
const MSG_PING1D_PROFILE:   u16 = 1212;
const MSG_PING1D_DISTANCE:  u16 = 1300;
const MSG_PING360_DATA:     u16 = 2300;
const MSG_POSITION:         u16 = 6;       // extended general_request response / position

// Channel IDs we assign
const CHAN_PING1D:  u32 = 0;  // single-beam depth sounder
const CHAN_PING360: u32 = 1;  // scanning sonar

// ── helpers ───────────────────────────────────────────────────────────────────

#[inline] fn le_u16(b: &[u8]) -> u16 { u16::from_le_bytes([b[0], b[1]]) }
#[inline] fn le_u32(b: &[u8]) -> u32 { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) }
#[inline] fn le_f64(b: &[u8]) -> f64 {
    f64::from_le_bytes([b[0],b[1],b[2],b[3],b[4],b[5],b[6],b[7]])
}

fn checksum_ok(bytes: &[u8]) -> bool {
    // checksum = sum of all bytes except the final 2
    if bytes.len() < 2 { return false; }
    let payload_end = bytes.len() - 2;
    let sum: u16 = bytes[..payload_end].iter().map(|&b| b as u16).fold(0u16, u16::wrapping_add);
    let stored = le_u16(&bytes[payload_end..]);
    sum == stored
}

// ── message parsing ───────────────────────────────────────────────────────────

struct Msg<'a> {
    msg_id:  u16,
    #[allow(dead_code)]
    src:     u8,
    #[allow(dead_code)]
    dst:     u8,
    payload: &'a [u8],
}

/// Try to read one complete message at `pos`.  Returns (Msg, next_pos) or None.
fn try_read_msg(bytes: &[u8], pos: usize) -> Option<(Msg<'_>, usize)> {
    if pos + MIN_MSG_SIZE > bytes.len() { return None; }
    if bytes[pos] != BR_START1 || bytes[pos + 1] != BR_START2 { return None; }

    let payload_len = le_u16(&bytes[pos + 2..pos + 4]) as usize;
    let msg_id      = le_u16(&bytes[pos + 4..pos + 6]);
    let src         = bytes[pos + 6];
    let dst         = bytes[pos + 7];

    let total = 8 + payload_len + 2;
    if pos + total > bytes.len() { return None; }

    let msg_bytes = &bytes[pos..pos + total];
    if !checksum_ok(msg_bytes) { return None; }

    let payload = &bytes[pos + 8..pos + 8 + payload_len];
    Some((Msg { msg_id, src, dst, payload }, pos + total))
}

/// Scan forward from `from` to find the next 'B','R' start pair.
fn next_br(bytes: &[u8], from: usize) -> Option<usize> {
    let limit = bytes.len().saturating_sub(1);
    for i in from..=limit {
        if bytes[i] == BR_START1 && bytes[i + 1] == BR_START2 {
            return Some(i);
        }
    }
    None
}

// ── public entry point ────────────────────────────────────────────────────────

pub fn parse_file(path: &Path) -> ParseResult {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return err(&format!("Cannot read Cerulean log: {e}")),
    };

    if bytes.len() < MIN_MSG_SIZE {
        return err("File too small to contain any Blue Robotics Ping Protocol messages");
    }

    let mut pings:          Vec<Ping>              = Vec::new();
    let mut channel_counts: BTreeMap<u32, usize>   = BTreeMap::new();
    let mut dropped_bytes:  usize                  = 0;
    let mut sequence:       u32                    = 0;

    // Current GPS position (updated by MSG_POSITION messages or NMEA companion)
    let mut cur_lat: f64 = 0.0;
    let mut cur_lon: f64 = 0.0;

    let mut pos = 0usize;

    // Find first message start
    pos = match next_br(&bytes, 0) {
        Some(p) => p,
        None => return err("No Blue Robotics Ping Protocol messages found (no 'BR' start byte pairs)"),
    };

    while pos < bytes.len() {
        match try_read_msg(&bytes, pos) {
            Some((msg, next_pos)) => {
                match msg.msg_id {
                    MSG_POSITION => {
                        // Extended position message: lat (f64) + lon (f64) = 16 bytes minimum
                        if msg.payload.len() >= 16 {
                            cur_lat = le_f64(&msg.payload[0..8]);
                            cur_lon = le_f64(&msg.payload[8..16]);
                        }
                    }

                    MSG_PING1D_PROFILE => {
                        if msg.payload.len() >= 32 {
                            let distance_mm   = le_u32(&msg.payload[0..4]);
                            let _confidence   = le_u32(&msg.payload[4..8]);
                            let ping_number   = le_u32(&msg.payload[12..16]);
                            let _scan_start   = le_u32(&msg.payload[16..20]);
                            let scan_length   = le_u32(&msg.payload[20..24]);
                            let n_samples     = le_u32(&msg.payload[28..32]) as usize;
                            let sonar_slice   = if msg.payload.len() >= 32 + n_samples {
                                &msg.payload[32..32 + n_samples]
                            } else {
                                &msg.payload[32..]
                            };

                            let depth_m  = distance_mm as f32 / 1000.0;
                            let depth_ft = depth_m * 3.28084;

                            let ping = Ping {
                                file_offset:    pos,
                                sequence:       ping_number,
                                timestamp_ms:   sequence as u64 * 40,
                                latitude:       cur_lat,
                                longitude:      cur_lon,
                                depth_m,
                                depth_ft,
                                altitude_m:     0.0,
                                temp_c:         None,
                                beam_angle_deg: 0.0,
                                channel:        CHAN_PING1D,
                                sample_count:   sonar_slice.len(),
                                sonar_offset:   pos + 8 + 32,
                                sonar_size:     sonar_slice.len(),
                                sample_format:  format!("ping1d/u8/{:.0}mm_range", scan_length),
                                samples:        sonar_slice.iter().map(|&b| (b as u16) * 257).collect(),
                            };
                            *channel_counts.entry(CHAN_PING1D).or_insert(0) += 1;
                            pings.push(ping);
                            sequence += 1;
                        }
                    }

                    MSG_PING1D_DISTANCE => {
                        // Compact distance-only message — emit a minimal ping with no samples
                        if msg.payload.len() >= 4 {
                            let distance_mm = le_u32(&msg.payload[0..4]);
                            let depth_m  = distance_mm as f32 / 1000.0;
                            let ping = Ping {
                                file_offset:    pos,
                                sequence,
                                timestamp_ms:   sequence as u64 * 40,
                                latitude:       cur_lat,
                                longitude:      cur_lon,
                                depth_m,
                                depth_ft:       depth_m * 3.28084,
                                altitude_m:     0.0,
                                temp_c:         None,
                                beam_angle_deg: 0.0,
                                channel:        CHAN_PING1D,
                                sample_count:   0,
                                sonar_offset:   pos,
                                sonar_size:     0,
                                sample_format:  "ping1d/distance_only".to_string(),
                                samples:        Vec::new(),
                            };
                            *channel_counts.entry(CHAN_PING1D).or_insert(0) += 1;
                            pings.push(ping);
                            sequence += 1;
                        }
                    }

                    MSG_PING360_DATA => {
                        if msg.payload.len() >= 16 {
                            let angle_grads = le_u16(&msg.payload[4..6]);
                            // Convert gradians (0-399) to degrees (0-360)
                            let bearing_deg = angle_grads as f32 * 360.0 / 400.0;
                            let n_samples   = le_u16(&msg.payload[12..14]) as usize;
                            let data_len    = le_u16(&msg.payload[14..16]) as usize;
                            let sonar_slice = if msg.payload.len() >= 16 + data_len {
                                &msg.payload[16..16 + data_len]
                            } else {
                                &msg.payload[16..]
                            };

                            let ping = Ping {
                                file_offset:    pos,
                                sequence,
                                timestamp_ms:   sequence as u64 * 40,
                                latitude:       cur_lat,
                                longitude:      cur_lon,
                                depth_m:        0.0,
                                depth_ft:       0.0,
                                altitude_m:     0.0,
                                temp_c:         None,
                                beam_angle_deg: bearing_deg,
                                channel:        CHAN_PING360,
                                sample_count:   n_samples,
                                sonar_offset:   pos + 8 + 16,
                                sonar_size:     sonar_slice.len(),
                                sample_format:  "ping360/u8".to_string(),
                                samples:        sonar_slice.iter().map(|&b| (b as u16) * 257).collect(),
                            };
                            *channel_counts.entry(CHAN_PING360).or_insert(0) += 1;
                            pings.push(ping);
                            sequence += 1;
                        }
                    }

                    _ => {} // ignore protocol_version, general_request, etc.
                }

                pos = next_pos;
            }
            None => {
                // Bad or incomplete message at `pos` — skip to next BR sync
                match next_br(&bytes, pos + 2) {
                    Some(next) => {
                        dropped_bytes += next - pos;
                        pos = next;
                    }
                    None => break,
                }
            }
        }
    }

    if pings.is_empty() {
        return err("No Ping1D/Ping360 messages decoded from file");
    }

    let channels = build_channel_list(&channel_counts);
    let record_count = pings.len();

    ParseResult {
        record_count,
        recovered_records:    0,
        dropped_bytes,
        parser_magic:         "CeruleanPingProtocol/BR".into(),
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

fn build_channel_list(counts: &BTreeMap<u32, usize>) -> Vec<ChannelInfo> {
    counts.keys().map(|&id| {
        let (name, mtype) = match id {
            CHAN_PING1D  => ("Ping1D (single-beam echosounder)", Some("primary")),
            CHAN_PING360 => ("Ping360 (scanning sonar)",          Some("chirp_downscan")),
            _            => ("Unknown Cerulean channel",          None),
        };
        ChannelInfo {
            id,
            name:         name.to_string(),
            detected:     true,
            mapped_type:  mtype.map(str::to_string),
            generation:   Some("cerulean".to_string()),
        }
    }).collect()
}

fn err(msg: &str) -> ParseResult {
    ParseResult {
        record_count:          0,
        recovered_records:     0,
        dropped_bytes:         0,
        parser_magic:          "Cerulean".into(),
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
