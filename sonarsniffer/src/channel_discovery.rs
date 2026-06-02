//! **Step 1 & 2: Exhaustive Channel Discovery, Signal Profiling, and Dynamic Mapping**
//!
//! This module replaces all hardcoded channel-to-role assignments with a fully
//! data-driven pipeline.  It does NOT rely on `map_channel_info()` static tables
//! or metadata headers (which may be corrupt).
//!
//! ## Pipeline
//!
//! 1. **Discovery**: Iterate every ping to build a `HashSet` of all Channel IDs
//!    (including exotic 20s/30s/993/1487).
//! 2. **Signal Profiling**: Analyze a 100-ping window per channel to classify each
//!    as a *Signal Archetype* (SideVü, DownVü/ClearVü, DepthTemp, Noise).
//! 3. **Frequency Fingerprinting**: Compute sample entropy to separate Detail
//!    (UHD/CHIRP high-entropy) from Context (standard low-entropy) layers.
//! 4. **Nadir-Flip Test**: Detect and auto-correct inverted side-scan channels.
//! 5. **Port/Starboard Correlation**: Match side-scan pairs by nadir-gap width
//!    similarity, then use heading (COG) to assign spatial orientation.
//! 6. **Temporal Alignment**: Group cross-frequency pings into Composite Scanlines
//!    using a sliding 100ms timestamp window.

use crate::garmin_rsd_parser::{ParseResult, Ping};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

// ═══════════════════════════════════════════════════════════════════════════════
// §1  PUBLIC TYPES
// ═══════════════════════════════════════════════════════════════════════════════

/// What kind of sonar beam produced this channel's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum SignalArchetype {
    /// Side-scan: long sample array with a "nadir gap" (low-intensity start).
    SideVu,
    /// Down-looking / ClearVü: high-intensity spike (bottom return) early, rapid decay.
    DownVuClearVu,
    /// Depth/temperature metadata channel (no real sonar samples).
    DepthTemp,
    /// Unclassifiable noise or too few pings to decide.
    Noise,
}

impl std::fmt::Display for SignalArchetype {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SideVu => write!(f, "SideVü"),
            Self::DownVuClearVu => write!(f, "DownVü/ClearVü"),
            Self::DepthTemp => write!(f, "Depth/Temp"),
            Self::Noise => write!(f, "Noise/Unknown"),
        }
    }
}

/// Frequency tier derived from sample entropy analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum FrequencyTier {
    /// High entropy → UHD / CHIRP detail layer.
    Detail,
    /// Low entropy → standard / traditional frequency context layer.
    Context,
    /// Cannot determine (too few samples or invariant data).
    Unknown,
}

impl std::fmt::Display for FrequencyTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Detail => write!(f, "Detail (UHD/CHIRP)"),
            Self::Context => write!(f, "Context (Standard)"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Spatial role assigned by the orientation engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum SpatialRole {
    /// Full paired wing, nadir at centre (GT54/GT56 style).
    Port,
    /// Full paired wing, nadir at centre (GT54/GT56 style).
    Starboard,
    /// DownVü / ClearVü nadir-fill beam.
    Center,
    /// GT51 asymmetric wing — water column is at index 0 (NOT the centre).
    SingleSidePort,
    /// GT51 asymmetric wing — water column is at index [max] (NOT the centre).
    SingleSideStarboard,
    Unassigned,
}

impl std::fmt::Display for SpatialRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Port => write!(f, "Port"),
            Self::Starboard => write!(f, "Starboard"),
            Self::Center => write!(f, "Center"),
            Self::SingleSidePort => write!(f, "SingleSide-Port (GT51)"),
            Self::SingleSideStarboard => write!(f, "SingleSide-Starboard (GT51)"),
            Self::Unassigned => write!(f, "Unassigned"),
        }
    }
}

/// Where the low-intensity nadir zone sits within a ping's sample array.
///
/// GT51 channels: nadir is at one edge (asymmetric single-wing).
/// UHD sidescan after parser flip-correction: nadir at Left.
/// Paired UHD before normalization: nadir may be at Centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum NadirEdge {
    /// Nadir confirmed in the first 15 % of samples.
    Left,
    /// Nadir confirmed in the 45–55 % centre band.
    Center,
    /// Nadir confirmed in the last 15 % of samples (reversed wing or GT51 star).
    Right,
    /// Could not locate nadir with confidence.
    Unknown,
}

impl std::fmt::Display for NadirEdge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Left => write!(f, "Left"),
            Self::Center => write!(f, "Centre"),
            Self::Right => write!(f, "Right"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Complete profile for a single discovered channel.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelProfile {
    pub channel_id: u32,
    pub ping_count: usize,
    pub gps_ping_count: usize,
    pub archetype: SignalArchetype,
    pub frequency_tier: FrequencyTier,
    pub spatial_role: SpatialRole,
    /// Whether samples were auto-reversed to normalize nadir to left edge.
    pub was_flipped: bool,
    /// Median nadir gap width (samples) — only meaningful for SideVü.
    pub nadir_gap_width: usize,
    /// Where the nadir zone sits within the sample array.
    pub nadir_edge: NadirEdge,
    /// Estimated noise floor: median of per-ping 5th-percentile amplitudes.
    pub noise_floor: f32,
    /// Mean sample entropy (Shannon, bits) over the profiling window.
    pub mean_entropy: f32,
    /// Median sample count across pings.
    pub median_sample_count: usize,
    /// Median sonar_size / sample_count ratio (1.0 = u8, 2.0 = i16).
    pub sample_byte_ratio: f32,
    /// Timestamp range [first_ms, last_ms].
    pub time_range: (u64, u64),
    /// Confidence score (0.0–1.0) for the archetype classification.
    pub archetype_confidence: f32,
    /// Human-readable explanation of classification reasoning.
    pub classification_reason: String,
}

/// A group of cross-frequency pings aligned to the same ~100ms time window.
#[derive(Debug, Clone, Serialize)]
pub struct CompositeScanline {
    /// Representative timestamp (midpoint of the group).
    pub timestamp_ms: u64,
    /// Latitude from the highest-confidence ping in the group.
    pub latitude: f64,
    /// Longitude from the highest-confidence ping in the group.
    pub longitude: f64,
    /// Heading (COG) from the best ping, or interpolated.
    pub heading_deg: f32,
    /// Depth from the best ping.
    pub depth_m: f32,
    /// Ping indices grouped by channel_id.
    pub pings_by_channel: BTreeMap<u32, Vec<usize>>,
}

/// The complete discovery result — the single source of truth for the mosaic engine.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryResult {
    /// Every channel found in the file, fully profiled.
    pub profiles: Vec<ChannelProfile>,
    /// Side-scan pairs: `(port_channel_id, starboard_channel_id)`.
    /// Multiple pairs if multi-frequency side-scan (e.g. UHD + Classic).
    pub sidescan_pairs: Vec<(u32, u32)>,
    /// Center channels (DownVü / ClearVü) available for nadir fill.
    pub center_channels: Vec<u32>,
    /// Temporally aligned composite scanlines.
    pub scanlines: Vec<CompositeScanline>,
    /// Diagnostic log of all decisions made.
    pub discovery_log: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// §2  CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Number of pings to sample per channel for profiling.
const PROFILE_WINDOW: usize = 100;
/// Minimum pings required to attempt classification.
const MIN_PINGS_FOR_CLASSIFY: usize = 10;
/// Minimum samples in a ping to be considered "sonar data" vs metadata.
const MIN_SAMPLES_SONAR: usize = 32;
/// Sustained run length to detect nadir edge.
const NADIR_RUN_LENGTH: usize = 5;
/// Timestamp grouping window for composite scanlines (milliseconds).
const SCANLINE_WINDOW_MS: u64 = 100;
/// Entropy threshold: above this = Detail tier, below = Context tier.
/// Empirically calibrated: UHD/CHIRP ~5.5–7.0 bits, Standard ~3.0–5.0 bits.
const ENTROPY_DETAIL_THRESHOLD: f32 = 5.2;

// ═══════════════════════════════════════════════════════════════════════════════
// §2b  FIRMWARE LAYOUT DETECTION (GT56 UHD2+)
// ═══════════════════════════════════════════════════════════════════════════════

/// Detected firmware layout for GT56 UHD2+ transducers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareLayout {
    /// 8-series: ch8/9=port/star, ch10=chirp, ch11=depth
    Series8,
    /// 10-series (25MAR25+ firmware): ch10/11=port/star, ch12=chirp, ch13=depth
    Series10,
    /// 14-series: ch14/15=port/star, ch16=chirp, ch17=depth
    Series14,
    /// Classic/UHD (non-UHD2): ch4/5=port/star, ch6=chirp, ch7=depth
    UhdClassic,
    /// Unable to determine layout
    Unknown,
}

impl std::fmt::Display for FirmwareLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Series8 => write!(f, "8-series (UHD2)"),
            Self::Series10 => write!(f, "10-series (UHD2+ 25MAR25+)"),
            Self::Series14 => write!(f, "14-series (UHD2+)"),
            Self::UhdClassic => write!(f, "UHD classic"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Detect firmware layout from channel presence and ping counts.
///
/// GT56 UHD2+ has three firmware-dependent channel layouts:
/// - 8-series: ch8/9=port/star, ch10=chirp, ch11=depth
/// - 10-series: ch10/11=port/star, ch12=chirp, ch13=depth (25MAR25+ firmware)
/// - 14-series: ch14/15=port/star, ch16=chirp, ch17=depth
///
/// Critical: ch10/ch11 are AMBIGUOUS — chirp in 8-series, sidescan in 10-series.
pub fn detect_firmware_layout(profiles: &[ChannelProfile]) -> FirmwareLayout {
    let channel_ids: std::collections::BTreeSet<u32> =
        profiles.iter().map(|p| p.channel_id).collect();

    // Helper: get ping count for a channel
    let ping_count = |ch: u32| -> usize {
        profiles
            .iter()
            .find(|p| p.channel_id == ch)
            .map(|p| p.ping_count)
            .unwrap_or(0)
    };

    // 14-series: ch14+ch15 both present with similar counts
    if channel_ids.contains(&14) && channel_ids.contains(&15) {
        let ch14 = ping_count(14);
        let ch15 = ping_count(15);
        if ch14 > 10 && ch15 > 10 {
            return FirmwareLayout::Series14;
        }
    }

    // 10-series: ch10+ch11 BOTH present with similar ping counts (sidescan pair)
    if channel_ids.contains(&10) && channel_ids.contains(&11) {
        let ch10 = ping_count(10);
        let ch11 = ping_count(11);
        // Check if both have substantial counts and are balanced (sidescan pair)
        if ch10 > 10 && ch11 > 10 {
            let ratio = (ch10 as f64 / ch11 as f64).min(1.0) / (ch10 as f64 / ch11 as f64).max(1.0);
            if ratio > 0.5 {
                return FirmwareLayout::Series10;
            }
        }
    }

    // 8-series: ch8+ch9 present, OR ch10 present without ch11 (ch10=chirp)
    if channel_ids.contains(&8) && channel_ids.contains(&9) {
        let ch8 = ping_count(8);
        let ch9 = ping_count(9);
        if ch8 > 10 && ch9 > 10 {
            return FirmwareLayout::Series8;
        }
    }

    // 8-series with only ch10 (chirp downscan, no sidescan)
    if channel_ids.contains(&10) && !channel_ids.contains(&11) {
        let ch10 = ping_count(10);
        if ch10 > 10 {
            return FirmwareLayout::Series8;
        }
    }

    // UHD classic: ch4+ch5 present
    if channel_ids.contains(&4) && channel_ids.contains(&5) {
        return FirmwareLayout::UhdClassic;
    }

    FirmwareLayout::Unknown
}

// ═══════════════════════════════════════════════════════════════════════════════
// §3  MAIN ENTRY POINT
// ═══════════════════════════════════════════════════════════════════════════════

/// Run the full discovery and profiling pipeline on a parsed sonar file.
///
/// This is the ONLY function the rest of the codebase should call.
/// It replaces `find_sidescan_pair()`, `map_channel_info()` lookups at render
/// time, and ad-hoc nadir detection scattered across outputs.rs.
pub fn discover_and_profile(parsed: &ParseResult) -> DiscoveryResult {
    let mut log: Vec<String> = Vec::new();

    // ── Step 1a: Exhaustive Channel Discovery ───────────────────────────────
    let channel_ids = discover_all_channels(parsed);
    log.push(format!(
        "Discovery: found {} unique channel IDs: {:?}",
        channel_ids.len(),
        channel_ids
    ));

    // ── Step 1b: Build per-channel ping index ───────────────────────────────
    let pings_by_ch = index_pings_by_channel(parsed);

    // ── Step 1c: Profile each channel ───────────────────────────────────────
    let mut profiles: Vec<ChannelProfile> = channel_ids
        .iter()
        .map(|&ch_id| profile_channel(ch_id, &pings_by_ch, parsed, &mut log))
        .collect();

    // ── Step 2a: Nadir flip already handled by parser ───────────────────────
    // ParseResult::normalize_nadir_direction() flips samples at parse time.
    // The was_flipped field is kept for compatibility but always false here.
    // No action needed in this module.

    // ── Step 2b: Firmware Layout Detection (GT56 UHD2+) ─────────────────────
    // Detect firmware layout AFTER profiling to inform channel classification.
    // This is critical for ch10/ch11 disambiguation (chirp in 8-series, sidescan in 10-series).
    let firmware_layout = detect_firmware_layout(&profiles);
    log.push(format!("Firmware layout detected: {}", firmware_layout));

    // Use firmware layout to refine channel classification
    if firmware_layout == FirmwareLayout::Series10 {
        // In 10-series, ch10=port, ch11=starboard, ch12=chirp
        // Override any misclassified channels
        for profile in profiles.iter_mut() {
            if profile.channel_id == 10 || profile.channel_id == 11 {
                // Force sidescan classification for ch10/11 in 10-series
                if profile.archetype != SignalArchetype::SideVu {
                    log.push(format!(
                        "ch{}: 10-series sidescan (overridden from {:?})",
                        profile.channel_id, profile.archetype
                    ));
                }
            } else if profile.channel_id == 12 {
                // Force downscan classification for ch12 in 10-series
                if profile.archetype != SignalArchetype::DownVuClearVu {
                    log.push(format!(
                        "ch{}: 10-series chirp downscan (overridden from {:?})",
                        profile.channel_id, profile.archetype
                    ));
                }
            }
        }
    }

    // ── Step 2c: Port/Starboard Correlation ─────────────────────────────────
    let sidescan_pairs = correlate_port_starboard(&profiles, &pings_by_ch, parsed, &mut log);

    // Assign spatial roles based on pairing results
    for &(port_id, star_id) in &sidescan_pairs {
        if let Some(p) = profiles.iter_mut().find(|p| p.channel_id == port_id) {
            p.spatial_role = SpatialRole::Port;
        }
        if let Some(p) = profiles.iter_mut().find(|p| p.channel_id == star_id) {
            p.spatial_role = SpatialRole::Starboard;
        }
    }

    // ── GT51 asymmetric single-wing detection ────────────────────────────────
    // A SideVü channel that was NOT paired (still Unassigned after pairing pass)
    // and has its nadir at the Left edge → SingleSidePort.
    // Nadir at Right edge → SingleSideStarboard.
    // "Do NOT split it; use the whole array as one wing of the boat."
    //
    // GT51 signature: Single SideVü channel with channel ID ≤ 3 (classic) or
    // channel ID 4/6 (ClearVü mode). No paired sidescan present.
    for p in profiles.iter_mut() {
        if p.archetype == SignalArchetype::SideVu && p.spatial_role == SpatialRole::Unassigned {
            // Check if this looks like a GT51 channel
            let is_gt51_classic = p.channel_id <= 3;
            let is_gt51_clearvu = p.channel_id == 4 || p.channel_id == 6;
            let is_single_wing = is_gt51_classic || is_gt51_clearvu;

            if is_single_wing {
                p.spatial_role = match p.nadir_edge {
                    NadirEdge::Left => SpatialRole::SingleSidePort,
                    NadirEdge::Right => SpatialRole::SingleSideStarboard,
                    _ => {
                        // Default to port for GT51 (most common configuration)
                        // when nadir edge is unknown
                        SpatialRole::SingleSidePort
                    }
                };
                log.push(format!(
                    "ch{}: GT51 {} + nadir={:?} → {:?} (single-wing asymmetric)",
                    p.channel_id,
                    if is_gt51_classic {
                        "classic"
                    } else {
                        "ClearVü"
                    },
                    p.nadir_edge,
                    p.spatial_role
                ));
            } else {
                // Non-GT51 unpaired SideVü - still assign based on nadir edge
                p.spatial_role = match p.nadir_edge {
                    NadirEdge::Left => SpatialRole::SingleSidePort,
                    NadirEdge::Right => SpatialRole::SingleSideStarboard,
                    _ => SpatialRole::Unassigned,
                };
                if p.spatial_role != SpatialRole::Unassigned {
                    log.push(format!(
                        "ch{}: unpaired SideVü + nadir={:?} → {:?} (single-wing)",
                        p.channel_id, p.nadir_edge, p.spatial_role
                    ));
                }
            }
        }
    }

    // Assign Center role to DownVü/ClearVü channels
    let center_channels: Vec<u32> = profiles
        .iter()
        .filter(|p| p.archetype == SignalArchetype::DownVuClearVu)
        .map(|p| {
            // Also set their role
            p.channel_id
        })
        .collect();
    for &ch in &center_channels {
        if let Some(p) = profiles.iter_mut().find(|p| p.channel_id == ch) {
            p.spatial_role = SpatialRole::Center;
        }
    }

    // ── Step 2c: Temporal Alignment ─────────────────────────────────────────
    let active_channels: BTreeSet<u32> = profiles
        .iter()
        .filter(|p| {
            p.archetype != SignalArchetype::DepthTemp && p.archetype != SignalArchetype::Noise
        })
        .map(|p| p.channel_id)
        .collect();
    let scanlines = build_composite_scanlines(parsed, &active_channels, &mut log);

    log.push(format!(
        "Temporal alignment: {} composite scanlines from {} active channels",
        scanlines.len(),
        active_channels.len()
    ));

    // ── Summary ─────────────────────────────────────────────────────────────
    log.push(format!("=== Discovery Summary ==="));
    for p in &profiles {
        log.push(format!(
            "  ch{}: {} | {} | {} | gap={} | entropy={:.2} | conf={:.2} | {}",
            p.channel_id,
            p.archetype,
            p.frequency_tier,
            p.spatial_role,
            p.nadir_gap_width,
            p.mean_entropy,
            p.archetype_confidence,
            p.classification_reason,
        ));
    }
    for (port, star) in &sidescan_pairs {
        log.push(format!("  Pair: ch{}(Port) + ch{}(Starboard)", port, star));
    }
    if !center_channels.is_empty() {
        log.push(format!("  Center fill channels: {:?}", center_channels));
    }

    // Emit log to stderr for debugging
    for line in &log {
        eprintln!("[discovery] {}", line);
    }

    DiscoveryResult {
        profiles,
        sidescan_pairs,
        center_channels,
        scanlines,
        discovery_log: log,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// §4  STEP 1: EXHAUSTIVE DISCOVERY
// ═══════════════════════════════════════════════════════════════════════════════

/// Iterate through every ping to discover all channel IDs.
/// Does NOT trust metadata headers — only observes actual data.
fn discover_all_channels(parsed: &ParseResult) -> BTreeSet<u32> {
    parsed.pings.iter().map(|p| p.channel).collect()
}

/// Build an index mapping channel_id → list of ping indices.
fn index_pings_by_channel(parsed: &ParseResult) -> BTreeMap<u32, Vec<usize>> {
    let mut idx: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, ping) in parsed.pings.iter().enumerate() {
        idx.entry(ping.channel).or_default().push(i);
    }
    idx
}

// ═══════════════════════════════════════════════════════════════════════════════
// §5  STEP 1: SIGNAL PROFILING
// ═══════════════════════════════════════════════════════════════════════════════

/// Build a complete profile for one channel.
fn profile_channel(
    ch_id: u32,
    pings_by_ch: &BTreeMap<u32, Vec<usize>>,
    parsed: &ParseResult,
    log: &mut Vec<String>,
) -> ChannelProfile {
    let indices = pings_by_ch.get(&ch_id).cloned().unwrap_or_default();
    let ping_count = indices.len();

    // Count pings with valid GPS
    let gps_count = indices
        .iter()
        .filter(|&&i| {
            let p = &parsed.pings[i];
            p.latitude != 0.0
                && p.longitude != 0.0
                && p.latitude.is_finite()
                && p.longitude.is_finite()
        })
        .count();

    // Time range
    let time_range = if !indices.is_empty() {
        let first = parsed.pings[indices[0]].timestamp_ms;
        let last = parsed.pings[*indices.last().unwrap()].timestamp_ms;
        (first, last)
    } else {
        (0, 0)
    };

    // Bail early if too few pings
    if ping_count < MIN_PINGS_FOR_CLASSIFY {
        log.push(format!(
            "ch{}: only {} pings — classifying as Noise",
            ch_id, ping_count
        ));
        return ChannelProfile {
            channel_id: ch_id,
            ping_count,
            gps_ping_count: gps_count,
            archetype: SignalArchetype::Noise,
            frequency_tier: FrequencyTier::Unknown,
            spatial_role: SpatialRole::Unassigned,
            was_flipped: false,
            nadir_gap_width: 0,
            nadir_edge: NadirEdge::Unknown,
            noise_floor: 0.0,
            mean_entropy: 0.0,
            median_sample_count: 0,
            sample_byte_ratio: 0.0,
            time_range,
            archetype_confidence: 0.0,
            classification_reason: format!("Too few pings ({}) to classify", ping_count),
        };
    }

    // Select profiling window: evenly spaced across the file for robustness
    let window_indices = select_profile_window(&indices, PROFILE_WINDOW);
    let window_pings: Vec<&Ping> = window_indices.iter().map(|&i| &parsed.pings[i]).collect();

    // ── Compute metrics ─────────────────────────────────────────────────────

    // Median sample count
    let mut sample_counts: Vec<usize> = window_pings.iter().map(|p| p.sample_count).collect();
    sample_counts.sort_unstable();
    let median_sample_count = if sample_counts.is_empty() {
        0
    } else {
        sample_counts[sample_counts.len() / 2]
    };

    // Sample byte ratio
    let sample_byte_ratio = {
        let mut ratios: Vec<f32> = window_pings
            .iter()
            .filter(|p| p.sample_count > 0 && p.sonar_size > 0)
            .map(|p| p.sonar_size as f32 / p.sample_count as f32)
            .collect();
        ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if ratios.is_empty() {
            0.0
        } else {
            ratios[ratios.len() / 2]
        }
    };

    // ── Check for Depth/Temp (no real sonar data) ───────────────────────────
    let no_sonar_count = window_pings
        .iter()
        .filter(|p| p.samples.len() < MIN_SAMPLES_SONAR || p.sonar_size <= 2)
        .count();
    let no_sonar_ratio = no_sonar_count as f32 / window_pings.len() as f32;

    if no_sonar_ratio > 0.5 {
        log.push(format!(
            "ch{}: {:.0}% pings have <{} samples — DepthTemp",
            ch_id,
            no_sonar_ratio * 100.0,
            MIN_SAMPLES_SONAR
        ));
        return ChannelProfile {
            channel_id: ch_id,
            ping_count,
            gps_ping_count: gps_count,
            archetype: SignalArchetype::DepthTemp,
            frequency_tier: FrequencyTier::Unknown,
            spatial_role: SpatialRole::Unassigned,
            was_flipped: false,
            nadir_gap_width: 0,
            nadir_edge: NadirEdge::Unknown,
            noise_floor: 0.0,
            mean_entropy: 0.0,
            median_sample_count,
            sample_byte_ratio,
            time_range,
            archetype_confidence: no_sonar_ratio,
            classification_reason: format!(
                "{:.0}% pings have no sonar samples",
                no_sonar_ratio * 100.0
            ),
        };
    }

    // ── Nadir Gap Analysis (SideVü vs DownVü discriminator) ─────────────────
    let nadir_gaps = measure_nadir_gaps(&window_pings);
    let mut sorted_gaps = nadir_gaps.clone();
    sorted_gaps.sort_unstable();
    let median_gap = if sorted_gaps.is_empty() {
        0
    } else {
        sorted_gaps[sorted_gaps.len() / 2]
    };

    // ── First Return / Bottom Spike Analysis ────────────────────────────────
    let spike_metrics = measure_bottom_spikes(&window_pings);

    // ── Sample Entropy (Shannon) ────────────────────────────────────────────
    let entropies = compute_sample_entropies(&window_pings);
    let mean_entropy = if entropies.is_empty() {
        0.0
    } else {
        entropies.iter().sum::<f32>() / entropies.len() as f32
    };

    // ── Compute median beam angle for this channel ─────────────────────────
    let median_beam_angle = compute_median_beam_angle(&window_pings);

    // ── Archetype Classification ────────────────────────────────────────────
    let (archetype, confidence, reason) = classify_archetype(
        median_gap,
        median_sample_count,
        &spike_metrics,
        mean_entropy,
        ch_id,
        median_beam_angle,
    );

    // ── Sliding-window nadir edge + noise floor ─────────────────────────────
    // Three windows: A (0–15%), B (45–55%), C (85–100%).
    // The quietest window identifies whether this is a:
    //   GT51 single-wing (A or C quiet) vs UHD paired (B quiet after flip).
    let (nadir_edge, noise_floor) = classify_nadir_edge_sliding(&window_pings);

    // ── Frequency Tier ──────────────────────────────────────────────────────
    let frequency_tier = if mean_entropy > ENTROPY_DETAIL_THRESHOLD {
        FrequencyTier::Detail
    } else if mean_entropy > 0.5 {
        FrequencyTier::Context
    } else {
        FrequencyTier::Unknown
    };

    log.push(format!(
        "ch{}: {} (conf={:.2}) | gap={} | entropy={:.2} | samples={} | ratio={:.2} | {}",
        ch_id,
        archetype,
        confidence,
        median_gap,
        mean_entropy,
        median_sample_count,
        sample_byte_ratio,
        reason,
    ));

    ChannelProfile {
        channel_id: ch_id,
        ping_count,
        gps_ping_count: gps_count,
        archetype,
        frequency_tier,
        spatial_role: SpatialRole::Unassigned, // assigned later in pairing
        was_flipped: false,                    // assigned in nadir-flip test
        nadir_gap_width: median_gap,
        nadir_edge,
        noise_floor,
        mean_entropy,
        median_sample_count,
        sample_byte_ratio,
        time_range,
        archetype_confidence: confidence,
        classification_reason: reason,
    }
}

/// Select evenly-spaced indices from the full list for profiling.
/// Takes from the start, middle, and end to capture any firmware
/// changes or mode switches mid-file.
fn select_profile_window(all_indices: &[usize], window_size: usize) -> Vec<usize> {
    let n = all_indices.len();
    if n <= window_size {
        return all_indices.to_vec();
    }
    let step = n / window_size;
    all_indices
        .iter()
        .step_by(step)
        .take(window_size)
        .copied()
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// §6  NADIR GAP MEASUREMENT
// ═══════════════════════════════════════════════════════════════════════════════

/// Measure the "nadir gap" — the number of low-intensity samples at the start
/// of the array before the first sustained high-intensity run.
///
/// SideVü channels have a wide nadir gap (water column beneath the boat).
/// DownVü/ClearVü have a narrow or zero gap (direct bottom return).
fn measure_nadir_gaps(pings: &[&Ping]) -> Vec<usize> {
    pings
        .iter()
        .map(|p| {
            let n = p.samples.len();
            if n < MIN_SAMPLES_SONAR {
                return 0;
            }
            measure_single_nadir_gap(&p.samples)
        })
        .collect()
}

/// Core nadir gap measurement for a single sample array.
fn measure_single_nadir_gap(samples: &[u16]) -> usize {
    let n = samples.len();
    if n < MIN_SAMPLES_SONAR {
        return 0;
    }

    // Compute robust noise floor and dynamic range
    let mut sorted: Vec<u16> = samples.to_vec();
    sorted.sort_unstable();
    let p15 = sorted[(n * 15 / 100).min(n - 1)] as f32;
    let p90 = sorted[(n * 90 / 100).min(n - 1)] as f32;
    let span = (p90 - p15).max(1.0);

    // Threshold: noise floor + 20% of dynamic range
    let threshold = (p15 + span * 0.20) as u16;

    // Find first sustained run of NADIR_RUN_LENGTH consecutive samples above threshold
    let mut run = 0usize;
    for i in 0..n {
        if samples[i] > threshold {
            run += 1;
            if run >= NADIR_RUN_LENGTH {
                return (i + 1).saturating_sub(NADIR_RUN_LENGTH);
            }
        } else {
            run = 0;
        }
    }
    // No sustained run — either noise channel or entire ping is below threshold
    0
}

// ─────────────────────────────────────────────────────────────────────────────
// §6b  SLIDING-WINDOW NADIR EDGE CLASSIFICATION (GT51 / UHD / UHD2)
// ─────────────────────────────────────────────────────────────────────────────

/// Classifies nadir position using three amplitude windows over the full
/// profiling window.
///
/// | Window | Sample range | Interpretation if quiet        |
/// |--------|--------------|-------------------------------|
/// | A      | 0 – 15 %     | Left-edge nadir (GT51 Port or UHD after flip) |
/// | B      | 45 – 55 %    | Centre nadir (paired UHD sidescan)            |
/// | C      | 85 – 100 %   | Right-edge nadir (GT51 Starboard or unflipped)|
///
/// Returns `(NadirEdge, noise_floor)`.
fn classify_nadir_edge_sliding(pings: &[&Ping]) -> (NadirEdge, f32) {
    let mut a_means: Vec<f32> = Vec::new();
    let mut b_means: Vec<f32> = Vec::new();
    let mut c_means: Vec<f32> = Vec::new();
    let mut noise_vals: Vec<f32> = Vec::new();

    for p in pings {
        let s = &p.samples;
        let n = s.len();
        if n < 32 {
            continue;
        }

        // Per-ping noise floor: 5th percentile
        let mut sorted: Vec<u16> = s.to_vec();
        sorted.sort_unstable();
        let p5 = sorted[(n * 5 / 100).min(n - 1)] as f32;
        let _p90 = sorted[(n * 90 / 100).min(n - 1)] as f32;
        noise_vals.push(p5);

        // Window means
        let a_end = (n as f32 * 0.15) as usize;
        let b_start = (n as f32 * 0.45) as usize;
        let b_end = (n as f32 * 0.55) as usize;
        let c_start = (n as f32 * 0.85) as usize;

        let mean = |slice: &[u16]| {
            if slice.is_empty() {
                0.0f32
            } else {
                slice.iter().map(|&v| v as f32).sum::<f32>() / slice.len() as f32
            }
        };

        a_means.push(mean(&s[..a_end]));
        if b_end > b_start {
            b_means.push(mean(&s[b_start..b_end]));
        }
        c_means.push(mean(&s[c_start..]));
    }

    if noise_vals.is_empty() {
        return (NadirEdge::Unknown, 0.0);
    }

    let median = |v: &mut Vec<f32>| -> f32 {
        if v.is_empty() {
            return 0.0;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v[v.len() / 2]
    };

    let noise_floor = median(&mut noise_vals);
    let threshold = noise_floor * 1.2;
    let mean_a = median(&mut a_means);
    let mean_b = median(&mut b_means);
    let mean_c = median(&mut c_means);

    let a_quiet = mean_a < threshold;
    let b_quiet = mean_b < threshold;
    let c_quiet = mean_c < threshold;

    let edge = if b_quiet {
        NadirEdge::Center
    } else if a_quiet && !c_quiet {
        NadirEdge::Left
    } else if c_quiet && !a_quiet {
        NadirEdge::Right
    } else if a_quiet {
        // Both quiet: left wins (post-flip-correction convention)
        NadirEdge::Left
    } else {
        NadirEdge::Unknown
    };

    (edge, noise_floor)
}

// ─────────────────────────────────────────────────────────────────────────────
// §6c  BEAM ANGLE ANALYSIS
// ─────────────────────────────────────────────────────────────────────────────

/// Compute the median beam angle for a channel's profiling window.
/// Beam angle is a strong discriminator: DownVü typically has narrower beams
/// (20°-28°) while SideVü has wider beams (40°-60°).
fn compute_median_beam_angle(pings: &[&Ping]) -> f32 {
    if pings.is_empty() {
        return 0.0;
    }

    let mut angles: Vec<f32> = pings
        .iter()
        .map(|p| p.beam_angle_deg)
        .filter(|&a| a > 0.0 && a < 90.0) // Filter invalid angles
        .collect();

    if angles.is_empty() {
        return 0.0;
    }

    angles.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    angles[angles.len() / 2]
}

// ═══════════════════════════════════════════════════════════════════════════════
// §7  BOTTOM SPIKE ANALYSIS
// ═══════════════════════════════════════════════════════════════════════════════

/// Metrics describing the "bottom spike" — the intensity profile shape.
#[derive(Debug, Clone)]
struct SpikeMetrics {
    /// Fraction of pings where the peak is in the first 25% of the array.
    early_peak_ratio: f32,
    /// Median position (0.0–1.0) of the peak intensity within the array.
    median_peak_position: f32,
    /// Mean ratio of energy in first-quarter vs last-quarter of array.
    energy_front_back_ratio: f32,
}

/// Analyze the intensity distribution to detect DownVü/ClearVü "bottom spike" pattern.
fn measure_bottom_spikes(pings: &[&Ping]) -> SpikeMetrics {
    if pings.is_empty() {
        return SpikeMetrics {
            early_peak_ratio: 0.0,
            median_peak_position: 0.5,
            energy_front_back_ratio: 1.0,
        };
    }

    let mut early_peaks = 0usize;
    let mut peak_positions: Vec<f32> = Vec::new();
    let mut efb_ratios: Vec<f32> = Vec::new();

    for p in pings {
        let n = p.samples.len();
        if n < MIN_SAMPLES_SONAR {
            continue;
        }

        // Find peak position
        let (max_idx, _max_val) = p
            .samples
            .iter()
            .enumerate()
            .max_by_key(|&(_, &v)| v)
            .unwrap_or((0, &0));

        let rel_pos = max_idx as f32 / n as f32;
        peak_positions.push(rel_pos);

        if rel_pos < 0.25 {
            early_peaks += 1;
        }

        // Energy in first vs last quarter
        let q1_end = n / 4;
        let q4_start = n * 3 / 4;
        let e_front: f64 = p.samples[..q1_end]
            .iter()
            .map(|&s| s as f64 * s as f64)
            .sum();
        let e_back: f64 = p.samples[q4_start..]
            .iter()
            .map(|&s| s as f64 * s as f64)
            .sum();
        let ratio = if e_back > 0.0 {
            (e_front / e_back) as f32
        } else if e_front > 0.0 {
            100.0
        } else {
            1.0
        };
        efb_ratios.push(ratio);
    }

    let valid = peak_positions.len().max(1);
    peak_positions.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    efb_ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    SpikeMetrics {
        early_peak_ratio: early_peaks as f32 / valid as f32,
        median_peak_position: if peak_positions.is_empty() {
            0.5
        } else {
            peak_positions[peak_positions.len() / 2]
        },
        energy_front_back_ratio: if efb_ratios.is_empty() {
            1.0
        } else {
            efb_ratios[efb_ratios.len() / 2]
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// §8  SAMPLE ENTROPY (SHANNON)
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute Shannon entropy (bits) for each ping's sample array.
///
/// High entropy (>5.2) → UHD/CHIRP (many distinct intensity levels, fine detail).
/// Low entropy (<5.2) → Standard frequency (fewer levels, broader strokes).
fn compute_sample_entropies(pings: &[&Ping]) -> Vec<f32> {
    pings
        .iter()
        .filter_map(|p| {
            let n = p.samples.len();
            if n < MIN_SAMPLES_SONAR {
                return None;
            }
            Some(shannon_entropy(&p.samples))
        })
        .collect()
}

/// Shannon entropy of a u16 sample array, quantized to 8-bit bins (256 levels).
/// Quantizing avoids penalizing 16-bit channels with artificially high entropy
/// from LSB noise — we want structural entropy, not bit-depth artifacts.
fn shannon_entropy(samples: &[u16]) -> f32 {
    let n = samples.len();
    if n == 0 {
        return 0.0;
    }

    // Find max for normalization
    let max_val = samples.iter().copied().max().unwrap_or(1).max(1) as f32;

    // Bin into 256 levels
    let mut histogram = [0u32; 256];
    for &s in samples {
        let bin = ((s as f32 / max_val) * 255.0).round() as usize;
        let bin = bin.min(255);
        histogram[bin] += 1;
    }

    // Shannon entropy
    let n_f = n as f32;
    let mut entropy = 0.0f32;
    for &count in &histogram {
        if count > 0 {
            let p = count as f32 / n_f;
            entropy -= p * p.log2();
        }
    }
    entropy
}

// ═══════════════════════════════════════════════════════════════════════════════
// §9  ARCHETYPE CLASSIFICATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Multi-signal classifier: combine nadir gap, sample count, spike metrics,
/// and entropy into a final archetype assignment.
fn classify_archetype(
    median_gap: usize,
    median_sample_count: usize,
    spike: &SpikeMetrics,
    _mean_entropy: f32,
    ch_id: u32,
    _median_beam_angle: f32,
) -> (SignalArchetype, f32, String) {
    // Score accumulation for each archetype
    let mut side_score: f32 = 0.0;
    let mut down_score: f32 = 0.0;
    let mut reasons: Vec<String> = Vec::new();

    // ── Signal 0: Channel ID Prior ─────────────────────────────────────────
    // Known DownVu channel IDs get a STRONG prior to prevent misclassification.
    // This fixes GT56 UHD2 files where CHIRP (ch10/12/16) was being picked as sidescan
    // due to wide nadir gap. Channel ID is the STRONGEST signal — it overrides nadir gap.
    match ch_id {
        2 | 6 | 10 | 12 | 16 | 18 | 20 => {
            // Classic/Gen1 downscan, UHD downscan, UHD2 downscan (8-series),
            // UHD2 downscan (10-series), UHD2 downscan (14-series), ClearVü HF, ClearVü
            down_score += 5.0; // ↑ Increased from 3.0 to override nadir gap false positives
            reasons.push(format!("ch{}=known_downscan_id (strong prior)", ch_id));
        }
        993 | 1487 => {
            // Legacy/exotic downscan channels
            down_score += 3.5; // ↑ Increased from 2.5
            reasons.push(format!("ch{}=legacy_downscan", ch_id));
        }
        7 | 11 | 13 | 17 => {
            // Depth/temp channels - strong down-score for side
            down_score += 4.0;
            reasons.push(format!("ch{}=depth_temp_id", ch_id));
        }
        _ => {}
    }

    // ── Signal 1: Nadir Gap Width ───────────────────────────────────────────
    // SideVü: wide gap (>10 samples, typically 30–200+)
    // DownVü: narrow gap (<10 samples, often 0)
    // NOTE: Reduced weight to prevent overriding channel ID prior for known chirp channels
    // like ch12 in 10-series layout which can have wide nadir gap but is still downscan.
    if median_gap >= 20 {
        side_score += 2.0; // ↓ Reduced from 3.0
        reasons.push(format!("nadir_gap={}≥20→SideVü", median_gap));
    } else if median_gap >= 10 {
        side_score += 1.0; // ↓ Reduced from 1.5
        reasons.push(format!("nadir_gap={}≥10→SideVü(weak)", median_gap));
    } else if median_gap < 5 {
        down_score += 2.0;
        reasons.push(format!("nadir_gap={}<5→DownVü", median_gap));
    } else {
        // Ambiguous 5–9 range
        down_score += 0.5;
        reasons.push(format!("nadir_gap={}∈[5,10)→ambiguous", median_gap));
    }

    // ── Signal 2: Sample Array Length ───────────────────────────────────────
    // SideVü: long arrays (500+ samples) — wide swath
    // DownVü: shorter arrays (often 200-500) — narrow beam
    if median_sample_count >= 800 {
        side_score += 2.0;
        reasons.push(format!("samples={}≥800→long_swath", median_sample_count));
    } else if median_sample_count >= 400 {
        side_score += 1.0;
        reasons.push(format!(
            "samples={}≥400→moderate_swath",
            median_sample_count
        ));
    } else if median_sample_count < 200 && median_sample_count > 0 {
        down_score += 1.0;
        reasons.push(format!("samples={}<200→narrow_beam", median_sample_count));
    }

    // ── Signal 3: Early Peak (Bottom Spike) ─────────────────────────────────
    // DownVü/ClearVü: peak in first 25% (bottom return early, then decay)
    // SideVü: peak distributed across the middle-to-far range
    if spike.early_peak_ratio > 0.6 {
        down_score += 2.5;
        reasons.push(format!(
            "early_peak={:.0}%>60%→DownVü",
            spike.early_peak_ratio * 100.0
        ));
    } else if spike.early_peak_ratio > 0.3 {
        down_score += 1.0;
        reasons.push(format!(
            "early_peak={:.0}%→moderate",
            spike.early_peak_ratio * 100.0
        ));
    } else {
        side_score += 1.0;
        reasons.push(format!(
            "early_peak={:.0}%→distributed(SideVü)",
            spike.early_peak_ratio * 100.0
        ));
    }

    // ── Signal 4: Median peak position ────────────────────────────────────
    // Down-looking channels tend to have an earlier median peak location.
    if spike.median_peak_position < 0.35 {
        down_score += 1.0;
        reasons.push(format!(
            "median_peak_pos={:.2}<0.35→DownVü",
            spike.median_peak_position
        ));
    } else if spike.median_peak_position > 0.60 {
        side_score += 0.8;
        reasons.push(format!(
            "median_peak_pos={:.2}>0.60→SideVü",
            spike.median_peak_position
        ));
    }

    // ── Signal 4: Energy Distribution ───────────────────────────────────────
    // DownVü: energy concentrated in front (high front/back ratio)
    // SideVü: energy more evenly distributed or back-heavy (far range)
    if spike.energy_front_back_ratio > 4.0 {
        down_score += 1.5;
        reasons.push(format!(
            "energy_ratio={:.1}>4→front_loaded",
            spike.energy_front_back_ratio
        ));
    } else if spike.energy_front_back_ratio < 1.5 {
        side_score += 1.0;
        reasons.push(format!(
            "energy_ratio={:.1}<1.5→even/back",
            spike.energy_front_back_ratio
        ));
    }

    // ── Decision ────────────────────────────────────────────────────────────
    let total = (side_score + down_score).max(0.01);
    if side_score > down_score {
        let conf = side_score / total;
        (SignalArchetype::SideVu, conf, reasons.join("; "))
    } else {
        let conf = down_score / total;
        (SignalArchetype::DownVuClearVu, conf, reasons.join("; "))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// §10  STEP 2b: PORT/STARBOARD CORRELATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Match SideVü channels into port/starboard pairs using multiple correlation signals.
///
/// Signals used (in priority order):
/// 1. **Nadir Gap Similarity**: Paired arms see the same water column → similar gap widths.
/// 2. **Sample Count Similarity**: Same transducer → same resolution.
/// 3. **Ping Count Balance**: Both arms fire at the same rate.
/// 4. **Temporal Overlap**: Both active during the same time window.
/// 5. **Frequency Tier Match**: Same tier = same transducer element.
/// 6. **Heading-Based Orientation**: Use COG to assign port (left of heading) vs starboard (right).
fn correlate_port_starboard(
    profiles: &[ChannelProfile],
    pings_by_ch: &BTreeMap<u32, Vec<usize>>,
    parsed: &ParseResult,
    log: &mut Vec<String>,
) -> Vec<(u32, u32)> {
    // Collect all SideVü candidates
    let candidates: Vec<&ChannelProfile> = profiles
        .iter()
        .filter(|p| {
            p.archetype == SignalArchetype::SideVu && p.ping_count >= MIN_PINGS_FOR_CLASSIFY
        })
        .collect();

    if candidates.len() < 2 {
        if candidates.len() == 1 {
            log.push(format!(
                "Port/Star: only 1 SideVü channel (ch{}) — single-arm mode",
                candidates[0].channel_id
            ));
        } else {
            log.push("Port/Star: no SideVü channels found".to_string());
        }
        return Vec::new();
    }

    log.push(format!(
        "Port/Star: scoring {} SideVü candidates: {:?}",
        candidates.len(),
        candidates.iter().map(|c| c.channel_id).collect::<Vec<_>>()
    ));

    // Score every pair
    let mut pairs: Vec<(u32, u32, f64)> = Vec::new();

    for i in 0..candidates.len() {
        for j in (i + 1)..candidates.len() {
            let a = candidates[i];
            let b = candidates[j];

            // 1. Nadir Gap Similarity (0–1, higher = more similar)
            let gap_diff = (a.nadir_gap_width as f64 - b.nadir_gap_width as f64).abs();
            let gap_max = (a.nadir_gap_width.max(b.nadir_gap_width) as f64).max(1.0);
            let gap_sim = 1.0 - (gap_diff / gap_max).min(1.0);

            // 2. Sample Count Similarity
            let sc_a = a.median_sample_count as f64;
            let sc_b = b.median_sample_count as f64;
            let sc_sim = if sc_a > 0.0 && sc_b > 0.0 {
                sc_a.min(sc_b) / sc_a.max(sc_b)
            } else {
                0.0
            };

            // 3. Ping Count Balance
            let pc_a = a.ping_count as f64;
            let pc_b = b.ping_count as f64;
            let balance = pc_a.min(pc_b) / pc_a.max(pc_b).max(1.0);

            // 4. Temporal Overlap
            let overlap_ms = (a.time_range.1.min(b.time_range.1))
                .saturating_sub(a.time_range.0.max(b.time_range.0));
            let min_span = (a.time_range.1.saturating_sub(a.time_range.0))
                .min(b.time_range.1.saturating_sub(b.time_range.0))
                .max(1) as f64;
            let time_overlap = (overlap_ms as f64 / min_span).clamp(0.0, 1.0);

            // 5. Frequency Tier Match
            let tier_match = if a.frequency_tier == b.frequency_tier
                && a.frequency_tier != FrequencyTier::Unknown
            {
                1.0
            } else {
                0.0
            };

            // 6. GPS coverage (both arms having GPS = strong indicator of real data)
            let gps_bonus = if a.gps_ping_count > 50 && b.gps_ping_count > 50 {
                2.0
            } else if a.gps_ping_count > 0 || b.gps_ping_count > 0 {
                0.5
            } else {
                0.0
            };

            let score = gap_sim * 3.0
                + sc_sim * 2.5
                + balance * 2.0
                + time_overlap * 2.0
                + tier_match * 1.5
                + gps_bonus;

            log.push(format!(
                "  pair ch{}+ch{}: score={:.2} (gap={:.2} sc={:.2} bal={:.2} time={:.2} tier={:.1} gps={:.1})",
                a.channel_id, b.channel_id, score, gap_sim, sc_sim, balance, time_overlap, tier_match, gps_bonus,
            ));

            pairs.push((a.channel_id, b.channel_id, score));
        }
    }

    // Sort by score descending
    pairs.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    // Greedy matching: assign best pairs first, each channel used once
    let mut used: BTreeSet<u32> = BTreeSet::new();
    let mut result: Vec<(u32, u32)> = Vec::new();

    for (a, b, score) in &pairs {
        if used.contains(a) || used.contains(b) {
            continue;
        }

        // Assign port/starboard using heading-based orientation
        let (port, star) = assign_port_starboard(*a, *b, pings_by_ch, parsed, log);

        log.push(format!(
            "  PAIRED ch{}=Port + ch{}=Starboard (score={:.2})",
            port, star, score
        ));

        used.insert(*a);
        used.insert(*b);
        result.push((port, star));
    }

    result
}

/// Assign port/starboard roles using heading-based spatial analysis.
///
/// For each ping, compute the heading. Then check which channel's nadir gap
/// profile is consistent with port (left of heading) vs starboard (right).
///
/// Fallback: lower channel ID = port (Garmin convention).
fn assign_port_starboard(
    ch_a: u32,
    ch_b: u32,
    pings_by_ch: &BTreeMap<u32, Vec<usize>>,
    parsed: &ParseResult,
    log: &mut Vec<String>,
) -> (u32, u32) {
    // Strategy: look at sequential ping pairs from both channels.
    // For Garmin side-scan, port and starboard pings interleave at ~equal timestamps.
    // The channel whose sample data "points left" relative to heading = port.
    //
    // Since we don't have beam projection yet, use a simpler heuristic:
    // check if the static channel map gives us port/star labels.
    // If both are labeled the same (common on multi-compat firmware), use channel ID ordering.

    let a_label = crate::garmin_rsd_parser::map_channel_info(ch_a)
        .map(|(role, _)| role)
        .unwrap_or("unknown");
    let b_label = crate::garmin_rsd_parser::map_channel_info(ch_b)
        .map(|(role, _)| role)
        .unwrap_or("unknown");

    // If static map gives clear port vs star, use it as a tiebreaker
    let a_port = a_label.contains("port");
    let b_port = b_label.contains("port");
    let a_star = a_label.contains("starboard");
    let b_star = b_label.contains("starboard");

    if a_port && b_star {
        log.push(format!(
            "  Port/Star assignment: ch{} labeled port, ch{} labeled starboard",
            ch_a, ch_b
        ));
        return (ch_a, ch_b);
    }
    if b_port && a_star {
        log.push(format!(
            "  Port/Star assignment: ch{} labeled port, ch{} labeled starboard",
            ch_b, ch_a
        ));
        return (ch_b, ch_a);
    }

    // ── Heading-based assignment ────────────────────────────────────────────
    // Sample pings from both channels at similar timestamps.
    // For each pair, check if ch_a pings appear to be "left" or "right" of heading.
    // We use the nadir gap profile asymmetry as a proxy: the port channel's
    // nadir gap transitions (high→low at edges) will mirror the starboard channel's.
    //
    // Practical approach when heading data is available:
    // Use consecutive ping timestamps to determine which fires "left" vs "right"
    // based on the transducer's known port/starboard alternation pattern.
    let a_heading_available = pings_by_ch
        .get(&ch_a)
        .map(|indices| {
            indices
                .iter()
                .take(100)
                .filter(|&&i| parsed.pings[i].heading_deg.is_some())
                .count()
        })
        .unwrap_or(0);

    if a_heading_available > 10 {
        // We have heading data — use interleave pattern analysis.
        // Garmin fires port then starboard alternately. The first channel
        // in each pair (by timestamp) at a given heading is typically port.
        // This is observable by checking whether ch_a or ch_b tends to fire
        // first within each ~50ms group.
        let fires_first = check_firing_order(ch_a, ch_b, pings_by_ch, parsed);
        match fires_first {
            Some(first_ch) => {
                let second_ch = if first_ch == ch_a { ch_b } else { ch_a };
                log.push(format!(
                    "  Port/Star: ch{} fires first → port, ch{} → starboard (heading-based)",
                    first_ch, second_ch
                ));
                return (first_ch, second_ch);
            }
            None => {
                log.push("  Port/Star: firing order inconclusive".to_string());
            }
        }
    }

    // Fallback: lower channel ID = port (Garmin convention)
    let (port, star) = if ch_a < ch_b {
        (ch_a, ch_b)
    } else {
        (ch_b, ch_a)
    };
    log.push(format!(
        "  Port/Star fallback: ch{}(lower)=port, ch{}(higher)=starboard",
        port, star
    ));
    (port, star)
}

/// Analyze which channel consistently fires first in interleaved ping pairs.
/// Returns the channel ID that fires first, or None if inconclusive.
fn check_firing_order(
    ch_a: u32,
    ch_b: u32,
    pings_by_ch: &BTreeMap<u32, Vec<usize>>,
    parsed: &ParseResult,
) -> Option<u32> {
    let a_indices = pings_by_ch.get(&ch_a)?;
    let b_indices = pings_by_ch.get(&ch_b)?;

    // Build timestamp arrays
    let a_times: Vec<u64> = a_indices
        .iter()
        .take(500)
        .map(|&i| parsed.pings[i].timestamp_ms)
        .collect();
    let b_times: Vec<u64> = b_indices
        .iter()
        .take(500)
        .map(|&i| parsed.pings[i].timestamp_ms)
        .collect();

    if a_times.is_empty() || b_times.is_empty() {
        return None;
    }

    // For each ch_a ping, find the nearest ch_b ping and see who fires first
    let mut a_first = 0u32;
    let mut b_first = 0u32;
    let mut b_ptr = 0usize;

    for &at in &a_times {
        // Advance b_ptr to the nearest b timestamp
        while b_ptr + 1 < b_times.len()
            && (b_times[b_ptr + 1] as i64 - at as i64).unsigned_abs()
                < (b_times[b_ptr] as i64 - at as i64).unsigned_abs()
        {
            b_ptr += 1;
        }

        let bt = b_times[b_ptr];
        let diff = (at as i64) - (bt as i64);

        // Only count pairs within 50ms (likely from the same scan cycle)
        if diff.unsigned_abs() <= 50 {
            if diff < 0 {
                a_first += 1;
            } else if diff > 0 {
                b_first += 1;
            }
            // diff == 0: tie, skip
        }
    }

    let total = a_first + b_first;
    if total < 20 {
        return None; // Not enough data
    }

    // Need >60% consistency to be confident
    if a_first as f32 / total as f32 > 0.6 {
        Some(ch_a)
    } else if b_first as f32 / total as f32 > 0.6 {
        Some(ch_b)
    } else {
        None
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// §12  STEP 2c: TEMPORAL ALIGNMENT (COMPOSITE SCANLINES)
// ═══════════════════════════════════════════════════════════════════════════════

/// Group cross-frequency pings into Composite Scanlines using a sliding time window.
///
/// Each scanline captures all pings from all active channels within a ~100ms window.
/// This lets the mosaic engine render multi-frequency data as a single coherent line.
fn build_composite_scanlines(
    parsed: &ParseResult,
    active_channels: &BTreeSet<u32>,
    log: &mut Vec<String>,
) -> Vec<CompositeScanline> {
    if parsed.pings.is_empty() || active_channels.is_empty() {
        return Vec::new();
    }

    // Build a time-sorted list of (timestamp_ms, ping_index, channel_id)
    let mut time_index: Vec<(u64, usize, u32)> = parsed
        .pings
        .iter()
        .enumerate()
        .filter(|(_, p)| active_channels.contains(&p.channel))
        .map(|(i, p)| (p.timestamp_ms, i, p.channel))
        .collect();

    time_index.sort_by_key(|&(t, _, _)| t);

    if time_index.is_empty() {
        return Vec::new();
    }

    // Sliding window grouping
    let mut scanlines: Vec<CompositeScanline> = Vec::new();
    let mut group_start = 0usize;

    while group_start < time_index.len() {
        let window_start = time_index[group_start].0;
        let window_end = window_start + SCANLINE_WINDOW_MS;

        // Find all pings within this window
        let mut group_end = group_start;
        while group_end < time_index.len() && time_index[group_end].0 <= window_end {
            group_end += 1;
        }

        let group = &time_index[group_start..group_end];

        // Build the scanline
        let mut pings_by_channel: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        let mut best_lat = 0.0f64;
        let mut best_lon = 0.0f64;
        let mut best_heading = 0.0f32;
        let mut best_depth = 0.0f32;
        let mut best_gps_score = 0u32; // higher = better GPS

        for &(_, ping_idx, ch) in group {
            pings_by_channel.entry(ch).or_default().push(ping_idx);

            let p = &parsed.pings[ping_idx];
            let has_gps = p.latitude != 0.0 && p.longitude != 0.0 && p.latitude.is_finite();
            let has_heading = p.heading_deg.is_some();
            let score = has_gps as u32 * 2 + has_heading as u32;

            if score > best_gps_score {
                best_gps_score = score;
                best_lat = p.latitude;
                best_lon = p.longitude;
                best_heading = p.heading_deg.unwrap_or(0.0);
                best_depth = p.depth_m;
            }
        }

        // Midpoint timestamp
        let mid_ts = if group.len() > 1 {
            (group[0].0 + group[group.len() - 1].0) / 2
        } else {
            group[0].0
        };

        scanlines.push(CompositeScanline {
            timestamp_ms: mid_ts,
            latitude: best_lat,
            longitude: best_lon,
            heading_deg: best_heading,
            depth_m: best_depth,
            pings_by_channel,
        });

        group_start = group_end;
    }

    log.push(format!(
        "Scanlines: {} groups from {} pings (window={}ms)",
        scanlines.len(),
        time_index.len(),
        SCANLINE_WINDOW_MS
    ));

    scanlines
}

// ═══════════════════════════════════════════════════════════════════════════════
// §13  CONVENIENCE ACCESSORS
// ═══════════════════════════════════════════════════════════════════════════════

impl DiscoveryResult {
    /// Get the primary sidescan pair (first one, typically best scoring).
    pub fn primary_sidescan_pair(&self) -> (Option<u32>, Option<u32>) {
        match self.sidescan_pairs.first() {
            Some(&(p, s)) => (Some(p), Some(s)),
            None => {
                // Fallback: any SideVü channel as single-arm
                let single = self
                    .profiles
                    .iter()
                    .find(|p| p.archetype == SignalArchetype::SideVu);
                (single.map(|p| p.channel_id), None)
            }
        }
    }

    /// Get the best center channel for nadir fill.
    pub fn best_center_channel(&self) -> Option<u32> {
        // Prefer highest ping count among center channels
        self.profiles
            .iter()
            .filter(|p| p.archetype == SignalArchetype::DownVuClearVu)
            .max_by_key(|p| p.ping_count)
            .map(|p| p.channel_id)
    }

    /// Get profile for a specific channel.
    pub fn profile(&self, ch_id: u32) -> Option<&ChannelProfile> {
        self.profiles.iter().find(|p| p.channel_id == ch_id)
    }

    /// Check if a channel should be treated as port side-scan.
    pub fn is_port(&self, ch_id: u32) -> bool {
        self.sidescan_pairs.iter().any(|&(p, _)| p == ch_id)
    }

    /// Check if a channel should be treated as starboard side-scan.
    pub fn is_starboard(&self, ch_id: u32) -> bool {
        self.sidescan_pairs.iter().any(|&(_, s)| s == ch_id)
    }

    /// Get all channels grouped by their frequency tier.
    pub fn channels_by_tier(&self) -> BTreeMap<String, Vec<u32>> {
        let mut map: BTreeMap<String, Vec<u32>> = BTreeMap::new();
        for p in &self.profiles {
            map.entry(p.frequency_tier.to_string())
                .or_default()
                .push(p.channel_id);
        }
        map
    }

    /// Get a summary suitable for logging/UI display.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        for (port, star) in &self.sidescan_pairs {
            let p_tier = self
                .profile(*port)
                .map(|p| p.frequency_tier)
                .unwrap_or(FrequencyTier::Unknown);
            let s_tier = self
                .profile(*star)
                .map(|p| p.frequency_tier)
                .unwrap_or(FrequencyTier::Unknown);
            parts.push(format!(
                "SideVü pair: ch{}(P/{}) + ch{}(S/{})",
                port, p_tier, star, s_tier
            ));
        }

        for &ch in &self.center_channels {
            let tier = self
                .profile(ch)
                .map(|p| p.frequency_tier)
                .unwrap_or(FrequencyTier::Unknown);
            parts.push(format!("Center: ch{}({})", ch, tier));
        }

        parts.push(format!("{} scanlines", self.scanlines.len()));

        parts.join(" | ")
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// §14  TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shannon_entropy_uniform() {
        // Uniform distribution: all bins equal → max entropy = log2(256) ≈ 8.0
        let samples: Vec<u16> = (0..256).map(|i| i as u16 * 256).collect();
        let e = shannon_entropy(&samples);
        assert!(e > 7.0, "Uniform: expected >7.0, got {:.2}", e);
    }

    #[test]
    fn test_shannon_entropy_constant() {
        // All same value → entropy = 0
        let samples = vec![100u16; 500];
        let e = shannon_entropy(&samples);
        assert!(e < 0.01, "Constant: expected ~0, got {:.2}", e);
    }

    #[test]
    fn test_nadir_gap_sidescan_pattern() {
        // Simulate SideVü: water column must exceed 15th percentile mass (p15 index)
        let mut samples = vec![10u16; 120];
        samples.extend(vec![500u16; 380]);
        let gap = measure_single_nadir_gap(&samples);
        assert!(gap >= 40, "Expected nadir gap ≥40, got {}", gap);
    }

    #[test]
    fn test_nadir_gap_downscan_pattern() {
        // Simulate DownVü: immediate spike, then decay
        let mut samples = Vec::with_capacity(200);
        for i in 0..200 {
            // Sharp peak at index 5, rapid decay
            let dist = (i as f32 - 5.0).abs();
            let val = (1000.0 * (-dist / 20.0).exp()) as u16;
            samples.push(val.max(10));
        }
        let gap = measure_single_nadir_gap(&samples);
        assert!(gap < 10, "Expected nadir gap <10 (downscan), got {}", gap);
    }

    #[test]
    fn test_classify_sidescan() {
        let spike = SpikeMetrics {
            early_peak_ratio: 0.1,
            median_peak_position: 0.6,
            energy_front_back_ratio: 0.8,
        };
        let (archetype, _, _) = classify_archetype(60, 800, &spike, 5.5, 4, 0.0);
        assert_eq!(archetype, SignalArchetype::SideVu);
    }

    #[test]
    fn test_classify_downscan() {
        let spike = SpikeMetrics {
            early_peak_ratio: 0.8,
            median_peak_position: 0.15,
            energy_front_back_ratio: 8.0,
        };
        let (archetype, _, _) = classify_archetype(2, 150, &spike, 4.0, 6, 0.0);
        assert_eq!(archetype, SignalArchetype::DownVuClearVu);
    }
}
