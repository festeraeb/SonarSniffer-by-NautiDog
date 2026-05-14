use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use crate::healing_api;

const MAGIC_REC_HDR: u32 = 0xB7E9DA86;
const MAGIC_REC_TRL: u32 = 0xD9264B7C;
const MAX_BACKTRACK_HEADER_START: usize = 64;
const MAX_SEARCH_WINDOW: usize = 16 * 1024 * 1024;

/// Sample encoding hint derived from channel generation.
/// Classic (ch 0–3): 8-bit u8 samples (Garmin non-UHD hardware).
/// UHD / UHD2 (ch 4+): 16-bit signed i16 samples (UHD CHIRP hardware).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SampleHint {
    U8,
    I16,
    Unknown, // fall back to heuristic
}

/// Detected hardware generation for a Garmin RSD file.
/// Controls how body varstruct fields are interpreted — critically, body field 7
/// is XID metadata in Gen1 but sample_count in Gen2+.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(clippy::upper_case_acronyms)]
pub enum RsdGeneration {
    /// Pre-UHD Echomap (channels 0–3, 8-bit u8, body field 7 = XID not sample_count)
    Gen1Classic,
    /// UHD series (channels 4–7, 16-bit i16, body field 7 = sample_count)
    UHD,
    /// UHD2 and later (channels 8+, 16-bit i16, body field 7 = sample_count)
    UHD2,
    /// Indeterminate — per-record heuristics applied
    Unknown,
}

impl std::fmt::Display for RsdGeneration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gen1Classic => write!(f, "gen1"),
            Self::UHD => write!(f, "uhd"),
            Self::UHD2 => write!(f, "uhd2"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelInfo {
    pub id: u32,
    pub name: String,
    pub detected: bool,
    /// Beam type: "port_sidescan" | "starboard_sidescan" | "chirp_downscan" | "depth_temp"
    pub mapped_type: Option<String>,
    /// Hardware generation: "classic" | "uhd" | "uhd2"
    pub generation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Ping {
    pub file_offset: usize,
    pub sequence: u32,
    pub timestamp_ms: u64,
    pub latitude: f64,
    pub longitude: f64,
    pub depth_m: f32,
    /// Depth converted to US customary feet (depth_m × 3.28084).
    pub depth_ft: f32,
    pub altitude_m: f32,
    pub temp_c: Option<f32>,
    pub beam_angle_deg: f32,
    pub heading_deg: Option<f32>,
    pub pitch_deg: Option<f32>,
    pub roll_deg: Option<f32>,
    pub channel: u32,
    pub sample_count: usize,
    pub sonar_offset: usize,
    pub sonar_size: usize,
    pub sample_format: String,
    pub samples: Vec<u16>,
    /// Raw hardware gain / TVG byte from field 7 (Gen1Classic only, 0–255).
    /// For UHD/UHD2, field 7 encodes sample_count — this is `None`.
    pub hardware_gain: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseResult {
    pub record_count: usize,
    pub recovered_records: usize,
    pub dropped_bytes: usize,
    pub parser_magic: String,
    /// Detected hardware generation (Gen1Classic, UHD, UHD2, Unknown)
    pub detected_generation: Option<RsdGeneration>,
    /// Firmware version from header field 1 (raw u32), if available
    pub firmware_version: Option<u32>,
    pub channels: Vec<ChannelInfo>,
    pub channel_counts: BTreeMap<u32, usize>,
    /// Per-field histogram: field_id → value → ping_count.
    /// Body fields use IDs 0–99 (as-is); header varstruct fields are offset by 100
    /// so header field 3 appears as key 103. This allows spotting frequency /
    /// offset constants that live in the record header rather than the body.
    pub field_channel_counts: BTreeMap<u32, BTreeMap<u32, usize>>,
    /// Unique decoded u32 values seen per field_id across all pings (same
    /// 0–99 body / 100+ header offset scheme). A constant entry (single value)
    /// is a strong signal of a per-file config field (e.g. frequency_hz).
    pub unique_field_values: BTreeMap<u32, Vec<u32>>,
    pub unknown_channels: Vec<String>,
    pub healing_actions: Vec<String>,
    pub error_message: Option<String>,
    pub pings: Vec<Ping>,
    pub crc_mismatch_count: usize,
}

impl ParseResult {
    pub fn normalize_nadir_direction(&mut self) {
        if self.pings.is_empty() {
            return;
        }

        let mut reverse_channels = std::collections::BTreeSet::new();
        let channels: std::collections::BTreeSet<u32> =
            self.pings.iter().map(|p| p.channel).collect();

        for ch in channels {
            let mut starts_with_nadir = 0;
            let mut ends_with_nadir = 0;

            let ch_pings: Vec<&Ping> = self
                .pings
                .iter()
                .filter(|p| p.channel == ch)
                .take(100)
                .collect();
            if ch_pings.is_empty() {
                continue;
            }

            for p in ch_pings {
                let n = p.samples.len();
                if n < 32 {
                    continue;
                }

                let mut sorted: Vec<u16> = p.samples.clone();
                sorted.sort_unstable();
                let p15_idx = (n * 15 / 100).min(n - 1);
                let p90_idx = (n * 90 / 100).min(n - 1);
                let p15 = sorted[p15_idx] as f32;
                let p90 = sorted[p90_idx] as f32;
                let span = (p90 - p15).max(1.0);
                let threshold = (p15 + span * 0.20) as u16;

                let mut run_left = 0usize;
                let mut left_nadir_size = 0usize;
                for i in 0..n {
                    if p.samples[i] > threshold {
                        run_left += 1;
                        if run_left >= 5 {
                            left_nadir_size = i + 1 - 5;
                            break;
                        }
                    } else {
                        run_left = 0;
                    }
                }

                let mut run_right = 0usize;
                let mut right_nadir_size = 0usize;
                for i in 0..n {
                    if p.samples[n - 1 - i] > threshold {
                        run_right += 1;
                        if run_right >= 5 {
                            right_nadir_size = i + 1 - 5;
                            break;
                        }
                    } else {
                        run_right = 0;
                    }
                }

                if right_nadir_size > left_nadir_size * 2 && right_nadir_size > 10 {
                    ends_with_nadir += 1;
                } else if left_nadir_size > right_nadir_size * 2 && left_nadir_size > 10 {
                    starts_with_nadir += 1;
                }
            }

            if ends_with_nadir > starts_with_nadir && ends_with_nadir > 5 {
                reverse_channels.insert(ch);
            }
        }

        for ch in &reverse_channels {
            self.healing_actions.push(format!(
                "Reversed samples for channel {} (native nadir at right edge)",
                ch
            ));
        }

        for p in &mut self.pings {
            if reverse_channels.contains(&p.channel) {
                p.samples.reverse();
            }
        }
    }

    pub fn empty_with_error(message: impl Into<String>) -> Self {
        Self {
            record_count: 0,
            recovered_records: 0,
            dropped_bytes: 0,
            parser_magic: format!("0x{MAGIC_REC_HDR:08X}"),
            detected_generation: None,
            firmware_version: None,
            channels: Vec::new(),
            channel_counts: BTreeMap::new(),
            field_channel_counts: BTreeMap::new(),
            unique_field_values: BTreeMap::new(),
            unknown_channels: Vec::new(),
            healing_actions: Vec::new(),
            error_message: Some(message.into()),
            pings: Vec::new(),
            crc_mismatch_count: 0,
        }
    }

    /// Return an error result for a file whose format could not be detected.
    #[allow(dead_code)]
    pub fn unknown_format(path: &std::path::Path) -> Self {
        Self::empty_with_error(format!(
            "Unrecognised file format: {}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("(unknown)")
        ))
    }
}

pub struct GarminRSDParser;

impl GarminRSDParser {
    pub fn new() -> Self {
        Self
    }

    pub fn parse_file(&mut self, path: &Path) -> ParseResult {
        let bytes = match fs::read(path) {
            Ok(content) => content,
            Err(err) => {
                return ParseResult::empty_with_error(format!("Failed to read file: {err}"));
            }
        };

        if bytes.len() < 16 {
            return ParseResult::empty_with_error(
                "Input appears too small to contain sonar records.",
            );
        }

        let magic_candidates = load_magic_candidates(path);
        let mut cursor = 0usize;
        let mut pings = Vec::new();
        let mut recovered_records = 0usize;
        let mut dropped_bytes = 0usize;
        let mut healing_actions = Vec::new();
        let mut channel_counts: BTreeMap<u32, usize> = BTreeMap::new();
        let mut field_channel_counts: BTreeMap<u32, BTreeMap<u32, usize>> = BTreeMap::new();
        let mut unique_field_values: BTreeMap<u32, std::collections::BTreeSet<u32>> =
            BTreeMap::new();
        let mut total_crc_mismatches: usize = 0;

        let first_sync = find_next_magic(&bytes, &magic_candidates, 0, bytes.len());
        let Some(mut scan_pos) = first_sync else {
            return ParseResult::empty_with_error("No Garmin record header magic detected.");
        };

        // ── generation detection (self-healing entry point) ─────────────────
        let (generation, gen_signals) = detect_generation(&bytes, scan_pos, &magic_candidates);
        healing_actions.push(format!(
            "Generation detected: {} [{}]",
            generation,
            gen_signals.join("; ")
        ));

        // Extract firmware version from header field 1 of the first record
        let firmware_version = probe_firmware_version(&bytes, scan_pos, &magic_candidates);
        if let Some(fw) = firmware_version {
            healing_actions.push(format!("Firmware version (header f1): {fw}"));
        }

        // Extract preamble channels once for channel recovery
        let preamble_channels = scan_preamble_channel_ids(&bytes, scan_pos);

        while scan_pos + 12 <= bytes.len() {
            if scan_pos > cursor {
                let skipped = scan_pos - cursor;
                if skipped > 0 {
                    dropped_bytes += skipped;
                    recovered_records += 1;
                    healing_actions.push(format!(
                        "Skipped {skipped} byte(s) while resynchronizing to header at {scan_pos}."
                    ));
                }
            }

            match self.try_parse_record(
                &bytes,
                scan_pos,
                &magic_candidates,
                generation,
                &preamble_channels,
            ) {
                Some((ping, field_values, next_scan_pos, crc_mismatches)) => {
                    total_crc_mismatches += crc_mismatches;
                    *channel_counts.entry(ping.channel).or_insert(0) += 1;

                    // Aggregate observed field->channel values (only u32 decodes) for debugging/mapping.
                    for (field_id, val) in &field_values {
                        let entry = field_channel_counts.entry(*field_id).or_default();
                        *entry.entry(*val).or_insert(0) += 1;
                        unique_field_values
                            .entry(*field_id)
                            .or_default()
                            .insert(*val);
                    }

                    pings.push(ping);
                    cursor = next_scan_pos.max(scan_pos + 4);
                    scan_pos = match find_next_magic(
                        &bytes,
                        &magic_candidates,
                        cursor.saturating_sub(1),
                        (cursor + MAX_SEARCH_WINDOW).min(bytes.len()),
                    ) {
                        Some(next) => next,
                        None => break,
                    };
                }
                None => {
                    if let Some(next_pos) = find_next_magic(
                        &bytes,
                        &magic_candidates,
                        scan_pos + 4,
                        (scan_pos + MAX_SEARCH_WINDOW).min(bytes.len()),
                    ) {
                        let skipped = next_pos.saturating_sub(scan_pos);
                        dropped_bytes += skipped;
                        recovered_records += 1;
                        healing_actions.push(format!(
                            "Failed parsing at {scan_pos}; jumped to {next_pos}."
                        ));
                        cursor = next_pos;
                        scan_pos = next_pos;
                    } else {
                        let tail = bytes.len().saturating_sub(scan_pos);
                        dropped_bytes += tail;
                        healing_actions.push(format!(
                            "No additional record sync found; dropped trailing {tail} byte(s)."
                        ));
                        break;
                    }
                }
            }
        }

        // Filename-based heuristics (C-prefix, one-sided remap) are intentionally
        // removed.  Channel role assignment is now fully data-driven at render time
        // via `channel_sonar_geometry()` in outputs.rs, which measures the actual
        // nadir offset from sample data to distinguish sidescan from
        // downscan/ClearVü without relying on file naming conventions.
        let channels = self.detect_channels(&channel_counts, false);
        let unknown_channels = channels
            .iter()
            .filter(|channel| channel.mapped_type.is_none())
            .map(|channel| format!("id {}", channel.id))
            .collect::<Vec<_>>();

        // Convert BTreeSet<u32> → Vec<u32> (already sorted by BTreeSet).
        let unique_field_values: BTreeMap<u32, Vec<u32>> = unique_field_values
            .into_iter()
            .map(|(k, s)| (k, s.into_iter().collect()))
            .collect();

        // ── post-parse self-healing: validate sample consistency ────────
        let mut sample_mismatches = 0usize;
        for p in &pings {
            let expected_bps: usize = if p.sample_format.contains("i16") {
                2
            } else {
                1
            };
            if p.sample_count > 0 {
                let expected_sonar = p.sample_count * expected_bps;
                // Allow ±8 bytes tolerance for varstruct padding/CRC
                if p.sonar_size > 0
                    && (p.sonar_size as isize - expected_sonar as isize).unsigned_abs() > 8
                {
                    sample_mismatches += 1;
                }
            }
        }
        let total = pings.len().max(1);
        let mismatch_rate = sample_mismatches as f64 / total as f64;
        if sample_mismatches > 0 {
            healing_actions.push(format!(
                "Sample consistency: {sample_mismatches}/{total} records ({:.1}%) have sonar_size/sample_count mismatch",
                mismatch_rate * 100.0
            ));
        }

        // Record a healing discovery if generation heuristics were used
        if generation == RsdGeneration::Unknown || mismatch_rate > 0.05 {
            let fingerprint = healing_api::compute_file_fingerprint(&bytes);
            let magic_val = le_u32(&bytes[scan_pos..scan_pos.saturating_add(4)]).unwrap_or(0);
            let preamble = scan_preamble_channel_ids(&bytes, scan_pos);
            let mut discovery = healing_api::HealingDiscovery {
                id: String::new(),
                discovered_at: chrono::Local::now().to_rfc3339(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                magic: magic_val,
                firmware_version,
                generation: generation.to_string(),
                channel_ids: preamble,
                correction_type: if generation == RsdGeneration::Unknown {
                    "generation_heuristic".into()
                } else {
                    "sample_mismatch_detected".into()
                },
                original_interpretation: format!("gen={}, field7=sample_count", generation),
                corrected_interpretation: format!(
                    "gen={}, mismatch_rate={:.3}, mismatches={}/{}",
                    generation, mismatch_rate, sample_mismatches, total
                ),
                records_parsed: pings.len(),
                confidence: 1.0 - mismatch_rate as f32,
                file_fingerprint: fingerprint,
            };
            discovery.id = healing_api::compute_discovery_id(&discovery);
            // Best-effort save to local cache (ignore errors)
            let _ = healing_api::record_discovery(discovery, None);
        }

        let mut result = ParseResult {
            record_count: pings.len(),
            recovered_records,
            dropped_bytes,
            parser_magic: format!("0x{MAGIC_REC_HDR:08X}"),
            detected_generation: Some(generation),
            firmware_version,
            channels,
            channel_counts,
            field_channel_counts,
            unique_field_values,
            unknown_channels,
            healing_actions,
            error_message: None,
            pings,
            crc_mismatch_count: total_crc_mismatches,
        };
        result.normalize_nadir_direction();
        result
    }

    fn try_parse_record(
        &self,
        bytes: &[u8],
        pos_magic: usize,
        magic_candidates: &[u32],
        generation: RsdGeneration,
        preamble_channels: &[u32],
    ) -> Option<(Ping, HashMap<u32, u32>, usize, usize)> {
        #[allow(clippy::type_complexity)]
        let mut header: Option<(HashMap<u32, Vec<u8>>, usize, usize, bool)> = None;

        for back in 1..=MAX_BACKTRACK_HEADER_START {
            if pos_magic < back {
                break;
            }
            let start = pos_magic - back;
            let parsed = match parse_varstruct(bytes, start, bytes.len(), CrcMode::Warn) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let Some(raw_magic) = parsed.0.get(&0) else {
                continue;
            };
            let Some(hdr_magic) = le_u32(raw_magic) else {
                continue;
            };
            if magic_candidates.contains(&hdr_magic) {
                header = Some((parsed.0, start, parsed.1, parsed.2));
                break;
            }
        }

        let (hdr, hdr_start, body_start, hdr_crc_ok) = header?;
        let sequence = le_u32(hdr.get(&2).unwrap_or(&vec![])).unwrap_or(0);
        let data_size = le_u16(hdr.get(&4).unwrap_or(&vec![])).unwrap_or(0) as usize;

        let (body, body_end, body_crc_ok) =
            parse_varstruct(bytes, body_start, bytes.len(), CrcMode::Warn).ok()?;
        let timestamp_ms = decode_timestamp_ms(&hdr, &body, sequence);
        let used = body_end.saturating_sub(body_start);
        let sonar_offset = body_start + used;
        let sonar_size = data_size.saturating_sub(used);

        if sonar_offset >= bytes.len() {
            return None;
        }
        let sonar_end = sonar_offset.saturating_add(sonar_size).min(bytes.len());
        let sonar = &bytes[sonar_offset..sonar_end];

        // Channel ID is stored as 1–4 bytes in body field 0.
        // Use padded decode to match Python's int.from_bytes(val[:4].ljust(4), 'little').
        // If field 0 is absent (e.g. GT54 test captures), we'll recover from preamble.
        let channel_from_body = le_u32_padded(body.get(&0).map(Vec::as_slice).unwrap_or(&[]));

        // Recovery: if body field 0 is absent, use channel from preamble metadata
        // or infer from beam type enum (field 12) + generation enum (field 6).
        let channel = if let Some(ch) = channel_from_body {
            ch
        } else {
            // Body field 0 absent — recovery needed
            // Strategy 1: Use beam type enum (field 12) to infer channel role
            let beam_type =
                le_u32_padded(body.get(&12).map(Vec::as_slice).unwrap_or(&[])).unwrap_or(0);
            let gen_enum =
                le_u32_padded(body.get(&6).map(Vec::as_slice).unwrap_or(&[])).unwrap_or(0);

            // Beam type mapping:
            // 1 = classic/gen1 starboard → ch1
            // 2 = UHD port (gen2 ch4) → ch4
            // 3 = UHD starboard (gen2 ch5) → ch5
            // 4 = UHD2 beam → ch8+ (use generation to disambiguate)
            let recovered = match (beam_type, gen_enum) {
                (1, _) => 1u32, // classic starboard
                (2, 2) => 4u32, // UHD port
                (3, 2) => 5u32, // UHD starboard
                (2, _) | (3, _) | (4, _) => {
                    // UHD2: use preamble channel list or default to ch8
                    if !preamble_channels.is_empty() {
                        // Pick first channel from preamble that matches beam type pattern
                        // For UHD2, typically ch8/9 (sidescan) or ch10+ (downscan)
                        preamble_channels.first().copied().unwrap_or(8)
                    } else {
                        8 // fallback
                    }
                }
                _ => 0, // unknown
            };
            eprintln!(
                "[parser] Body field 0 absent at seq{} @ 0x{:X} — recovered channel {} from beam_type={} gen_enum={}",
                sequence, hdr_start, recovered, beam_type, gen_enum
            );
            recovered
        };

        // ── self-healing field 7 interpretation ──────────────────────────
        // Gen1 Classic: field 7 is XID/TVG metadata (0-255), NOT sample_count.
        //   → Derive sample_count from sonar_size (u8 → 1:1).
        // Gen2+ (UHD/UHD2): field 7 IS sample_count.
        // Unknown: per-record heuristic using sonar_size / field7 ratio.
        let field7_raw =
            le_u32_padded(body.get(&7).map(Vec::as_slice).unwrap_or(&[])).unwrap_or(0) as usize;
        // For Gen1Classic, field 7 is a hardware gain/TVG byte (0–255), not sample_count.
        // Capture it now before it is reinterpreted below.
        let hardware_gain: Option<u32> = match generation {
            RsdGeneration::Gen1Classic => {
                let g = field7_raw as u32;
                if g <= 255 {
                    Some(g)
                } else {
                    None
                }
            }
            _ => None,
        };
        let sample_count_raw = match generation {
            RsdGeneration::Gen1Classic => {
                // Gen1: 8-bit samples → sample_count = sonar_size
                sonar_size
            }
            RsdGeneration::Unknown => {
                // Heuristic: check if field7 makes sense as sample_count
                if field7_raw > 0 && sonar_size > 0 {
                    let ratio = sonar_size as f64 / field7_raw as f64;
                    if (ratio - 1.0).abs() < 0.15 || (ratio - 2.0).abs() < 0.15 {
                        field7_raw // Plausible sample_count
                    } else if field7_raw < 256 && sonar_size > 256 {
                        // field7 looks like an XID (small), sonar_size is large → Gen1 pattern
                        sonar_size
                    } else {
                        field7_raw // Default to treating as sample_count
                    }
                } else if sonar_size > 0 {
                    sonar_size // No field7 → use sonar_size
                } else {
                    field7_raw
                }
            }
            _ => field7_raw, // Gen2+: field 7 IS sample_count
        };

        // Use generation knowledge to pick the right sample format:
        // classic (0–3) → u8, uhd/uhd2 (4+) → i16.
        let mapped_hint = match map_channel_info(channel) {
            Some((_, "classic")) => SampleHint::U8,
            Some((_, _)) => SampleHint::I16,
            None => SampleHint::Unknown,
        };
        // Older/mixed firmware can emit non-standard channel IDs where static mapping
        // is wrong. Override with per-record sonar_size/sample_count evidence.
        let inferred_hint =
            infer_sample_hint_from_layout(sonar_size, sample_count_raw, sonar.len());
        let sample_hint = match (mapped_hint, inferred_hint) {
            (SampleHint::U8, SampleHint::I16) => SampleHint::I16,
            (SampleHint::I16, SampleHint::U8) => SampleHint::U8,
            (_, h) if h != SampleHint::Unknown => h,
            (h, _) => h,
        };
        let sample_count =
            normalize_sample_count(sample_count_raw, sonar_size, sonar.len(), sample_hint);
        let latitude = body
            .get(&9)
            .and_then(|b| le_i32(b))
            .map(mapunit_to_deg)
            .unwrap_or(0.0);
        let longitude = body
            .get(&10)
            .and_then(|b| le_i32(b))
            .map(mapunit_to_deg)
            .unwrap_or(0.0);
        let mut depth_m = body
            .get(&1)
            .and_then(|b| read_varint_from_slice(b).ok())
            .map(|v| v as f32 / 1000.0)
            .unwrap_or(0.0);
        if !(0.0..=500.0).contains(&depth_m) {
            // Guard against obvious corrupt values from bad resync; clamp to 0 so downstream stats
            // and overlays stay sane. 500 m (~1640 ft) covers even deep lakes/ocean demos.
            depth_m = 0.0;
        }
        let temp_c = body
            .get(&14)
            .and_then(|b| read_varint_from_slice(b).ok())
            .map(|v| (v as f32) / 1000.0)
            .map(|t| t.abs())
            .filter(|t| *t > 0.05 && *t < 60.0);
        // Decode attitude-related fields with conservative heuristics.
        // Preferred IDs are used first, then we scan all remaining fields as fallback.
        let beam_angle_deg =
            pick_angle_from_fields(&body, &[11, 12, 16], -95.0, 95.0).unwrap_or(0.0);
        let heading_deg = pick_heading_from_fields(&body, &[16, 15, 11, 12]);
        let pitch_deg = pick_angle_from_fields(&body, &[17, 18, 13], -90.0, 90.0);
        let roll_deg = pick_angle_from_fields(&body, &[18, 17, 19], -180.0, 180.0);

        let (samples, mut sample_format) = decode_samples(sonar, sample_count, sample_hint);
        let voids = detect_low_energy_voids(&samples);
        if voids.len() >= 2 {
            sample_format = format!("{sample_format}+mux2");
        }

        // ── Validate sonar_size vs decoded sample byte footprint ──────────────────
        // A mismatch > 8 bytes signals a padding/skip alignment error, which manifests
        // downstream as black "ghost" stripes on GT56 UHD2 imagery.
        {
            let expected_bytes = decoded_payload_bytes(&sample_format, samples.len());
            if expected_bytes > 0 && sonar_size > 0 {
                let diff = (sonar_size as isize - expected_bytes as isize).unsigned_abs();
                if diff > 8 {
                    eprintln!(
                        "[parser] Warning: sample mismatch ch{} seq{} @ 0x{:X}: \
                         sonar_size={} bytes, decoded {} samples (fmt={}, expected_payload={} bytes). \
                         delta={} — possible alignment/padding skip error (ghosting risk).",
                        channel, sequence, hdr_start,
                        sonar_size, samples.len(), sample_format, expected_bytes, diff
                    );
                }
            }
        }

        // Collect u32-like field values for histogramming (use padded decode like Python).
        // Body fields 0–15 are stored as-is; header fields are stored at offset +100
        // (e.g. header field 3 → key 103) so both can be tracked in the same map.
        let mut field_values: HashMap<u32, u32> = HashMap::new();
        for (fid, raw) in body.iter() {
            if *fid <= 99 {
                if let Some(v) = le_u32_padded(raw) {
                    field_values.insert(*fid, v);
                }
            }
        }
        // Header varstruct fields (offset +100): skip magic(0), sequence(2),
        // data_size(4), timestamp(5) — those already have typed decoding above.
        for (fid, raw) in hdr.iter() {
            let hid = *fid;
            if hid != 0 && hid != 2 && hid != 4 && hid != 5 && hid <= 99 {
                if let Some(v) = le_u32_padded(raw) {
                    field_values.insert(hid + 100, v);
                }
            }
        }

        let mut next_pos = sonar_end;
        let trailer_pos = body_start.saturating_add(data_size);
        if trailer_pos + 12 <= bytes.len() {
            // Trailer fields are little-endian (Python: struct.unpack('<III', ...))
            if let (Some(tr_magic), Some(chunk_size)) = (
                le_u32(&bytes[trailer_pos..trailer_pos + 4]),
                le_u32(&bytes[trailer_pos + 4..trailer_pos + 8]),
            ) {
                if tr_magic == MAGIC_REC_TRL && chunk_size > 0 {
                    next_pos = hdr_start
                        .saturating_add(chunk_size as usize)
                        .min(bytes.len());
                }
            }
        }

        // Optional debug dump for new file formats: set SNF_DEBUG_FIELDS=1 to log first few records.
        #[cfg(debug_assertions)]
        if std::env::var("SNF_DEBUG_FIELDS").is_ok() && sequence <= 5 {
            fn hex_bytes(v: &[u8]) -> String {
                v.iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            let decode_field = |fid: u32,
                                src: &std::collections::HashMap<u32, Vec<u8>>|
             -> (String, Option<u32>, Option<i32>, Option<f32>) {
                if let Some(b) = src.get(&fid) {
                    let le_u32v = le_u32_padded(b);
                    let le_i32v = le_i32(b);
                    let var_v = read_varint_from_slice(b).ok().map(|v| v as f32 / 1000.0);
                    (hex_bytes(b), le_u32v, le_i32v, var_v)
                } else {
                    ("<none>".into(), None, None, None)
                }
            };

            // Dump a compact table of all body fields for quick field hunting (temp/roll/pitch/etc.).
            let mut all_fields: Vec<u32> = body.keys().copied().collect();
            all_fields.sort();
            let mut field_lines = Vec::new();
            for fid in all_fields {
                let (hex, u32v, i32v, varv) = decode_field(fid, &body);
                field_lines.push(format!(
                    "    f{fid:02} hex={hex} le_u32={u32v:?} le_i32={i32v:?} var/1000={varv:?}"
                ));
            }

            eprintln!(
                "DBG seq {sequence} ch {channel} depth_m={depth_m:.3}\n  body_keys={:?}\n{}",
                body.keys().collect::<Vec<_>>(),
                field_lines.join("\n"),
            );
        }

        let crc_mismatches = (!hdr_crc_ok as usize) + (!body_crc_ok as usize);
        Some((
            Ping {
                file_offset: hdr_start,
                sequence,
                timestamp_ms,
                latitude,
                longitude,
                depth_m,
                depth_ft: depth_m * 3.280_84,
                altitude_m: 0.0,
                temp_c,
                beam_angle_deg,
                heading_deg,
                pitch_deg,
                roll_deg,
                channel,
                sample_count,
                sonar_offset,
                sonar_size,
                sample_format,
                samples,
                hardware_gain,
            },
            field_values,
            next_pos,
            crc_mismatches,
        ))
    }

    fn detect_channels(
        &self,
        channel_counts: &BTreeMap<u32, usize>,
        force_c_downscan: bool,
    ) -> Vec<ChannelInfo> {
        channel_counts
            .keys()
            .copied()
            .map(|id| {
                let (mapped_type, generation, name) = match map_channel_info(id) {
                    Some((beam, gen)) => {
                        let beam = if force_c_downscan && beam.contains("sidescan") {
                            "chirp_downscan"
                        } else {
                            beam
                        };
                        let label = format!(
                            "{} {}",
                            match gen {
                                "classic" => "Classic",
                                "uhd" => "UHD",
                                "uhd2" => "UHD2",
                                _ => gen,
                            },
                            match beam {
                                "port_sidescan" => "Port Sidescan",
                                "starboard_sidescan" => "Starboard Sidescan",
                                "port_sidescan_hf" => "Port Sidescan (HF, tentative)",
                                "starboard_sidescan_hf" => "Starboard Sidescan (HF, tentative)",
                                "chirp_downscan" => "Chirp Downscan",
                                "depth_temp" => "Depth/Temp",
                                _ => beam,
                            }
                        );
                        (Some(beam.to_string()), Some(gen.to_string()), label)
                    }
                    None => (None, None, format!("Channel {id} (unknown)")),
                };
                ChannelInfo {
                    id,
                    name,
                    detected: true,
                    mapped_type,
                    generation,
                }
            })
            .collect()
    }
}

// is_one_sided_channel_layout removed — see comment above near force_downscan_for_downscope.

/// Returns `(beam_type, generation)` for a known channel ID, or `None` for unrecognised IDs.
///
/// Three hardware generations observed in ECHOMAP / STRIKER captures:
///
/// | Range | Generation | Typical transducer |
/// |-------|------------|--------------------|
/// | 0–3   | classic    | GT20, GT22, GT51, GT52 (non-UHD, 8-bit samples) |
/// | 4–7   | uhd        | GT54 (standard), GT56 on UHD unit |
/// | 8–11  | uhd2       | GT54 UHD2 config, GT56 on UHD2 unit |
///
/// Empirically confirmed channel IDs (observed in actual .RSD captures):
///   ch0  – inferred classic port  (no sample file yet)
///   ch1  – confirmed classic starboard (GT54 bench file)
///   ch2  – confirmed classic downscan  (GT54 bench file)
///   ch4  – confirmed UHD port      (Holloway, Sonar000, Sonar001)
///   ch5  – confirmed UHD starboard (Holloway, Sonar000, Sonar001, GT54 bench)
///   ch10 – confirmed UHD2 downscan (GT54 UHD2 bench file)
///
/// All others are structurally inferred from the generation ladder.
pub fn map_channel_info(id: u32) -> Option<(&'static str, &'static str)> {
    Some(match id {
        // ── Classic (non-UHD) ──────────────────────────────────────────────
        // Pre-UHD ECHOMAP / STRIKER units; GT20, GT22, GT51, GT52 transducers.
        // Sonar samples are 8-bit u8; typical sample count 256–512.
        0 => ("port_sidescan", "classic"),
        1 => ("starboard_sidescan", "classic"),
        2 => ("chirp_downscan", "classic"),
        // Empirical override: many captures show ch3 as a sidescan arm rather than depth/temp.
        3 => ("port_sidescan", "classic"),

        // ── UHD CHIRP ──────────────────────────────────────────────────────
        // ECHOMAP UHD / STRIKER Vivid UHD series.
        // GT54 in standard (non-UHD2) config, GT56 almost always produces these.
        4 => ("port_sidescan", "uhd"),
        5 => ("starboard_sidescan", "uhd"),
        6 => ("chirp_downscan", "uhd"),
        7 => ("depth_temp", "uhd"),

        // ── UHD2 (ECHOMAP Ultra 2 / 8-series channel layout) ─────────────
        // Observed on 93SV UHD2, 126SV UHD2 with GT54 in single-sidescan config:
        //   ch8/9 = sidescan pair, ch10 = chirp downscan, ch11 = depth/temp.
        8 => ("port_sidescan", "uhd2"),
        9 => ("starboard_sidescan", "uhd2"),
        10 => ("chirp_downscan", "uhd2"),
        11 => ("depth_temp", "uhd2"),

        // ── UHD2+ (10-series channel layout — 25MAR25 / newer firmware) ───
        // Observed on ECHOMAP Ultra 2 with GT56 / newer GT54 firmware where the
        // channel numbering shifts up by 2: ch10/11 = sidescan pair,
        // ch12 = chirp downscan, ch13 = depth/temp.
        // NOTE: ch10 is AMBIGUOUS — on older firmware it is chirp_downscan (above);
        // on newer firmware it is port_sidescan.  `find_sidescan_pair` resolves
        // this at runtime by checking whether ch10 AND ch11 both have large sonar
        // ping counts, in which case they are treated as the sidescan pair.
        // The static map labels ch10 as "chirp_downscan" to keep 8-series compat;
        // the runtime override in find_sidescan_pair handles 10-series devices.
        12 => ("chirp_downscan", "uhd2"),
        13 => ("depth_temp", "uhd2"),

        // ── UHD2+ (14-series channel layout — highest-end UHD2) ───────────
        14 => ("port_sidescan", "uhd2"),
        15 => ("starboard_sidescan", "uhd2"),
        16 => ("chirp_downscan", "uhd2"),
        17 => ("depth_temp", "uhd2"),

        // ── UHD2+ ClearVü dual-frequency channels ─────────────────────────
        // Observed on GT54/GT56 ClearVü transducer with UHD2 sonarhead.
        // ch18 fires at the beam-heading rate (ClearVü HF), ch20 at roll-following rate.
        // Both have ClearVü beam geometry (~26°) and near-zero nadir offset.
        18 => ("chirp_downscan", "uhd2"),
        20 => ("chirp_downscan", "uhd2"),

        // ── Legacy / platform-specific channels ──────────────────────────
        // ch993: seen on 93SV UHD platform, single-arm bench capture, beam ~22.7°.
        // Could be sidescan or ClearVü — nadir geometry discriminates at render time.
        993 => ("chirp_downscan", "uhd"),
        // ch1487: seen on ECHOMAP Ultra with firmware 0x0C018902 (201392386).
        // Single-channel ClearVü capture, beam ~25.6°, valid GPS and depth.
        1487 => ("chirp_downscan", "uhd"),

        _ => return None,
    })
}

/// Quick sanity check run before the full parse.
/// Validates that the file looks like a well-formed Garmin RSD by reading only
/// the very first record header + body varstruct without decoding sample data.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub struct FileProbe {
    /// File size in bytes.
    pub file_size: usize,
    /// Magic bytes found at the first record (`0xB7E9DA86` or variant).
    pub magic_found: Option<String>,
    /// Byte offset where the first magic was located.
    pub magic_offset: Option<usize>,
    /// Whether the first header varstruct CRC passed.
    pub header_crc_ok: bool,
    /// Whether the first body varstruct CRC passed.
    pub body_crc_ok: bool,
    /// Channel ID from body field 0 of the first record (`None` if field absent).
    pub first_channel: Option<u32>,
    /// Decoded channel type label from the static channel table (e.g. `"UHD Port Sidescan"`).
    pub first_channel_label: Option<String>,
    /// All body field IDs present in the first record.
    pub first_record_fields: Vec<u32>,
    /// Estimated total record count based on file size / first record stride.
    pub estimated_records: Option<usize>,
    /// Channel IDs declared in the file preamble (VS#0 f06 metadata block).
    /// Two encoding formats are recognised:
    ///   NEW  – `[03 03 01 01 CH ...]` → CH is the channel ID (1 byte, IDs 0-127)
    ///   OLD  – `[03 04 01 02 LO HI …]` → LE-u16 [LO,HI] is the channel ID
    pub preamble_channels: Vec<u32>,
    /// Human-readable summary suitable for a status tooltip.
    pub summary: String,
}

impl GarminRSDParser {
    /// Run a fast pre-parse probe: locate the first real sonar record, validate
    /// both varstruct CRCs, and return key metadata without decoding any samples.
    ///
    /// Garmin RSD files open with a few zero-data metadata records whose "body"
    /// region is actually the record trailer, not a varstruct.  We therefore walk
    /// forward through magic occurrences (up to MAX_PROBE_SCAN) until we find a
    /// record with data_size > 0 whose body varstruct successfully parses with at
    /// least one field.  This is the same "find GPS then backtrack to header"
    /// strategy used by the original Python explorer.
    #[allow(dead_code)]
    pub fn probe_file(&self, path: &Path) -> FileProbe {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                return FileProbe {
                    file_size: 0,
                    magic_found: None,
                    magic_offset: None,
                    header_crc_ok: false,
                    body_crc_ok: false,
                    first_channel: None,
                    first_channel_label: None,
                    first_record_fields: vec![],
                    estimated_records: None,
                    preamble_channels: vec![],
                    summary: format!("Probe failed: {e}"),
                };
            }
        };
        let file_size = bytes.len();
        let magic_candidates = load_magic_candidates(path);

        // Locate the first magic anywhere in the file (used for metadata only).
        let Some(first_magic_pos) = find_next_magic(&bytes, &magic_candidates, 0, bytes.len())
        else {
            return FileProbe {
                file_size,
                magic_found: None,
                magic_offset: None,
                header_crc_ok: false,
                body_crc_ok: false,
                first_channel: None,
                first_channel_label: None,
                first_record_fields: vec![],
                estimated_records: None,
                preamble_channels: vec![],
                summary: "Probe: no Garmin magic found — not a recognised RSD file".into(),
            };
        };

        let magic_val = le_u32(&bytes[first_magic_pos..first_magic_pos + 4]).unwrap_or(0);

        // ── Walk forward looking for the first real sonar record ─────────────
        // Metadata records have data_size=0; their "body" region is the record
        // trailer bytes which blow up varuint field_count decoding.  Skip them.
        const MAX_PROBE_SCAN: usize = 64;
        let mut scan_pos = first_magic_pos;
        let mut found_hdr_crc_ok = false;
        let mut found_body_crc_ok = false;
        let mut found_channel: Option<u32> = None;
        let mut found_fields: Vec<u32> = vec![];
        let mut found_data_size: usize = 0;
        let mut found_any = false;

        for _ in 0..MAX_PROBE_SCAN {
            // Try to locate the header varstruct that contains this magic.
            let mut hdr_start_opt: Option<(HashMap<u32, Vec<u8>>, usize, bool)> = None;
            for back in 1..=MAX_BACKTRACK_HEADER_START {
                if scan_pos < back {
                    break;
                }
                let start = scan_pos - back;
                let Ok(parsed) = parse_varstruct(&bytes, start, bytes.len(), CrcMode::Warn) else {
                    continue;
                };
                if parsed.0.get(&0).and_then(|b| le_u32(b)) == Some(magic_val) {
                    hdr_start_opt = Some((parsed.0, parsed.1, parsed.2));
                    break;
                }
            }

            if let Some((hdr, body_start, hdr_crc_ok)) = hdr_start_opt {
                let ds = le_u16(hdr.get(&4).unwrap_or(&vec![])).unwrap_or(0) as usize;
                if ds > 0 {
                    // Real data record — try to parse the body varstruct.
                    if let Ok((body, _, body_crc_ok)) =
                        parse_varstruct(&bytes, body_start, bytes.len(), CrcMode::Warn)
                    {
                        if !body.is_empty() {
                            found_hdr_crc_ok = hdr_crc_ok;
                            found_body_crc_ok = body_crc_ok;
                            found_channel =
                                le_u32_padded(body.get(&0).map(Vec::as_slice).unwrap_or(&[]));
                            let mut fields: Vec<u32> = body.keys().copied().collect();
                            fields.sort_unstable();
                            found_fields = fields;
                            found_data_size = ds;
                            found_any = true;
                            break;
                        }
                    }
                }
            }

            // Advance to next magic occurrence.
            let Some(next) = find_next_magic(&bytes, &magic_candidates, scan_pos + 4, bytes.len())
            else {
                break;
            };
            scan_pos = next;
        }

        let first_channel_label = found_channel.and_then(|ch| {
            map_channel_info(ch).map(|(beam, gen)| {
                format!(
                    "{} {}",
                    match gen {
                        "classic" => "Classic",
                        "uhd" => "UHD",
                        "uhd2" => "UHD2",
                        g => g,
                    },
                    match beam {
                        "port_sidescan" => "Port Sidescan",
                        "starboard_sidescan" => "Starboard Sidescan",
                        "port_sidescan_hf" => "Port Sidescan (HF, tentative)",
                        "starboard_sidescan_hf" => "Starboard Sidescan (HF, tentative)",
                        "chirp_downscan" => "Chirp Downscan",
                        "depth_temp" => "Depth/Temp",
                        b => b,
                    }
                )
            })
        });

        // Estimated record count: stride = data_size + ~51 bytes of overhead
        // (header 37 + body varstruct ~4 + trailer 12 ≈ 53, use 51 as conservative).
        let estimated_records = if found_data_size > 0 {
            Some(file_size / (found_data_size + 51))
        } else {
            None
        };

        let summary = if found_any {
            // Body CRC is a per-file sentinel (not a real checksum) — always mismatches.
            // Header CRC uses an unknown polynomial/seed — mismatches are normal.
            let hdr_tag = if found_hdr_crc_ok {
                "OK"
            } else {
                "mismatch (expected — unknown poly)"
            };
            let body_tag = if found_body_crc_ok {
                "OK"
            } else {
                "sentinel (not a checksum — normal)"
            };
            format!(
                "Probe OK: magic {:#010X} at +{}, hdr_crc={}, body_crc={}, first_ch={} ({}), body_fields={:?}, est_records={}",
                magic_val,
                first_magic_pos,
                hdr_tag,
                body_tag,
                found_channel.map(|v| v.to_string()).unwrap_or_else(|| "?".into()),
                first_channel_label.as_deref().unwrap_or("unknown"),
                found_fields,
                estimated_records.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
            )
        } else {
            format!(
                "Probe WARN: magic {:#010X} found at +{} but no real sonar record found in first {} candidates",
                magic_val, first_magic_pos, MAX_PROBE_SCAN,
            )
        };

        FileProbe {
            file_size,
            magic_found: Some(format!("{:#010X}", magic_val)),
            magic_offset: Some(first_magic_pos),
            header_crc_ok: found_hdr_crc_ok,
            body_crc_ok: found_body_crc_ok,
            first_channel: found_channel,
            first_channel_label,
            first_record_fields: found_fields,
            estimated_records,
            preamble_channels: scan_preamble_channel_ids(&bytes, first_magic_pos),
            summary,
        }
    }
}

/// Scan the file preamble (bytes 0..first_magic_pos) for channel IDs declared
/// in the VS#0 f06 calibration block.  Two encoding formats are handled:
///
/// NEW format (contemporary UHD/UHD2 firmware):
///   VS#0 f06 contains one entry per channel.  Each entry has the pattern
///   `03 03 01 01 CH …` where CH is the 1-byte channel ID (values 0-127).
///
/// OLD format (legacy single-channel firmware):
///   VS#0 f06 has exactly one entry starting with `03 04 01 02 LO HI …`
///   where [LO, HI] is the channel ID as a little-endian u16.
fn scan_preamble_channel_ids(bytes: &[u8], first_magic: usize) -> Vec<u32> {
    let limit = first_magic.min(bytes.len());
    if limit < 6 {
        return vec![];
    }
    let window = &bytes[..limit];
    let mut ids: Vec<u32> = Vec::new();

    // Scan for both patterns simultaneously.
    for i in 0..window.len().saturating_sub(5) {
        // NEW format: 03 03 01 01 CH
        if window[i] == 0x03
            && window[i + 1] == 0x03
            && window[i + 2] == 0x01
            && window[i + 3] == 0x01
        {
            let ch_id = window[i + 4] as u32;
            if !ids.contains(&ch_id) {
                ids.push(ch_id);
            }
        }
        // OLD format: 03 04 01 02 LO HI
        if i + 6 <= window.len()
            && window[i] == 0x03
            && window[i + 1] == 0x04
            && window[i + 2] == 0x01
            && window[i + 3] == 0x02
        {
            let ch_id = u16::from_le_bytes([window[i + 4], window[i + 5]]) as u32;
            if !ids.contains(&ch_id) {
                ids.push(ch_id);
            }
        }
    }
    ids
}

// ── generation detection (self-healing core) ──────────────────────────────────

/// Analyse the file preamble + first data record(s) to determine the RSD
/// hardware generation.  Returns `(generation, diagnostic_signals)`.
///
/// Detection strategy (strongest signal → weakest):
/// 1. Preamble channel IDs declared in VS#0 calibration block.
/// 2. First data-record body field 0 (channel ID) + field 7 / sonar_size ratio.
fn detect_generation(
    bytes: &[u8],
    first_magic: usize,
    magic_candidates: &[u32],
) -> (RsdGeneration, Vec<String>) {
    let mut signals: Vec<String> = Vec::new();

    // ── Signal 1: preamble channel IDs ────────────────────────────────────
    let preamble = scan_preamble_channel_ids(bytes, first_magic);
    if !preamble.is_empty() {
        let max_ch = *preamble.iter().max().unwrap();
        signals.push(format!("preamble_ch={:?}", preamble));
        if max_ch >= 8 {
            signals.push("preamble→UHD2".into());
            return (RsdGeneration::UHD2, signals);
        }
        if max_ch >= 4 {
            signals.push("preamble→UHD".into());
            return (RsdGeneration::UHD, signals);
        }
        signals.push("preamble→Gen1Classic".into());
        return (RsdGeneration::Gen1Classic, signals);
    }

    // ── Signal 2: first data record analysis ─────────────────────────────
    if let Some((ch, f7, sonar_sz)) = probe_first_data_record(bytes, first_magic, magic_candidates)
    {
        signals.push(format!("first_rec: ch={ch}, f7={f7}, sonar={sonar_sz}"));

        if ch >= 8 {
            return (RsdGeneration::UHD2, signals);
        }
        if ch >= 4 {
            return (RsdGeneration::UHD, signals);
        }
        // ch 0–3: could be Gen1,  but also Gen2 with low channel IDs
        if f7 > 0 && sonar_sz > 0 {
            let ratio = sonar_sz as f64 / f7 as f64;
            if (ratio - 1.0).abs() < 0.15 || (ratio - 2.0).abs() < 0.15 {
                // field7 looks like a plausible sample_count
                if f7 > 400 {
                    signals.push("f7>400→likely sample_count→UHD".into());
                    return (RsdGeneration::UHD, signals);
                }
            }
            if !(0.25..=4.0).contains(&ratio) {
                signals.push(format!("ratio={ratio:.1}→field7 is NOT sample_count→Gen1"));
                return (RsdGeneration::Gen1Classic, signals);
            }
        }
        if ch <= 3 {
            signals.push("ch≤3, ambiguous→Gen1Classic".into());
            return (RsdGeneration::Gen1Classic, signals);
        }
    }

    signals.push("no strong signal".into());
    (RsdGeneration::Unknown, signals)
}

/// Extract (channel_id, field7_value, sonar_size) from the first real data record.
fn probe_first_data_record(
    bytes: &[u8],
    first_magic: usize,
    magic_candidates: &[u32],
) -> Option<(u32, usize, usize)> {
    let magic_val = le_u32(&bytes[first_magic..first_magic + 4])?;
    let mut scan = first_magic;

    for _ in 0..20 {
        // Backtrack to find header varstruct containing this magic
        for back in 1..=MAX_BACKTRACK_HEADER_START {
            if scan < back {
                break;
            }
            let start = scan - back;
            let Ok((hdr, body_start, _)) =
                parse_varstruct(bytes, start, bytes.len(), CrcMode::Warn)
            else {
                continue;
            };
            if hdr.get(&0).and_then(|b| le_u32(b)) != Some(magic_val) {
                continue;
            }
            let ds = le_u16(hdr.get(&4).unwrap_or(&vec![])).unwrap_or(0) as usize;
            if ds == 0 {
                break; // metadata record, skip
            }
            let Ok((body, body_end, _)) =
                parse_varstruct(bytes, body_start, bytes.len(), CrcMode::Warn)
            else {
                break;
            };
            let ch = le_u32_padded(body.get(&0).map(Vec::as_slice).unwrap_or(&[])).unwrap_or(0);
            let f7 =
                le_u32_padded(body.get(&7).map(Vec::as_slice).unwrap_or(&[])).unwrap_or(0) as usize;
            let sonar_sz = ds.saturating_sub(body_end.saturating_sub(body_start));
            return Some((ch, f7, sonar_sz));
        }
        scan = find_next_magic(bytes, magic_candidates, scan + 4, bytes.len())?;
    }
    None
}

/// Quick probe to extract firmware version (header field 1) from the first record.
fn probe_firmware_version(
    bytes: &[u8],
    first_magic: usize,
    _magic_candidates: &[u32],
) -> Option<u32> {
    let magic_val = le_u32(&bytes[first_magic..first_magic + 4])?;
    for back in 1..=MAX_BACKTRACK_HEADER_START {
        if first_magic < back {
            break;
        }
        let start = first_magic - back;
        let Ok((hdr, _, _)) = parse_varstruct(bytes, start, bytes.len(), CrcMode::Warn) else {
            continue;
        };
        if hdr.get(&0).and_then(|b| le_u32(b)) != Some(magic_val) {
            continue;
        }
        return hdr.get(&1).and_then(|b| le_u32_padded(Some(b.as_slice())?));
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrcMode {
    Warn,
    #[allow(dead_code)]
    Strict,
    #[allow(dead_code)]
    Off,
}

fn load_magic_candidates(path: &Path) -> Vec<u32> {
    let mut out = vec![MAGIC_REC_HDR, 0xB7E9DA87, 0xB7E9DA88, 0xB7E9DA89];
    let mut candidates = Vec::<PathBuf>::new();
    if let Some(parent) = path.parent() {
        candidates.push(parent.join("garmin_magic_variants.txt"));
    }
    candidates.push(PathBuf::from("garmin_magic_variants.txt"));

    for file in candidates {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for line in text.lines() {
            let mut s = line.trim().to_ascii_lowercase();
            if s.is_empty() || s.starts_with('#') {
                continue;
            }
            if let Some(rest) = s.strip_prefix("0x") {
                s = rest.to_string();
            }
            if let Ok(v) = u32::from_str_radix(&s, 16) {
                if !out.contains(&v) {
                    out.push(v);
                }
            }
        }
    }
    out
}

pub(crate) fn find_next_magic(
    bytes: &[u8],
    candidates: &[u32],
    start: usize,
    end: usize,
) -> Option<usize> {
    let search_end = end.min(bytes.len());
    if search_end <= start + 4 {
        return None;
    }
    let mut i = start;
    while i + 4 <= search_end {
        let v = le_u32(&bytes[i..i + 4])?;
        if candidates.contains(&v) {
            return Some(i);
        }
        i += 1;
    }
    None
}

pub(crate) fn parse_varstruct(
    bytes: &[u8],
    mut pos: usize,
    limit: usize,
    crc_mode: CrcMode,
) -> Result<(HashMap<u32, Vec<u8>>, usize, bool), ()> {
    #![allow(clippy::type_complexity)]
    let start = pos;
    let (field_count, p) = read_varuint(bytes, pos, limit)?;
    pos = p;
    if field_count > 10_000 {
        return Err(());
    }

    let mut fields = HashMap::new();
    for _ in 0..field_count {
        let (key, p2) = read_varuint(bytes, pos, limit)?;
        pos = p2;
        let fn_id = key >> 3;
        let lc = key & 7;
        let vlen = if lc == 7 {
            let (v, p3) = read_varuint(bytes, pos, limit)?;
            pos = p3;
            v as usize
        } else {
            lc as usize
        };

        let endv = pos.saturating_add(vlen);
        if endv > limit || endv > bytes.len() {
            return Err(());
        }
        fields.insert(fn_id, bytes[pos..endv].to_vec());
        pos = endv;
    }

    if pos + 4 > limit || pos + 4 > bytes.len() {
        return Err(());
    }
    // CRC is stored little-endian (Python: struct.unpack('<I', ...))
    let crc_read = le_u32(&bytes[pos..pos + 4]).ok_or(())?;
    let crc_calc = crc32_custom(&bytes[start..pos]);
    pos += 4;

    let crc_ok = crc_calc == crc_read;
    if crc_mode == CrcMode::Strict && !crc_ok {
        return Err(());
    }

    Ok((fields, pos, crc_ok))
}

fn read_varuint(bytes: &[u8], mut pos: usize, limit: usize) -> Result<(u32, usize), ()> {
    let mut result = 0u32;
    let mut shift = 0u32;
    while pos < limit && pos < bytes.len() {
        let b = bytes[pos];
        pos += 1;
        result |= ((b & 0x7F) as u32) << shift;
        if b & 0x80 == 0 {
            return Ok((result, pos));
        }
        shift += 7;
        if shift > 35 {
            return Err(());
        }
    }
    Err(())
}

fn read_varint_from_slice(buf: &[u8]) -> Result<i32, ()> {
    let (u, _) = read_varuint(buf, 0, buf.len())?;
    let v = ((u >> 1) as i32) ^ (-((u & 1) as i32));
    Ok(v)
}

fn crc32_custom(data: &[u8]) -> u32 {
    let poly = 0x04C11DB7u32;
    let mut crc = 0u32;
    for b in data {
        crc ^= (*b as u32) << 24;
        for _ in 0..8 {
            if (crc & 0x8000_0000) != 0 {
                crc = (crc << 1) ^ poly;
            } else {
                crc <<= 1;
            }
        }
    }
    let mut rev = 0u32;
    let mut tmp = crc;
    for _ in 0..32 {
        rev = (rev << 1) | (tmp & 1);
        tmp >>= 1;
    }
    rev ^ 0xFFFF_FFFF
}

fn mapunit_to_deg(value: i32) -> f64 {
    value as f64 * (360.0 / ((1u64 << 32) as f64))
}

fn decode_samples(sonar: &[u8], sample_count: usize, hint: SampleHint) -> (Vec<u16>, String) {
    if sonar.is_empty() {
        return (Vec::new(), "none".to_string());
    }

    // Physical-first decode: always consume full payload width.
    // sample_count is treated as a label/hint, not a hard truncation limit.
    if hint == SampleHint::U8 {
        let out = sonar.iter().map(|v| (*v as u16) * 257).collect();
        return (out, "u8".to_string());
    }
    if hint == SampleHint::I16 {
        let mut out = Vec::with_capacity(sonar.len() / 2);
        let mut idx = 0usize;
        while idx + 1 < sonar.len() {
            let s = i16::from_le_bytes([sonar[idx], sonar[idx + 1]]) as i32;
            out.push(s.unsigned_abs().min(u16::MAX as u32) as u16);
            idx += 2;
        }
        return (out, "i16".to_string());
    }

    // --- Heuristic fallback for unknown channel IDs ---
    if sample_count > 0 {
        let ratio = sonar.len() as f64 / sample_count as f64;
        // 1.5x payload is usually packed 12-bit data.
        if (1.45..=1.60).contains(&ratio) && sonar.len() >= 3 {
            let out = unpack_12bit_le(sonar);
            if !out.is_empty() {
                return (out, "u12-packed".to_string());
            }
        }
        // 2x payload is 16-bit data.
        if (1.80..=2.20).contains(&ratio) && sonar.len() >= 2 {
            let mut out = Vec::with_capacity(sonar.len() / 2);
            let mut idx = 0usize;
            while idx + 1 < sonar.len() {
                let s = i16::from_le_bytes([sonar[idx], sonar[idx + 1]]) as i32;
                out.push(s.unsigned_abs().min(u16::MAX as u32) as u16);
                idx += 2;
            }
            return (out, "i16-by-ratio".to_string());
        }
        // 1x payload is 8-bit data.
        if (0.85..=1.15).contains(&ratio) {
            let out = sonar.iter().map(|v| (*v as u16) * 257).collect::<Vec<_>>();
            return (out, "u8-by-ratio".to_string());
        }
    }

    // Unknown ratio: prefer i16 for even payload sizes.
    if sonar.len() >= 2 && sonar.len().is_multiple_of(2) {
        let mut out = Vec::with_capacity(sonar.len() / 2);
        let mut idx = 0usize;
        while idx + 1 < sonar.len() {
            let s = i16::from_le_bytes([sonar[idx], sonar[idx + 1]]) as i32;
            out.push(s.unsigned_abs().min(u16::MAX as u32) as u16);
            idx += 2;
        }
        return (out, "i16-fallback-full".to_string());
    }

    let out = sonar.iter().map(|v| (*v as u16) * 257).collect::<Vec<_>>();
    (out, "u8-fallback-full".to_string())
}

fn unpack_12bit_le(sonar: &[u8]) -> Vec<u16> {
    if sonar.len() < 3 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity((sonar.len() / 3) * 2);
    let mut i = 0usize;
    while i + 2 < sonar.len() {
        let b0 = sonar[i] as u16;
        let b1 = sonar[i + 1] as u16;
        let b2 = sonar[i + 2] as u16;
        let a = b0 | ((b1 & 0x0F) << 8);
        let b = (b1 >> 4) | (b2 << 4);
        out.push(a << 4);
        out.push(b << 4);
        i += 3;
    }
    out
}

fn decoded_payload_bytes(sample_format: &str, sample_len: usize) -> usize {
    if sample_len == 0 {
        return 0;
    }
    if sample_format.starts_with("u12") {
        return (sample_len * 3).div_ceil(2);
    }
    if sample_format.starts_with("i16") {
        return sample_len * 2;
    }
    sample_len
}

fn detect_low_energy_voids(samples: &[u16]) -> Vec<usize> {
    if samples.len() < 64 {
        return Vec::new();
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let p10 = sorted[(sorted.len() / 10).min(sorted.len() - 1)];
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, &v) in samples.iter().enumerate() {
        if v <= p10 {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            runs.push((s, i));
        }
    }
    if let Some(s) = start {
        runs.push((s, samples.len()));
    }
    runs.sort_by_key(|(s, e)| std::cmp::Reverse(e.saturating_sub(*s)));
    runs.into_iter()
        .filter(|(s, e)| e.saturating_sub(*s) >= 8)
        .take(2)
        .map(|(s, e)| (s + e) / 2)
        .collect()
}

fn infer_sample_hint_from_layout(
    sonar_size: usize,
    sample_count: usize,
    sonar_len: usize,
) -> SampleHint {
    if sample_count == 0 {
        return SampleHint::Unknown;
    }
    let effective = sonar_size.max(sonar_len);
    if effective == 0 {
        return SampleHint::Unknown;
    }
    let ratio = effective as f64 / sample_count as f64;
    // Allow small framing overhead around exact 1x/2x layouts.
    if (ratio - 2.0).abs() <= 0.25
        || (effective as isize - (sample_count * 2) as isize).unsigned_abs() <= 12
    {
        return SampleHint::I16;
    }
    if (ratio - 1.0).abs() <= 0.25
        || (effective as isize - sample_count as isize).unsigned_abs() <= 12
    {
        return SampleHint::U8;
    }
    SampleHint::Unknown
}

fn normalize_sample_count(
    raw_count: usize,
    sonar_size: usize,
    sonar_len: usize,
    hint: SampleHint,
) -> usize {
    let effective = sonar_size.min(sonar_len);
    if effective == 0 {
        return 0;
    }

    let fallback = match hint {
        SampleHint::I16 => (effective / 2).max(1),
        _ => effective,
    };

    if raw_count == 0 {
        return fallback;
    }

    match hint {
        SampleHint::I16 => {
            let expected = raw_count.saturating_mul(2);
            if expected.abs_diff(effective) <= 16 {
                raw_count
            } else {
                fallback
            }
        }
        SampleHint::U8 => {
            if raw_count.abs_diff(effective) <= 16 {
                raw_count
            } else {
                fallback
            }
        }
        SampleHint::Unknown => raw_count.min(fallback).max(1),
    }
}

fn decode_timestamp_ms(
    hdr: &std::collections::HashMap<u32, Vec<u8>>,
    body: &std::collections::HashMap<u32, Vec<u8>>,
    sequence: u32,
) -> u64 {
    // Canonical Garmin timestamp field in most captures.
    if let Some(raw) = hdr.get(&5) {
        if let Some(ts) = decode_timestamp_candidate(raw) {
            if ts > 0 {
                return ts;
            }
        }
    }

    // Try other likely header/body fields used by some firmware variants.
    for fid in [6u32, 3, 1, 8] {
        if let Some(raw) = hdr.get(&fid) {
            if let Some(ts) = decode_timestamp_candidate(raw) {
                if ts > 0 {
                    return ts;
                }
            }
        }
    }
    for fid in [5u32, 6, 15, 16] {
        if let Some(raw) = body.get(&fid) {
            if let Some(ts) = decode_timestamp_candidate(raw) {
                if ts > 0 {
                    return ts;
                }
            }
        }
    }

    // Last resort so downstream sort/segmentation still gets monotonic-ish time.
    (sequence as u64).saturating_mul(40)
}

fn decode_timestamp_candidate(raw: &[u8]) -> Option<u64> {
    if raw.is_empty() {
        return None;
    }

    // 64-bit little-endian timestamp (ms or us depending on firmware).
    if raw.len() >= 8 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&raw[..8]);
        let v = u64::from_le_bytes(b);
        if (1_000..=4_102_444_800_000).contains(&v) {
            return Some(v);
        }
        // Microseconds -> milliseconds.
        if (1_000_000..=4_102_444_800_000_000).contains(&v) {
            return Some(v / 1_000);
        }
    }

    if let Some(v32) = le_u32_padded(raw) {
        let v = v32 as u64;
        if (1_000..=4_102_444_800_000).contains(&v) {
            return Some(v);
        }
        // Unix epoch seconds -> ms.
        if (100_000_000..=4_102_444_800).contains(&v) {
            return Some(v * 1_000);
        }
    }

    if let Ok(vi) = read_varint_from_slice(raw) {
        if vi > 0 {
            let v = vi as u64;
            if (1_000..=4_102_444_800_000).contains(&v) {
                return Some(v);
            }
            if (100_000_000..=4_102_444_800).contains(&v) {
                return Some(v * 1_000);
            }
        }
    }

    None
}

fn decode_angle_candidate(raw: &[u8], min_deg: f32, max_deg: f32) -> Option<f32> {
    if raw.is_empty() {
        return None;
    }

    if let Some(vf) = le_f32(raw) {
        if vf.is_finite() && (vf == 0.0 || vf.abs() >= 0.01) && vf >= min_deg && vf <= max_deg {
            return Some(vf);
        }
    }

    if let Some(vi) = le_i32(raw) {
        for scale in [1000.0f32, 100.0f32] {
            let v = (vi as f32) / scale;
            if v.is_finite() && (v == 0.0 || v.abs() >= 0.01) && v >= min_deg && v <= max_deg {
                return Some(v);
            }
        }
    }

    if raw.len() >= 2 {
        if let Ok(vi) = read_varint_from_slice(raw) {
            for scale in [1000.0f32, 100.0f32] {
                let v = (vi as f32) / scale;
                if v.is_finite() && (v == 0.0 || v.abs() >= 0.01) && v >= min_deg && v <= max_deg {
                    return Some(v);
                }
            }
        }
    }

    None
}

fn pick_angle_from_fields(
    fields: &std::collections::HashMap<u32, Vec<u8>>,
    preferred_ids: &[u32],
    min_deg: f32,
    max_deg: f32,
) -> Option<f32> {
    for fid in preferred_ids {
        if let Some(raw) = fields.get(fid) {
            if let Some(v) = decode_angle_candidate(raw, min_deg, max_deg) {
                return Some(v);
            }
        }
    }

    let mut ids: Vec<u32> = fields.keys().copied().collect();
    ids.sort_unstable();
    for fid in ids {
        if preferred_ids.contains(&fid) {
            continue;
        }
        // Skip known non-angle IDs.
        if matches!(fid, 0 | 1 | 5 | 6 | 7 | 9 | 10 | 14) {
            continue;
        }
        if let Some(raw) = fields.get(&fid) {
            if let Some(v) = decode_angle_candidate(raw, min_deg, max_deg) {
                return Some(v);
            }
        }
    }
    None
}

fn pick_heading_from_fields(
    fields: &std::collections::HashMap<u32, Vec<u8>>,
    preferred_ids: &[u32],
) -> Option<f32> {
    let raw = pick_angle_from_fields(fields, preferred_ids, -180.0, 360.0)?;
    // Normalize to [0, 360)
    let mut h = raw;
    if h < 0.0 {
        h += 360.0;
    }
    if h >= 360.0 {
        h -= 360.0 * (h / 360.0).floor();
    }
    Some(h)
}

pub(crate) fn le_u16(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 2 {
        return None;
    }
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn le_i32(bytes: &[u8]) -> Option<i32> {
    if bytes.len() < 4 {
        return None;
    }
    Some(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn le_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 4 {
        return None;
    }
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Matches Python's `int.from_bytes(val[:4].ljust(4, b'\x00'), 'little')` —
/// decodes as little-endian u32, padding shorts (<4 bytes) with zeros.
pub(crate) fn le_u32_padded(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    let mut buf = [0u8; 4];
    let n = bytes.len().min(4);
    buf[..n].copy_from_slice(&bytes[..n]);
    Some(u32::from_le_bytes(buf))
}

fn le_f32(bytes: &[u8]) -> Option<f32> {
    if bytes.len() < 4 {
        return None;
    }
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[allow(dead_code)]
fn be_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 4 {
        return None;
    }
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_custom_roundtrip_nonzero() {
        let data = b"garmin-rsd-crc-test";
        let crc = crc32_custom(data);
        assert_ne!(crc, 0);
    }

    #[test]
    fn varint_zigzag_decode() {
        // zigzag-encoded 123 -> 246 (0xF6 0x01)
        let buf = [0xF6u8, 0x01u8];
        let v = read_varint_from_slice(&buf).expect("varint should decode");
        assert_eq!(v, 123);
    }

    #[test]
    fn parse_real_recovery_sample_if_present() {
        let sample = Path::new(
            "D:\\Temp\\cesarops_repo_tmp\\Garminjunk\\archive\\HistoryofCESARSNIFFERBAGFILE\\Sonar Samples\\Sonar002.RSD",
        );
        if !sample.exists() {
            return;
        }

        let mut parser = GarminRSDParser::new();
        let result = parser.parse_file(sample);
        assert!(result.error_message.is_none(), "{:?}", result.error_message);
        assert!(
            result.record_count > 0,
            "Expected records from recovery sample, got 0"
        );
    }

    /// Verify that Holloway.RSD produces channels 4 (port) and 5 (starboard),
    /// matching the Python v5 DualEngine reference output (81,390 records,
    /// roughly equal counts on ch4 and ch5).
    #[test]
    fn holloway_channels_4_and_5() {
        let path = Path::new(
            "D:\\Temp\\cesarops_repo_tmp\\Garminjunk\\archive\\HistoryofCESARSNIFFERBAGFILE\\Sonar Samples\\Holloway.RSD",
        );
        if !path.exists() {
            return; // skip if sample not available
        }

        let mut parser = GarminRSDParser::new();
        let result = parser.parse_file(path);

        assert!(result.error_message.is_none(), "{:?}", result.error_message);
        assert!(result.record_count > 0, "No records parsed");

        assert!(
            result.channel_counts.contains_key(&4),
            "Expected channel 4 (port_sidescan); got channels: {:?}",
            result.channel_counts.keys().collect::<Vec<_>>()
        );
        assert!(
            result.channel_counts.contains_key(&5),
            "Expected channel 5 (starboard_sidescan); got channels: {:?}",
            result.channel_counts.keys().collect::<Vec<_>>()
        );

        // Counts should be roughly equal (alternating port/star pings)
        let ch4 = result.channel_counts[&4];
        let ch5 = result.channel_counts[&5];
        let ratio = ch4.min(ch5) as f64 / ch4.max(ch5) as f64;
        assert!(
            ratio > 0.9,
            "Channel counts wildly unequal: ch4={ch4} ch5={ch5}"
        );

        // Channel type mapped correctly
        let ch4_info = result.channels.iter().find(|c| c.id == 4).unwrap();
        let ch5_info = result.channels.iter().find(|c| c.id == 5).unwrap();
        assert_eq!(ch4_info.mapped_type.as_deref(), Some("port_sidescan"));
        assert_eq!(ch5_info.mapped_type.as_deref(), Some("starboard_sidescan"));
    }

    /// 126SV-UHD2-GT54.RSD: test capture without GPS.
    /// Body field 0 is absent so all records default to channel 0.
    /// CRC mismatches are expected and silenced.
    #[test]
    fn gt54_parses_records() {
        let path = Path::new(
            "D:\\Temp\\cesarops_repo_tmp\\Garminjunk\\archive\\HistoryofCESARSNIFFERBAGFILE\\Sonar Samples\\126SV-UHD2-GT54.RSD",
        );
        if !path.exists() {
            return;
        }
        let mut parser = GarminRSDParser::new();
        let result = parser.parse_file(path);
        assert!(result.error_message.is_none(), "{:?}", result.error_message);
        assert!(
            result.record_count > 5_000,
            "Expected >5000 records from GT54, got {}",
            result.record_count
        );
        eprintln!(
            "GT54: {} records, channels={:?}, CRC mismatches={}",
            result.record_count,
            result.channel_counts.keys().collect::<Vec<_>>(),
            result.crc_mismatch_count
        );
    }

    /// Sonar001.RSD: mid-length capture.
    #[test]
    fn sonar001_parses_records() {
        let path = Path::new(
            "D:\\Temp\\cesarops_repo_tmp\\Garminjunk\\archive\\HistoryofCESARSNIFFERBAGFILE\\Sonar Samples\\Sonar001.RSD",
        );
        if !path.exists() {
            return;
        }
        let mut parser = GarminRSDParser::new();
        let result = parser.parse_file(path);
        assert!(result.error_message.is_none(), "{:?}", result.error_message);
        assert!(
            result.record_count > 5_000,
            "Expected >5000 records from Sonar001, got {}",
            result.record_count
        );
        eprintln!(
            "Sonar001: {} records, channels={:?}, CRC mismatches={}",
            result.record_count,
            result.channel_counts.keys().collect::<Vec<_>>(),
            result.crc_mismatch_count
        );
    }

    /// End-to-end smoke test across all 4 sample files.
    /// CRC mismatches are expected (particularly GT54 test captures) — they are
    /// counted in `crc_mismatch_count` and never abort parsing.
    #[test]
    fn all_four_samples_end_to_end() {
        let sample_dir = Path::new(
            "D:\\Temp\\cesarops_repo_tmp\\Garminjunk\\archive\\HistoryofCESARSNIFFERBAGFILE\\Sonar Samples",
        );
        let files: &[(&str, usize)] = &[
            ("126SV-UHD2-GT54.RSD", 5_000),
            ("Holloway.RSD", 50_000),
            ("Sonar000.RSD", 100_000),
            ("Sonar001.RSD", 5_000),
        ];
        let mut any_found = false;
        for (filename, min_records) in files {
            let path = sample_dir.join(filename);
            if !path.exists() {
                eprintln!("Skipping missing file: {filename}");
                continue;
            }
            any_found = true;
            let mut parser = GarminRSDParser::new();
            let result = parser.parse_file(&path);
            assert!(
                result.error_message.is_none(),
                "{filename}: parse error: {:?}",
                result.error_message
            );
            assert!(
                result.record_count >= *min_records,
                "{filename}: expected >={min_records} records, got {}",
                result.record_count
            );
            eprintln!(
                "{filename}: {} records, channels={:?}, CRC mismatches={}",
                result.record_count,
                result.channel_counts.keys().collect::<Vec<_>>(),
                result.crc_mismatch_count
            );
        }
        if !any_found {
            eprintln!("No sample files present — end-to-end test skipped.");
        }
    }

    /// Verify Sonar000.RSD also separates into port + starboard channels.
    #[test]
    fn sonar000_channels_4_and_5() {
        let path = Path::new(
            "D:\\Temp\\cesarops_repo_tmp\\Garminjunk\\archive\\HistoryofCESARSNIFFERBAGFILE\\Sonar Samples\\Sonar000.RSD",
        );
        if !path.exists() {
            return;
        }

        let mut parser = GarminRSDParser::new();
        let result = parser.parse_file(path);

        assert!(
            result.record_count > 0,
            "No records parsed from Sonar000.RSD"
        );
        assert!(
            result.channel_counts.contains_key(&4) || result.channel_counts.contains_key(&5),
            "Expected ch4 or ch5; got: {:?}",
            result.channel_counts.keys().collect::<Vec<_>>()
        );
    }

    /// Diagnostic: compare header field 3 and other low-cardinality fields across
    /// GT54 and GT56 transducer files. Header field 3 (key 103) is the candidate
    /// transducer ID — if it differs between GT54 and GT56 captures that confirms it.
    ///
    /// Run with:  cargo test gt56_transducer_id_diagnostic -- --nocapture
    #[test]
    fn gt56_transducer_id_diagnostic() {
        let files = [
            "D:\\Temp\\cesarops_repo_tmp\\Garminjunk\\archive\\HistoryofCESARSNIFFERBAGFILE\\Sonar Samples\\126SV-UHD2-GT54.RSD",
            "C:\\Users\\thomf\\CrossDevice\\NautiDog Sailing's S25 Ultra\\storage\\Download\\126SV-UHD2-GT56.RSD",
        ];
        for filename in &files {
            let path = Path::new(filename);
            if !path.exists() {
                eprintln!("SKIP (not found): {filename}");
                continue;
            }
            let mut parser = GarminRSDParser::new();
            let result = parser.parse_file(path);
            let label = path.file_name().unwrap().to_string_lossy();
            eprintln!("=== {label} ===");
            eprintln!(
                "  records={}, channels={:?}",
                result.record_count,
                result.channel_counts.keys().collect::<Vec<_>>()
            );
            eprintln!(
                "  device_id (body f13): {:?}",
                result.unique_field_values.get(&13)
            );
            eprintln!(
                "  hdr f1  (fw ver):     {:?}",
                result.unique_field_values.get(&101)
            );
            eprintln!(
                "  hdr f3  (xducer id?): {:?}",
                result.unique_field_values.get(&103)
            );
            eprintln!(
                "  body f6 (gen enum):   {:?}",
                result.unique_field_values.get(&6)
            );
            eprintln!(
                "  body f12 (beam type): {:?}",
                result.unique_field_values.get(&12)
            );
            // also dump all low-cardinality header fields (100-199)
            let hdr_fields: Vec<_> = result
                .unique_field_values
                .iter()
                .filter(|(k, v)| **k >= 100 && **k < 200 && v.len() <= 8)
                .collect();
            if !hdr_fields.is_empty() {
                eprintln!("  Low-cardinality header fields:");
                for (fid, vals) in hdr_fields {
                    eprintln!("    HDR f{}: {:?}", fid - 100, vals);
                }
            }
        }
    }

    /// Diagnostic: dump unique_field_values for GT54 bench file to compare
    /// frequency/band field values across sidescan (ch1,5) and downscan (ch2,10).
    ///
    /// Run with:  cargo test gt54_field_diagnostic -- --nocapture
    #[test]
    fn gt54_field_diagnostic() {
        for filename in &[
            "D:\\Temp\\cesarops_repo_tmp\\Garminjunk\\archive\\HistoryofCESARSNIFFERBAGFILE\\Sonar Samples\\126SV-UHD2-GT54.RSD",
        ] {
            let path = Path::new(filename);
            if !path.exists() { continue; }
            let mut parser = GarminRSDParser::new();
            let result = parser.parse_file(path);
            eprintln!("=== {} unique_field_values ===", path.file_name().unwrap().to_string_lossy());
            eprintln!("  channels: {:?}", result.channel_counts.keys().collect::<Vec<_>>());
            for (fid, vals) in &result.unique_field_values {
                let loc = if *fid >= 100 { format!("HDR f{}", fid-100) } else { format!("body f{fid}") };
                // only show low-cardinality fields
                if vals.len() <= 8 {
                    eprintln!("  [{loc}] ({} distinct): {:?}", vals.len(), vals);
                }
            }
            // Per-channel breakdown for fields 6 and 12 (candidate freq/type fields)
            for cand_fid in [6u32, 12u32] {
                if let Some(ch_map) = result.field_channel_counts.get(&cand_fid) {
                    eprintln!("  field {} per-value counts: {:?}", cand_fid, ch_map);
                }
            }
        }
    }
    /// from Holloway.RSD for reverse-engineering frequency / offset metadata.
    ///
    /// Run with:  cargo test holloway_field_diagnostic -- --nocapture
    #[test]
    fn holloway_field_diagnostic() {
        let path = Path::new(
            "D:\\Temp\\cesarops_repo_tmp\\Garminjunk\\archive\\HistoryofCESARSNIFFERBAGFILE\\Sonar Samples\\Holloway.RSD",
        );
        if !path.exists() {
            eprintln!("holloway_field_diagnostic: file not found, skipping");
            return;
        }

        let mut parser = GarminRSDParser::new();
        let result = parser.parse_file(path);

        eprintln!("=== Holloway unique_field_values ===");
        eprintln!("  (body fields 0-99, header fields 100-199)");
        for (fid, vals) in &result.unique_field_values {
            let location = if *fid >= 100 {
                format!("HDR field {}", fid - 100)
            } else {
                format!("body field {fid}")
            };
            let vals_str: Vec<String> = vals.iter().map(|v| format!("{v}")).collect();
            eprintln!(
                "  [{location}] ({} distinct): {}",
                vals.len(),
                vals_str.join(", ")
            );
        }

        // Per-channel breakdown for candidate frequency/type fields
        for cand_fid in [3u32, 6u32, 8u32, 12u32] {
            if let Some(ch_map) = result.field_channel_counts.get(&cand_fid) {
                eprintln!("  body field {} value-counts: {:?}", cand_fid, ch_map);
            }
        }

        // Depth stats in feet
        let depths_ft: Vec<f32> = result
            .pings
            .iter()
            .filter(|p| p.depth_ft > 0.1)
            .map(|p| p.depth_ft)
            .collect();
        if !depths_ft.is_empty() {
            let min_ft = depths_ft.iter().cloned().fold(f32::MAX, f32::min);
            let max_ft = depths_ft.iter().cloned().fold(f32::MIN, f32::max);
            let avg_ft = depths_ft.iter().sum::<f32>() / depths_ft.len() as f32;
            eprintln!("=== Depth stats ({} non-zero pings) ===", depths_ft.len());
            eprintln!(
                "  min={:.1}ft  avg={:.1}ft  max={:.1}ft",
                min_ft, avg_ft, max_ft
            );
            eprintln!(
                "  First 10 depths (ft): {}",
                depths_ft[..depths_ft.len().min(10)]
                    .iter()
                    .map(|d| format!("{:.1}", d))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
}
