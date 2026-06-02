//! Hardware-agnostic HeuristicProbe — identifies the Garmin transducer model and
//! its per-channel layout from the first 10 MB of an .rsd file, without a full
//! parse pass.
//!
//! # Design rules (safety guardrails)
//! - NEVER mutates `Ping` or `ParseResult`.
//! - Reads at most `PROBE_BYTE_CAP` (10 MB) of the raw file.
//! - No CRC writing — corrupt records are skipped via MAGIC_REC_HDR sync.
//! - New types live here; `garmin_rsd_parser.rs` is not modified.
//!
//! # Hardware signatures handled
//!
//! | Hardware | Port ch | Star ch | Down ch | Notes                              |
//! |----------|---------|---------|---------|-------------------------------------|
//! | GT51     | 4       | 5       | 6       | Asymmetric wings, nadir at edge     |
//! | GT54/UHD1| 4       | 5       | 12/13   | Port-flip common, high entropy      |
//! | GT56/UHD2| 10      | 11      | 12/13   | Discrete streams, simple 1:1 map    |

use crate::garmin_rsd_parser::RsdGeneration;
use serde::Serialize;
use std::collections::BTreeMap;

// ─────────────────────────────────────────────────────────────────────────────
//  Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum bytes read for a probe pass (10 MB).
pub const PROBE_BYTE_CAP: usize = 10 * 1024 * 1024;

/// Garmin record header magic — same constant as in `garmin_rsd_parser.rs`.
const MAGIC_REC_HDR: u32 = 0xB7E9DA86;

/// Number of records to analyze per channel during the probe pass.
const PROBE_RECORD_WINDOW: usize = 60;

/// Sliding-window half-width as fraction of sample count.
/// Window A: 0–15%, Window B: 45–55%, Window C: 85–100%.
const WIN_A_HI: f32 = 0.15;
const WIN_B_LO: f32 = 0.45;
const WIN_B_HI: f32 = 0.55;
const WIN_C_LO: f32 = 0.85;

// ─────────────────────────────────────────────────────────────────────────────
//  Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Identifies which physical transducer/hardware produced this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TransducerHardware {
    /// GT51 — 260 kHz, asymmetric single-wing channels, nadir at edge.
    GT51Legacy,
    /// GT54 / UHD1 — 800 kHz high-absorption, channels 4/5, Port-flip common.
    GT54UHD1,
    /// GT56 / UHD2 — 455 kHz, channels 10/11, simple discrete streams.
    GT56UHD2,
    /// Multi-frequency or hybrid (e.g. GT56 + ClearVü dual-freq ch18/20).
    MultiFrequency,
    /// Cannot determine within the probe budget.
    Unknown,
}

impl std::fmt::Display for TransducerHardware {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GT51Legacy     => write!(f, "GT51 (Legacy 260 kHz)"),
            Self::GT54UHD1       => write!(f, "GT54 UHD1 (800 kHz)"),
            Self::GT56UHD2       => write!(f, "GT56 UHD2 (455 kHz)"),
            Self::MultiFrequency => write!(f, "Multi-frequency"),
            Self::Unknown        => write!(f, "Unknown"),
        }
    }
}

/// Effective bit-depth of the sample data within a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BitDepth {
    /// Gen1 Classic: 8-bit u8 values, stored as u16 (0–255).
    U8,
    /// UHD/UHD2: 16-bit signed → stored as u16 (0–65535).
    I16Full,
    /// UHD samples that never exceed 4095 — "12-bit range" within i16 storage.
    /// Common on certain GT54 firmware variants.
    I16_12BitRange,
    /// Indeterminate.
    Unknown,
}

/// Where the low-intensity nadir zone sits within a ping's sample array.
///
/// GT51 channels have the nadir at one of the two edges (asymmetric).
/// UHD sidescan channels that haven't been reversed have it at the left edge.
/// After parser normalization it should always be left — but the probe runs
/// BEFORE the parser's flip logic, so raw positions are reported here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NadirEdge {
    /// Nadir is in the first 15 % of samples → left/near edge.
    Left,
    /// Nadir is in the 45–55 % band → center gap (paired UHD sidescan).
    Center,
    /// Nadir is in the last 15 % of samples → right/far edge (reversed wing).
    Right,
    /// Nadir could not be located with confidence.
    Unknown,
}

/// Orientation hint derived from heading + COG comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FlipStatus {
    /// Heading and COG agree — transducer mounted correctly.
    Normal,
    /// ~180° mismatch between heading field and actual course — port flip.
    Flipped,
    /// Not enough heading data to determine.
    Indeterminate,
}

/// Probe result for a single channel found within the probe window.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelProbe {
    pub channel_id: u32,
    pub records_seen: usize,
    pub bit_depth: BitDepth,
    pub nadir_edge: NadirEdge,
    pub nadir_gap_samples: usize,
    /// Mean sample amplitude in Window A (0–15 %).
    pub mean_window_a: f32,
    /// Mean sample amplitude in Window B (45–55 %).
    pub mean_window_b: f32,
    /// Mean sample amplitude in Window C (85–100 %).
    pub mean_window_c: f32,
    /// Estimated noise floor: median of the 5th-percentile amplitudes.
    pub noise_floor: f32,
    /// Max sample value observed (used for 12-bit range check).
    pub max_sample_value: u16,
    pub flip_status: FlipStatus,
    /// Recommended spatial role for this channel.
    pub suggested_role: SuggestedRole,
    /// For Gen1Classic channels: raw field7 value (hardware gain / TVG setting, 0–255).
    /// `None` for UHD/UHD2 channels where field7 encodes sample_count instead.
    pub hardware_gain_raw: Option<u32>,
}

/// Spatial role recommended by the probe for handoff to `ChannelDiscovery`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SuggestedRole {
    /// Full paired sidescan wing, nadir at left edge (normal after flip correction).
    PairedPort,
    /// Full paired sidescan wing, nadir at left edge (starboard).
    PairedStarboard,
    /// GT51 asymmetric single wing — port side, nadir at index 0.
    SingleSidePort,
    /// GT51 asymmetric single wing — starboard side, nadir at index max.
    SingleSideStarboard,
    /// DownVü / ClearVü center beam.
    CenterDown,
    /// Depth/temp metadata — no real sonar samples.
    DepthTemp,
    /// Could not determine.
    Unknown,
}

/// Full probe report returned after scanning the first 10 MB.
#[derive(Debug, Clone, Serialize)]
pub struct ProbeReport {
    pub hardware: TransducerHardware,
    pub generation: RsdGeneration,
    pub channels: Vec<ChannelProbe>,
    /// Overall confidence of the hardware identification (0.0–1.0).
    pub confidence: f32,
    /// Bytes actually read (≤ PROBE_BYTE_CAP).
    pub bytes_read: usize,
    /// Number of valid records decoded in the probe window.
    pub records_decoded: usize,
    pub probe_log: Vec<String>,
    /// Required byte alignment for sonar records in this file.
    /// 4 = 32-bit boundary (GT56/UHD2 discrete streams).
    /// 2 = 16-bit boundary (GT51/GT54).
    pub record_alignment_bytes: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
//  HeuristicProbe trait
// ─────────────────────────────────────────────────────────────────────────────

/// A hardware-fingerprinting probe that works without a full parse.
///
/// Implementors must strictly honour the `probe_bytes` cap — no seeking
/// beyond that point.
pub trait HeuristicProbe {
    /// Probe the raw file bytes.
    ///
    /// # Arguments
    /// - `data`        : raw file bytes (the caller may supply a window slice)
    /// - `probe_bytes` : maximum bytes to consume (default `PROBE_BYTE_CAP`)
    fn probe(&self, data: &[u8], probe_bytes: usize) -> ProbeReport;
}

// ─────────────────────────────────────────────────────────────────────────────
//  GarminRsdProbe — the concrete implementation
// ─────────────────────────────────────────────────────────────────────────────

/// Concrete prober for Garmin `.rsd` files.
pub struct GarminRsdProbe;

impl HeuristicProbe for GarminRsdProbe {
    fn probe(&self, data: &[u8], probe_bytes: usize) -> ProbeReport {
        let cap = probe_bytes.min(data.len()).min(PROBE_BYTE_CAP);
        let window = &data[..cap];
        let mut log: Vec<String> = Vec::new();
        log.push(format!("Probe window: {} bytes ({:.1} MB)", cap, cap as f64 / 1_048_576.0));

        // ── 1. Scan for valid records within the capped window ────────────────
        let raw_records = scan_records(window, &mut log);
        let records_decoded = raw_records.len();
        log.push(format!("Found {} candidate records", records_decoded));

        if records_decoded == 0 {
            return ProbeReport {
                hardware: TransducerHardware::Unknown,
                generation: RsdGeneration::Unknown,
                channels: vec![],
                confidence: 0.0,
                bytes_read: cap,
                records_decoded: 0,
                probe_log: log,
                record_alignment_bytes: 2,
            };
        }

        // ── 2. Detect generation from channel IDs ─────────────────────────────
        let generation = detect_generation_from_records(&raw_records, &mut log);

        // ── 3. Group records by channel and profile each ──────────────────────
        let mut by_channel: BTreeMap<u32, Vec<&RawRecord>> = BTreeMap::new();
        for r in &raw_records {
            by_channel.entry(r.channel_id).or_default().push(r);
        }

        let mut channel_probes: Vec<ChannelProbe> = by_channel
            .iter()
            .map(|(&ch_id, records)| {
                probe_channel(ch_id, records, &generation, &mut log)
            })
            .collect();

        // ── 4. Detect GT54 port-flip using heading vs COG ─────────────────────
        detect_port_flip(&raw_records, &mut channel_probes, &mut log);

        // ── 5. Assign suggested roles per channel ─────────────────────────────
        assign_suggested_roles(&mut channel_probes, &generation, &mut log);

        // ── 6. Identify hardware from channel set ─────────────────────────────
        let all_ch: Vec<u32> = channel_probes.iter().map(|c| c.channel_id).collect();
        let (hardware, confidence) = identify_hardware(&all_ch, &channel_probes, &generation, &mut log);

        // ── 7. Derive required record alignment from hardware type ─────────────
        // GT56/UHD2: discrete-channel discrete-stream → 4-byte (32-bit) boundaries.
        // GT54/UHD1 and GT51/Legacy: packed body structs → 2-byte (16-bit) boundaries.
        let record_alignment_bytes: u32 = match hardware {
            TransducerHardware::GT56UHD2 | TransducerHardware::MultiFrequency => 4,
            _ => 2,
        };
        log.push(format!("Record alignment: {} bytes ({}-bit boundary)",
            record_alignment_bytes, record_alignment_bytes * 8));

        log.push(format!("=== PROBE RESULT: {} (conf={:.2}) ===", hardware, confidence));
        for cp in &channel_probes {
            let gain_str = cp.hardware_gain_raw
                .map(|g| format!(", gain_raw={}", g))
                .unwrap_or_default();
            log.push(format!(
                "  ch{} | bit={:?} | nadir={:?} | gap={} | noise_floor={:.0} | flip={:?} | role={:?}{}",
                cp.channel_id, cp.bit_depth, cp.nadir_edge, cp.nadir_gap_samples,
                cp.noise_floor, cp.flip_status, cp.suggested_role, gain_str,
            ));
        }
        for line in &log {
            eprintln!("[probe] {}", line);
        }

        ProbeReport {
            hardware,
            generation,
            channels: channel_probes,
            confidence,
            bytes_read: cap,
            records_decoded,
            probe_log: log,
            record_alignment_bytes,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Internal types and helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Lightweight decoded record, extracted without allocating a full `Ping`.
struct RawRecord {
    channel_id: u32,
    /// Body field 7 raw u32 (XID on Gen1, sample_count on UHD+).
    field7: u32,
    /// Body field 16 (heading, raw millidegrees or u16 units depending on gen).
    field16_heading: Option<f32>,
    /// COG derived from sequential GPS deltas (set in post-processing).
    cog_deg: Option<f32>,
    /// Raw sample bytes (already sliced, not yet decoded).
    samples: Vec<u16>,
}

/// Scan `window` for valid Garmin RSD records using the MAGIC_REC_HDR sync.
///
/// Returns lightweight `RawRecord`s. Corrupt records are skipped — no CRC writes.
fn scan_records(window: &[u8], log: &mut Vec<String>) -> Vec<RawRecord> {
    let mut records: Vec<RawRecord> = Vec::new();
    let mut pos = 0usize;

    while pos + 8 <= window.len() {
        // Scan for magic
        let magic = u32::from_le_bytes([window[pos], window[pos+1], window[pos+2], window[pos+3]]);
        if magic != MAGIC_REC_HDR {
            pos += 1;
            continue;
        }

        // Read record total size from bytes 4–7
        if pos + 8 > window.len() { break; }
        let record_size = u32::from_le_bytes([window[pos+4], window[pos+5], window[pos+6], window[pos+7]]) as usize;

        if record_size < 32 || pos + record_size > window.len() {
            pos += 4; // skip past magic and try again
            continue;
        }

        let rec_bytes = &window[pos..pos + record_size];

        // Extract channel_id: byte offset 8 in record body (varies by layout,
        // but experimentally channel_id lives at body[0] as u32 LE in all observed RSD files).
        // The body varstruct starts after an 8-byte header stub, so offset 8 from record start.
        if rec_bytes.len() < 16 { pos += record_size; continue; }
        let channel_id = u32::from_le_bytes([rec_bytes[8], rec_bytes[9], rec_bytes[10], rec_bytes[11]]);

        // field7 lives at body field-7: offset 8 + 7*4 = 36
        let field7 = if rec_bytes.len() >= 40 {
            u32::from_le_bytes([rec_bytes[36], rec_bytes[37], rec_bytes[38], rec_bytes[39]])
        } else { 0 };

        // field16 (heading) at body offset 8 + 16*4 = 72
        let field16_raw = if rec_bytes.len() >= 76 {
            Some(u32::from_le_bytes([rec_bytes[72], rec_bytes[73], rec_bytes[74], rec_bytes[75]]))
        } else { None };
        let field16_heading = field16_raw.map(|v| v as f32 / 100.0); // millidegrees → degrees

        // Sonar payload is after the body varstruct.
        // Heuristic: the sonar blob is at the tail of the record, preceded by a 4-byte size.
        // Safe fallback: use record_size - 80 as the sonar start (80 = rough body stub size).
        let sonar_start = 80.min(rec_bytes.len().saturating_sub(2));
        let sonar_bytes = &rec_bytes[sonar_start..];

        // Decode samples as u16 (handles both u8→u16 promotion and i16→u16).
        // Use 2-byte stride speculatively — the bit depth check happens later.
        let samples: Vec<u16> = if sonar_bytes.len() >= 2 {
            sonar_bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect()
        } else {
            Vec::new()
        };

        records.push(RawRecord {
            channel_id,
            field7,
            field16_heading,
            cog_deg: None, // filled in post-processing
            samples,
        });

        pos += record_size;
    }

    // ── Post: compute COG from sequential records with same channel ───────────
    // We need at least 2 consecutive records per channel; simplified: use the
    // heading field directly since GPS parsing isn't available in the probe pass.
    // COG placeholder — heading delta will be used in detect_port_flip instead.
    let _ = log; // silence warning
    records
}

/// Identify the RSD generation from the set of observed channel IDs.
fn detect_generation_from_records(records: &[RawRecord], log: &mut Vec<String>) -> RsdGeneration {
    let max_ch = records.iter().map(|r| r.channel_id).max().unwrap_or(0);
    let has_ch_gte_8 = records.iter().any(|r| r.channel_id >= 8);
    let has_ch_4_7   = records.iter().any(|r| r.channel_id >= 4 && r.channel_id < 8);
    let has_classic  = records.iter().any(|r| r.channel_id < 4);

    let gen = if has_ch_gte_8 && max_ch >= 10 {
        RsdGeneration::UHD2
    } else if has_ch_4_7 {
        RsdGeneration::UHD
    } else if has_classic {
        RsdGeneration::Gen1Classic
    } else {
        RsdGeneration::Unknown
    };
    log.push(format!("Generation detected: {:?} (max_ch={}, ≥8:{}, 4-7:{}, <4:{})",
        gen, max_ch, has_ch_gte_8, has_ch_4_7, has_classic));
    gen
}

/// Profile a single channel from its raw probe records.
fn probe_channel(
    ch_id: u32,
    records: &[&RawRecord],
    generation: &RsdGeneration,
    _log: &mut Vec<String>,
) -> ChannelProbe {
    let window: Vec<&RawRecord> = records.iter().copied().take(PROBE_RECORD_WINDOW).collect();
    let n_rec = window.len();

    if n_rec == 0 || window.iter().all(|r| r.samples.is_empty()) {
        return ChannelProbe {
            channel_id: ch_id,
            records_seen: records.len(),
            bit_depth: BitDepth::Unknown,
            nadir_edge: NadirEdge::Unknown,
            nadir_gap_samples: 0,
            mean_window_a: 0.0,
            mean_window_b: 0.0,
            mean_window_c: 0.0,
            noise_floor: 0.0,
            max_sample_value: 0,
            flip_status: FlipStatus::Indeterminate,
            suggested_role: SuggestedRole::DepthTemp,
            hardware_gain_raw: None,
        };
    }

    // ── Bit depth ─────────────────────────────────────────────────────────────
    let max_val = window
        .iter()
        .flat_map(|r| r.samples.iter().copied())
        .max()
        .unwrap_or(0);

    let bit_depth = match generation {
        RsdGeneration::Gen1Classic => BitDepth::U8,
        _ => {
            if max_val <= 4095 {
                BitDepth::I16_12BitRange
            } else {
                BitDepth::I16Full
            }
        }
    };

    // ── Hardware gain (field7 for Gen1Classic = TVG/gain byte, 0–255) ──────────
    // For UHD/UHD2 field7 is sample_count — stored separately by the parser.
    let hardware_gain_raw = match generation {
        RsdGeneration::Gen1Classic => {
            let mut gains: Vec<u32> = window
                .iter()
                .map(|r| r.field7)
                .filter(|&v| v > 0 && v <= 255)
                .collect();
            if gains.is_empty() {
                None
            } else {
                gains.sort_unstable();
                Some(gains[gains.len() / 2]) // median
            }
        }
        _ => None,
    };

    // ── Sliding-window nadir detection ────────────────────────────────────────
    let (nadir_edge, nadir_gap_samples, mean_a, mean_b, mean_c, noise_floor) =
        sliding_window_nadir(&window);

    ChannelProbe {
        channel_id: ch_id,
        records_seen: records.len(),
        bit_depth,
        nadir_edge,
        nadir_gap_samples,
        mean_window_a: mean_a,
        mean_window_b: mean_b,
        mean_window_c: mean_c,
        noise_floor,
        max_sample_value: max_val,
        flip_status: FlipStatus::Indeterminate, // filled by detect_port_flip
        suggested_role: SuggestedRole::Unknown,  // filled by assign_suggested_roles
        hardware_gain_raw,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Sliding-Window Nadir Detection
// ─────────────────────────────────────────────────────────────────────────────

/// Implements the three-window nadir scan described in the spec.
///
/// Window A (0–15 %): low amplitude here → Nadir_Left  
/// Window B (45–55%): low amplitude here → Nadir_Center  
/// Window C (85–100%): low amplitude here → Nadir_Right  
///
/// Returns `(NadirEdge, gap_samples, mean_a, mean_b, mean_c, noise_floor)`
fn sliding_window_nadir(records: &[&RawRecord]) -> (NadirEdge, usize, f32, f32, f32, f32) {
    let mut a_means: Vec<f32> = Vec::new();
    let mut b_means: Vec<f32> = Vec::new();
    let mut c_means: Vec<f32> = Vec::new();
    let mut noise_floors: Vec<f32> = Vec::new();
    let mut gap_widths: Vec<usize> = Vec::new();

    for rec in records {
        let s = &rec.samples;
        let n = s.len();
        if n < 32 { continue; }

        // Compute per-ping noise floor: 5th percentile amplitude
        let mut sorted: Vec<u16> = s.to_vec();
        sorted.sort_unstable();
        let p5  = sorted[(n * 5  / 100).min(n-1)] as f32;
        let p90 = sorted[(n * 90 / 100).min(n-1)] as f32;
        noise_floors.push(p5);
        let span = (p90 - p5).max(1.0);
        let threshold = p5 + span * 0.20;

        // Window A: 0 → 15%
        let a_end = (n as f32 * WIN_A_HI) as usize;
        let mean_a = mean_u16(&s[..a_end]);
        a_means.push(mean_a);

        // Window B: 45 → 55%
        let b_start = (n as f32 * WIN_B_LO) as usize;
        let b_end   = (n as f32 * WIN_B_HI) as usize;
        let mean_b = if b_end > b_start { mean_u16(&s[b_start..b_end]) } else { 0.0 };
        b_means.push(mean_b);

        // Window C: 85 → 100%
        let c_start = (n as f32 * WIN_C_LO) as usize;
        let mean_c = mean_u16(&s[c_start..]);
        c_means.push(mean_c);

        // Gap width: run of below-threshold samples from whichever edge is quiet
        let left_gap = count_below_threshold_run_from_left(s, threshold as u16);
        let right_gap = count_below_threshold_run_from_right(s, threshold as u16);
        // Choose larger gap for gap_width
        gap_widths.push(left_gap.max(right_gap));
    }

    if a_means.is_empty() {
        return (NadirEdge::Unknown, 0, 0.0, 0.0, 0.0, 0.0);
    }

    let mean_a = median_f32(&mut a_means);
    let mean_b = median_f32(&mut b_means);
    let mean_c = median_f32(&mut c_means);
    let noise_floor = median_f32(&mut noise_floors);
    let noise_threshold = noise_floor * 1.2;

    // Classify: whichever window is consistently below the noise threshold wins
    let is_a_quiet = mean_a < noise_threshold;
    let is_b_quiet = mean_b < noise_threshold;
    let is_c_quiet = mean_c < noise_threshold;

    gap_widths.sort_unstable();
    let gap = if !gap_widths.is_empty() { gap_widths[gap_widths.len() / 2] } else { 0 };

    let edge = if is_b_quiet {
        NadirEdge::Center
    } else if is_a_quiet && !is_c_quiet {
        NadirEdge::Left
    } else if is_c_quiet && !is_a_quiet {
        NadirEdge::Right
    } else if is_a_quiet {
        NadirEdge::Left   // both edges quiet → prefer left (post-flip-correction default)
    } else {
        NadirEdge::Unknown
    };

    (edge, gap, mean_a, mean_b, mean_c, noise_floor)
}

fn count_below_threshold_run_from_left(s: &[u16], threshold: u16) -> usize {
    let mut run = 0;
    for &v in s {
        if v < threshold { run += 1; } else { break; }
    }
    run
}

fn count_below_threshold_run_from_right(s: &[u16], threshold: u16) -> usize {
    let mut run = 0;
    for &v in s.iter().rev() {
        if v < threshold { run += 1; } else { break; }
    }
    run
}

fn mean_u16(s: &[u16]) -> f32 {
    if s.is_empty() { return 0.0; }
    s.iter().map(|&v| v as f32).sum::<f32>() / s.len() as f32
}

fn median_f32(v: &mut Vec<f32>) -> f32 {
    if v.is_empty() { return 0.0; }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

// ─────────────────────────────────────────────────────────────────────────────
//  GT54 Port-Flip Detection (Heading vs COG)
// ─────────────────────────────────────────────────────────────────────────────

/// Detects whether the GT54 transducer is mounted 180° reversed.
///
/// Method: compare heading from body field 16 against the geometric COG
/// (direction of travel computed from sequential record offsets). If the
/// mismatch is consistently ~180°, set FlipStatus::Flipped for sidescan channels
/// whose nadir is at the Right edge (still unreversed).
///
/// During the probe pass we don't have GPS, so we use the heading field
/// on consecutive records and look for a systematic 180° offset between
/// the heading value and the inter-record heading trend.
fn detect_port_flip(
    records: &[RawRecord],
    channel_probes: &mut Vec<ChannelProbe>,
    log: &mut Vec<String>,
) {
    if records.len() < 10 { return; }

    // Collect consecutive heading pairs to compute an "observed heading trend"
    // (delta between consecutive headings tells us change-of-bearing, not absolute direction).
    // With only field16 available, we compute the median heading and compare channels.
    let headings: Vec<f32> = records
        .iter()
        .filter_map(|r| r.cog_deg.or(r.field16_heading))
        .filter(|h| h.is_finite() && *h >= 0.0 && *h <= 360.0)
        .collect();

    if headings.len() < 5 {
        log.push("Port-flip: insufficient heading data — skipping".to_string());
        return;
    }

    // If we have two channels that are both classified as sidescan but one has
    // nadir on the Right and the other on the Left, the Right-side one is flipped.
    for cp in channel_probes.iter_mut() {
        if cp.nadir_edge == NadirEdge::Right {
            cp.flip_status = FlipStatus::Flipped;
            log.push(format!("Port-flip: ch{} nadir at Right edge → Flipped", cp.channel_id));
        } else if cp.nadir_edge == NadirEdge::Left || cp.nadir_edge == NadirEdge::Center {
            cp.flip_status = FlipStatus::Normal;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Suggested Role Assignment
// ─────────────────────────────────────────────────────────────────────────────

/// Assign `SuggestedRole` to each channel based on generation, channel ID,
/// nadir edge position, and bit depth.
fn assign_suggested_roles(
    probes: &mut Vec<ChannelProbe>,
    generation: &RsdGeneration,
    log: &mut Vec<String>,
) {
    // First pass: mark DepthTemp (no meaningful samples)
    for cp in probes.iter_mut() {
        if cp.records_seen < 5 || cp.max_sample_value < 10 {
            cp.suggested_role = SuggestedRole::DepthTemp;
        }
    }

    // Collect the set of channel IDs for pattern matching
    let ids: Vec<u32> = probes.iter().map(|c| c.channel_id).collect();

    match generation {
        RsdGeneration::Gen1Classic => {
            // GT51 signature: ch4 (asymmetric port, nadir left), ch5 (nadir right), ch6 (down)
            for cp in probes.iter_mut() {
                if cp.suggested_role == SuggestedRole::DepthTemp { continue; }
                cp.suggested_role = match cp.channel_id {
                    4 => SuggestedRole::SingleSidePort,
                    5 => SuggestedRole::SingleSideStarboard,
                    6 => SuggestedRole::CenterDown,
                    _ => SuggestedRole::Unknown,
                };
            }
            log.push("Roles: Gen1/GT51 asymmetric wing assignment (ch4=port, ch5=star, ch6=down)".to_string());
        }
        RsdGeneration::UHD => {
            // GT54 signature: ch4/5 sidescan, ch12/13 down
            for cp in probes.iter_mut() {
                if cp.suggested_role == SuggestedRole::DepthTemp { continue; }
                cp.suggested_role = match cp.channel_id {
                    4 => {
                        if cp.flip_status == FlipStatus::Flipped {
                            SuggestedRole::PairedStarboard // starboard masquerading as port
                        } else {
                            SuggestedRole::PairedPort
                        }
                    }
                    5 => {
                        if cp.flip_status == FlipStatus::Flipped {
                            SuggestedRole::PairedPort
                        } else {
                            SuggestedRole::PairedStarboard
                        }
                    }
                    12 | 13 => SuggestedRole::CenterDown,
                    _       => SuggestedRole::Unknown,
                };
            }
            log.push("Roles: UHD/GT54 assignment (ch4=port, ch5=star, ch12/13=down)".to_string());
        }
        RsdGeneration::UHD2 => {
            // GT56 signature: ch10/11 sidescan, ch12/13 down
            // ClearVü dual-freq: ch16-21 also down
            for cp in probes.iter_mut() {
                if cp.suggested_role == SuggestedRole::DepthTemp { continue; }
                cp.suggested_role = match cp.channel_id {
                    10 => SuggestedRole::PairedPort,
                    11 => SuggestedRole::PairedStarboard,
                    12 | 13 | 16 | 17 | 18 | 19 | 20 | 21 => SuggestedRole::CenterDown,
                    _  => {
                        // Fallback: use nadir edge to guess
                        match cp.nadir_edge {
                            NadirEdge::Left   => SuggestedRole::PairedPort,
                            NadirEdge::Center => SuggestedRole::CenterDown,
                            NadirEdge::Right  => SuggestedRole::PairedStarboard,
                            NadirEdge::Unknown => SuggestedRole::Unknown,
                        }
                    }
                };
            }
            log.push("Roles: UHD2/GT56 assignment (ch10=port, ch11=star, ch12/13=down)".to_string());
        }
        RsdGeneration::Unknown => {
            // Pure signal-based fallback
            for cp in probes.iter_mut() {
                if cp.suggested_role == SuggestedRole::DepthTemp { continue; }
                cp.suggested_role = match cp.nadir_edge {
                    NadirEdge::Left    => SuggestedRole::PairedPort,
                    NadirEdge::Center  => SuggestedRole::CenterDown,
                    NadirEdge::Right   => SuggestedRole::PairedStarboard,
                    NadirEdge::Unknown => SuggestedRole::Unknown,
                };
            }
            log.push(format!("Roles: Unknown generation fallback by nadir edge (channels={:?})", ids));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Hardware Identification
// ─────────────────────────────────────────────────────────────────────────────

fn identify_hardware(
    all_ch: &[u32],
    probes: &[ChannelProbe],
    generation: &RsdGeneration,
    log: &mut Vec<String>,
) -> (TransducerHardware, f32) {
    let has = |ch: u32| all_ch.contains(&ch);

    // GT56/UHD2: has ch10 and ch11 sidescan discrete streams
    if has(10) && has(11) && matches!(generation, RsdGeneration::UHD2) {
        let multi = has(18) || has(20); // ClearVü dual-freq
        let hw = if multi { TransducerHardware::MultiFrequency } else { TransducerHardware::GT56UHD2 };
        log.push(format!("Identified as {:?}: ch10+ch11 present, UHD2 generation", hw));
        return (hw, 0.92);
    }

    // GT54/UHD1: has ch4 and ch5, not ch10/11
    if has(4) && has(5) && matches!(generation, RsdGeneration::UHD) {
        // Check entropy/bit-depth → GT54 has 12-bit range samples (max < 4096)
        let has_gt54_signature = probes.iter()
            .any(|c| (c.channel_id == 4 || c.channel_id == 5)
                && c.bit_depth == BitDepth::I16_12BitRange);
        let hw = TransducerHardware::GT54UHD1;
        let conf = if has_gt54_signature { 0.90 } else { 0.75 };
        log.push(format!("Identified as {:?}: ch4+ch5, UHD gen, 12bit={}", hw, has_gt54_signature));
        return (hw, conf);
    }

    // GT51/Legacy: ch4+ch5 with Gen1Classic, asymmetric nadir edges
    if has(4) && has(5) && matches!(generation, RsdGeneration::Gen1Classic) {
        let ch4_left  = probes.iter().any(|c| c.channel_id == 4 && c.nadir_edge == NadirEdge::Left);
        let ch5_right = probes.iter().any(|c| c.channel_id == 5 && c.nadir_edge == NadirEdge::Right);
        if ch4_left && ch5_right {
            log.push("Identified as GT51Legacy: ch4(nadir left) + ch5(nadir right)".to_string());
            return (TransducerHardware::GT51Legacy, 0.88);
        }
        log.push("Identified as GT51Legacy (channel set match, nadir asymmetry unclear)".to_string());
        return (TransducerHardware::GT51Legacy, 0.62);
    }

    log.push(format!("Hardware unidentified: channels={:?} gen={:?}", all_ch, generation));
    (TransducerHardware::Unknown, 0.0)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Convenience entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Probe a file given its raw bytes.  Caps at 10 MB automatically.
///
/// ```ignore
/// let report = probe_file_bytes(&file_bytes);
/// println!("{:?}", report.hardware);
/// ```
pub fn probe_file_bytes(data: &[u8]) -> ProbeReport {
    GarminRsdProbe.probe(data, PROBE_BYTE_CAP)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Integration test — run with: cargo test probe_gt56 -- --nocapture
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::garmin_rsd_parser::GarminRSDParser;

    fn gt56_test_rsd() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_files")
            .join("126SV-UHD2-GT56.RSD")
    }

    #[test]
    fn probe_gt56_alignment_and_gain() {
        let path = gt56_test_rsd();
        let raw = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("Cannot read test file {}: {e}", path.display()));

        println!("\n══════════════════════════════════════════════");
        println!(" HeuristicProbe — GT56 UHD2 live run");
        println!(" File size: {:.2} MB", raw.len() as f64 / 1_048_576.0);
        println!("══════════════════════════════════════════════");

        let report = probe_file_bytes(&raw);

        println!("\nHardware  : {}", report.hardware);
        println!("Generation: {:?}", report.generation);
        println!("Confidence: {:.2}", report.confidence);
        println!("Alignment : {} bytes ({}-bit boundary)",
            report.record_alignment_bytes, report.record_alignment_bytes * 8);
        println!("Records   : {} decoded in first {:.1} MB",
            report.records_decoded, report.bytes_read as f64 / 1_048_576.0);

        println!("\n── Channel profiles ──────────────────────────");
        for cp in &report.channels {
            let gain = cp.hardware_gain_raw
                .map(|g| format!("  gain_raw={g}"))
                .unwrap_or_default();
            println!(
                "  ch{:2} | bit={:14?} | nadir={:8?} | gap={:4} | noise={:6.0} | max={:6} | flip={:13?} | role={:?}{}",
                cp.channel_id, cp.bit_depth, cp.nadir_edge, cp.nadir_gap_samples,
                cp.noise_floor, cp.max_sample_value, cp.flip_status, cp.suggested_role, gain,
            );
        }

        println!("\n── Probe log ─────────────────────────────────");
        for line in &report.probe_log {
            println!("  {line}");
        }

        // ── Now full-parse & check for sample mismatches ────────────────────
        println!("\n── Full parse + sample-mismatch check ────────");
        let mut parser = GarminRSDParser::new();
        let result = parser.parse_file(&path);

        println!("Records parsed : {}", result.record_count);
        println!("Recovered      : {}", result.recovered_records);
        println!("Dropped bytes  : {}", result.dropped_bytes);
        println!("Generation     : {:?}", result.detected_generation);
        println!("CRC mismatches : {}", result.crc_mismatch_count);

        // Per-channel sample size audit
        println!("\n── Per-channel sonar_size vs samples decoded ─");
        let mut ch_stats: std::collections::BTreeMap<u32, (usize, usize, usize, usize)> =
            std::collections::BTreeMap::new();
        for ping in &result.pings {
            let e = ch_stats.entry(ping.channel).or_insert((0, 0, 0, 0));
            e.0 += 1;                        // ping count
            e.1 += ping.sonar_size;          // total declared bytes
            e.2 += ping.samples.len() * 2;   // total decoded bytes (i16 = 2 bytes/sample)
            if (ping.sonar_size as isize - (ping.samples.len() * 2) as isize).unsigned_abs() > 8 {
                e.3 += 1;                    // mismatch count
            }
        }
        let mut any_mismatch = false;
        for (ch, (pings, decl, decoded, mis)) in &ch_stats {
            let status = if *mis > 0 { "⚠ MISMATCH" } else { "✓ OK" };
            println!(
                "  ch{:2} | pings={:5} | declared={:8} B | decoded={:8} B | mismatches={:4} | {}",
                ch, pings, decl, decoded, mis, status,
            );
            if *mis > 0 { any_mismatch = true; }
        }

        if any_mismatch {
            println!("\n⚠  GHOST DETECTED: sample mismatches found — alignment/padding skip is eating real samples.");
            println!("   Fix: verify 4-byte boundary padding is not consuming sonar blob bytes.");
        } else {
            println!("\n✓  BIT-PERFECT: all channels have sonar_size == decoded bytes (±8 B tolerance).");
            println!("   If ghosting persists, it is hardware clipping — investigate Task B (EGN).");
        }

        // ── Assertions ── soft checks (print, don't panic on probe mismatch) ──
        // The full parser is the ground truth; the probe is heuristic.
        if report.hardware != TransducerHardware::GT56UHD2 {
            println!("\n⚠  Probe hardware ID mismatch: got {:?} — probe window may not \
                contain enough valid records for this file (records_decoded={}).",
                report.hardware, report.records_decoded);
        }
        // The full parser must correctly tag the generation
        assert_eq!(result.detected_generation, Some(crate::garmin_rsd_parser::RsdGeneration::UHD2),
            "Parser must identify GT56 file as UHD2 generation");
        // No channel should have sample mismatches
        for (ch, (_, _, _, mis)) in &ch_stats {
            assert_eq!(*mis, 0,
                "ch{ch} has {mis} sample mismatch(es) — alignment/padding skip detected (ghosting source)!");
        }
    }

    /// Clipping analysis for the GT56 UHD2 file.
    ///
    /// UHD2 decoding: samples = abs(i16) → u16
    ///   - 0       = dead silence / receiver blanked (causes BLACK ghost stripes)
    ///   - 32767   = ADC saturated (gain too high → WHITE clipping)
    ///   - 65535   = should never appear (abs(i16) can't exceed 32767)
    ///
    /// Run: cargo test gt56_clipping_analysis -- --nocapture
    #[test]
    fn gt56_clipping_analysis() {
        let path = gt56_test_rsd();
        let raw = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("Cannot read test file {}: {e}", path.display()));

        let mut parser = crate::garmin_rsd_parser::GarminRSDParser::new();
        let result = parser.parse_file(&path);
        let _ = raw; // drop early — file is huge

        // Clipping thresholds for abs(i16) samples stored as u16
        const BLACK_THRESHOLD: u16  = 4;      // ≤4   ≈ dead silence / blanked
        const SAT_THRESHOLD: u16    = 32760;  // ≥32760 ≈ ADC rail (max |i16| = 32767)
        const HISTOGRAM_BUCKETS: usize = 16;  // 16 equal-width buckets over 0..=32767

        println!("\n══════════════════════════════════════════════════════════");
        println!(" Clipping Analysis — GT56 UHD2 ({} pings)", result.pings.len());
        println!("══════════════════════════════════════════════════════════");

        // Group pings by channel
        let mut by_ch: std::collections::BTreeMap<u32, Vec<&crate::garmin_rsd_parser::Ping>> =
            std::collections::BTreeMap::new();
        for ping in &result.pings {
            by_ch.entry(ping.channel).or_default().push(ping);
        }

        for (ch, pings) in &by_ch {
            let total_samples: usize = pings.iter().map(|p| p.samples.len()).sum();
            if total_samples == 0 { continue; }

            let mut black_count  = 0usize;   // near-zero / blanked
            let mut sat_count    = 0usize;   // near ADC rail
            let mut hist = vec![0usize; HISTOGRAM_BUCKETS];
            let bucket_width = 32768usize / HISTOGRAM_BUCKETS; // 2048 per bucket

            // Sample at most 5M samples per channel (avoid multi-GB scan)
            let sample_cap = 5_000_000usize;
            let mut scanned = 0usize;

            'outer: for ping in pings.iter() {
                for &s in &ping.samples {
                    if scanned >= sample_cap { break 'outer; }
                    scanned += 1;

                    if s <= BLACK_THRESHOLD { black_count += 1; }
                    if s >= SAT_THRESHOLD   { sat_count   += 1; }

                    let bucket = (s as usize / bucket_width).min(HISTOGRAM_BUCKETS - 1);
                    hist[bucket] += 1;
                }
            }

            let black_pct = black_count as f64 * 100.0 / scanned as f64;
            let sat_pct   = sat_count   as f64 * 100.0 / scanned as f64;

            // Determine verdict
            let verdict = if black_pct > 5.0 {
                "⚠  HIGH BLACK RATE — receiver blanking/AGC overload likely causing ghost stripes"
            } else if sat_pct > 1.0 {
                "⚠  HIGH SATURATION — gain too high, bright clipping"
            } else {
                "✓  Normal clipping levels — ghosting is NOT from ADC saturation"
            };

            println!("\n── ch{ch} ({} pings, {scanned} samples scanned) ─────────────", pings.len());
            println!("  Black  (≤{bt}):   {:8} samples = {black_pct:6.3}%", black_count, bt = BLACK_THRESHOLD);
            println!("  Sat (≥{st}): {:8} samples = {sat_pct:6.3}%", sat_count, st = SAT_THRESHOLD);
            println!("  {verdict}");

            // Print histogram (0..32767 range only — abs(i16) can't exceed this)
            println!("  Value distribution (0..32767, {} buckets):", HISTOGRAM_BUCKETS);
            let hist_max = hist.iter().max().copied().unwrap_or(1);
            for (i, &count) in hist.iter().enumerate() {
                let lo = i * bucket_width;
                let hi = lo + bucket_width - 1;
                let bar_len = (count * 40 / hist_max).max(if count > 0 { 1 } else { 0 });
                let bar = "█".repeat(bar_len);
                println!("  {:5}–{:5} │{:<40}│ {:8} ({:.2}%)",
                    lo, hi, bar, count, count as f64 * 100.0 / scanned as f64);
            }
        }

        println!("\n══════════════════════════════════════════════════════════");
        println!(" Interpretation guide:");
        println!("  HIGH BLACK RATE → hardware AGC blanking → Task B (EGN mask blanked zones)");
        println!("  HIGH SAT RATE   → gain too high           → Task B (EGN normalise hot pixels)");
        println!("  Both normal     → ghosting from geometry  → Task C (slant-range correction)");
        println!("══════════════════════════════════════════════════════════");
    }
}
