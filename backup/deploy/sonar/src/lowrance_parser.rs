use serde::Serialize;
use std::f64::consts::PI;
use std::fs;
use std::path::{Path, PathBuf};

/// Represents a single sonar ping extracted from an SL2/SL3 record.
#[derive(Debug, Clone, Serialize)]
pub struct Ping {
    pub file_offset: usize,
    pub sequence: u32,
    pub timestamp_ms: u64,
    pub latitude: f64,
    pub longitude: f64,
    pub depth_m: f32,
    pub depth_ft: f32,
    pub altitude_m: Option<f32>,
    pub temp_c: Option<f32>,
    pub beam_angle_deg: f32,
    pub heading_deg: Option<f32>,
    pub pitch_deg: Option<f32>,
    pub roll_deg: Option<f32>,
    // Lowrance-specific fields
    pub channel_id: u16,
    pub channel_name: String,
    pub raw_samples: Vec<u8>,
    pub speed_knots: f32,
}

/// Aggregated result of parsing an SL2/SL3 file.
#[derive(Debug, Clone, Serialize)]
pub struct ParseResult {
    pub record_count: usize,
    pub recovered_records: usize,
    pub dropped_bytes: usize,
    pub parser_magic: String,
    pub detected_format: String,
    pub pings: Vec<Ping>,
}

impl ParseResult {
    pub fn new() -> Self {
        Self {
            record_count: 0,
            recovered_records: 0,
            dropped_bytes: 0,
            parser_magic: "LOWRANCE_SL".to_string(),
            detected_format: String::new(),
            pings: Vec::new(),
        }
    }
}

const EARTH_RADIUS: f64 = 6356752.3142;

/// Converts Lowrance mercator coordinates to WGS84 latitude/longitude.
fn mercator_to_wgs84(x: i32, y: i32) -> (f64, f64) {
    let lon = x as f64 / EARTH_RADIUS * (180.0 / PI);
    let lat = (2.0 * (y as f64 / EARTH_RADIUS).exp().atan() - PI / 2.0) * (180.0 / PI);
    (lat, lon)
}

/// Maps channel ID to human-readable name.
fn channel_name(ch: u16) -> &'static str {
    match ch {
        0 => "primary",
        1 => "secondary",
        2 => "downscan",
        3 => "sidescan_port",
        4 => "sidescan_star",
        _ => "unknown",
    }
}

pub struct LowranceParser;

impl LowranceParser {
    pub fn new() -> Self { Self }

    /// Parses a Lowrance .sl2 or .sl3 binary file and returns structured ping data.
    pub fn parse_file(&self, path: &Path) -> ParseResult {
        let data = match fs::read(path) {
            Ok(d) => d,
            Err(_) => return ParseResult::new(),
        };

        if data.len() < 10 {
            return ParseResult::new();
        }

        // File Header (10 bytes)
        let fmt_ver = u16::from_le_bytes([data[0], data[1]]);
        
        // Block size at bytes 4-7. Prompt specifies u16 LE but occupies 4 bytes.
        // We read as u32 to safely cover the stride value (typically ~3200).
        let block_size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        if block_size == 0 {
            return ParseResult::new();
        }

        let mut result = ParseResult::new();
        result.detected_format = if fmt_ver == 1 { "SL2".to_string() } else { "SL3".to_string() };

        let mut offset = 10;
        while offset + 76 <= data.len() {
            // Offset 0-3: record offset from file start
            let rec_offset_from_start = u32::from_le_bytes([
                data[offset], data[offset+1], data[offset+2], data[offset+3]
            ]) as usize;

            // Offset 28-31: channel (u16 LE) -> actually bytes 28-29
            let channel = u16::from_le_bytes([data[offset + 28], data[offset + 29]]);
            
            // Offset 32-35: packet size (u16 LE) -> reading bytes 32-33 as u16 per spec
            let packet_size = u16::from_le_bytes([data[offset + 32], data[offset + 33]]) as usize;
            
            // Offset 48-51: depth (f32 LE, feet)
            let depth_ft = f32::from_le_bytes([
                data[offset + 48], data[offset + 49], data[offset + 50], data[offset + 51]
            ]);
            
            // Offset 56-59: speed (f32 LE, knots)
            let speed_knots = f32::from_le_bytes([
                data[offset + 56], data[offset + 57], data[offset + 58], data[offset + 59]
            ]);
            
            // Offset 60-63: temperature (f32 LE, Celsius)
            let temp_c = f32::from_le_bytes([
                data[offset + 60], data[offset + 61], data[offset + 62], data[offset + 63]
            ]);
            
            // Offset 64-67: longitude (i32 LE, mercator X)
            let lon_x = i32::from_le_bytes([
                data[offset + 64], data[offset + 65], data[offset + 66], data[offset + 67]
            ]);
            
            // Offset 68-71: latitude (i32 LE, mercator Y)
            let lat_y = i32::from_le_bytes([
                data[offset + 68], data[offset + 69], data[offset + 70], data[offset + 71]
            ]);

            let (lat, lon) = mercator_to_wgs84(lon_x, lat_y);

            // Raw sonar samples start at offset 140
            let raw_samples_start = offset + 140;
            let samples_len = packet_size.saturating_sub(140);
            let end = raw_samples_start + samples_len;
            
            let samples = if end <= data.len() {
                data[raw_samples_start..end].to_vec()
            } else {
                result.dropped_bytes += 1;
                Vec::new()
            };

            result.record_count += 1;
            result.recovered_records += 1;

            result.pings.push(Ping {
                file_offset: rec_offset_from_start,
                sequence: rec_offset_from_start as u32,
                timestamp_ms: 0, // Not present in SL record header
                latitude: lat,
                longitude: lon,
                depth_m: depth_ft * 0.3048,
                depth_ft,
                altitude_m: None,
                temp_c: Some(temp_c),
                beam_angle_deg: 0.0,
                heading_deg: None,
                pitch_deg: None,
                roll_deg: None,
                channel_id: channel,
                channel_name: channel_name(channel).to_string(),
                raw_samples: samples,
                speed_knots,
            });

            // Advance to next record. Use block size as stride.
            offset += std::cmp::max(block_size, packet_size + 140);
        }

        result
    }
}
