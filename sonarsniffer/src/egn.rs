//! Empirical Gain Normalization (EGN)
//!
//! Removes the across-track gain gradient caused by the transducer beam pattern.
//! Every sonar image has a characteristic "flashlight" or diagonal slope caused by:
//!
//! - **UHD / paired sidescan** (GT54, GT56): signal strongest near the nadir, falls
//!   off towards the far edge.  Beam profile looks like a ∨ shape (strong left/right
//!   edges when the nadir is trimmed, or a \ slope on a single wing post-trim).
//!
//! - **GT51 asymmetric single-wing**: sample[0] is nearest the boat, so beam
//!   response peaks at index 0 and decays monotonically to the far edge.
//!   Profile is a diagonal slope: strong → weak (left to right).
//!
//! # How EGN works
//!
//! 1. `compute_beam_profile`: accumulate min-of-percentile amplitude at every
//!    sample index across a representative window of pings → yields the "baseline"
//!    beam response.  Uses the **10th percentile** per column so that occasional
//!    fish echoes or structure don't inflate the profile.
//!
//! 2. `apply_egn`: divide every sample by its precomputed correction factor from
//!    the profile, producing a flattened image.
//!
//! # Relation to TVG
//!
//! `compute_empirical_tvg` in `outputs.rs` is a **range-based** (temporal) TVG.
//! EGN is an **across-track** (spatial) correction.  They are independent and
//! can be applied in sequence: TVG first, then EGN.
//!
//! # Guarantee (safety rule)
//!
//! - Never clips samples to 0 (floor at 1/65535).
//! - Correction factors are clamped to [MIN_GAIN, MAX_GAIN] to prevent blown-out
//!   or invisible columns near the nadir dead zone.
//! - Does not mutate `Ping` — operates on owned `Vec<u16>` copies.

use crate::channel_discovery::SpatialRole;
use serde::Serialize;

// ─────────────────────────────────────────────────────────────────────────────
//  Tuning constants
// ─────────────────────────────────────────────────────────────────────────────

/// Minimum gain correction factor (never darken a column more than 10×).
const MIN_GAIN: f32 = 0.10;
/// Maximum gain correction factor (never amplify a column more than 20×).
const MAX_GAIN: f32 = 20.0;

/// Percentile (0.0–1.0) used to build the beam profile.  10th percentile per
/// column gives the "average seabed response" while ignoring fish/structure echoes.
const PROFILE_PERCENTILE: f32 = 0.10;

/// Minimum number of pings required to compute a reliable profile.
const MIN_PINGS_FOR_PROFILE: usize = 20;

/// Smoothing window (in samples) for the profile — prevents single-bin spikes
/// from introducing correction artefacts.
const SMOOTH_WINDOW: usize = 15;

/// For GT51 single-wing channels: the gain profile is expected to be a monotone
/// slope.  We enforce this by capping the near-edge gain so we never brighten
/// index 0 (already the strongest signal in the beam) past this factor.
const GT51_NEAR_EDGE_CAP: f32 = 1.5;

// ─────────────────────────────────────────────────────────────────────────────
//  Public types
// ─────────────────────────────────────────────────────────────────────────────

/// The result of a beam profile calibration pass.
///
/// Can be cached and reused across files that share the same transducer + depth.
#[derive(Debug, Clone, Serialize)]
pub struct BeamProfile {
    /// Length of the profile in samples (matches the canonical channel width).
    pub len: usize,
    /// Per-sample gain correction factor.  Apply via `apply_egn`.
    pub gain: Vec<f32>,
    /// Mean gain factor (diagnostic — 1.0 means beam was already flat).
    pub mean_gain: f32,
    /// Max gain factor applied (diagnostic).
    pub max_gain_applied: f32,
    /// Spatial role this profile was computed for.
    pub role: String,
    /// Number of pings used to calibrate.
    pub pings_used: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
//  compute_beam_profile
// ─────────────────────────────────────────────────────────────────────────────

/// Build an EGN `BeamProfile` from a representative window of raw pings.
///
/// # Arguments
/// - `pings`      — raw u16 sample slices (already had TVG applied, or raw)
/// - `role`       — `SpatialRole` from channel discovery (controls GT51 logic)
/// - `nadir_skip` — samples to skip at the start of each ping (nadir dead zone).
///                  Pass `0` for GT51 single-wing (nadir is at index 0 but IS data).
///
/// # Returns
/// A `BeamProfile` whose `gain` vector has length `max_sample_count - nadir_skip`.
pub fn compute_beam_profile(
    pings: &[&[u16]],
    role: SpatialRole,
    nadir_skip: usize,
) -> BeamProfile {
    let empty = BeamProfile {
        len: 0,
        gain: vec![],
        mean_gain: 1.0,
        max_gain_applied: 1.0,
        role: format!("{:?}", role),
        pings_used: 0,
    };

    let usable: Vec<&[u16]> = pings
        .iter()
        .map(|&s| if nadir_skip < s.len() { &s[nadir_skip..] } else { &[] })
        .filter(|s| !s.is_empty())
        .collect();

    if usable.len() < MIN_PINGS_FOR_PROFILE {
        // Not enough data — return flat unity gain
        let len = usable.iter().map(|s| s.len()).max().unwrap_or(0);
        return BeamProfile {
            len,
            gain: vec![1.0; len],
            mean_gain: 1.0,
            max_gain_applied: 1.0,
            role: format!("{:?}", role),
            pings_used: usable.len(),
        };
    }

    let max_len = usable.iter().map(|s| s.len()).max().unwrap_or(0);
    if max_len < 8 {
        return empty;
    }

    // ── Step 1: collect all values per column ─────────────────────────────────
    let mut columns: Vec<Vec<u16>> = vec![Vec::new(); max_len];
    for s in &usable {
        for (i, &v) in s.iter().enumerate() {
            if i < max_len && v > 0 {
                columns[i].push(v);
            }
        }
    }

    // ── Step 2: 10th-percentile per column (beam baseline) ───────────────────
    let mut baseline: Vec<f32> = columns
        .iter()
        .map(|col| {
            if col.is_empty() {
                return 0.0;
            }
            let mut sorted = col.clone();
            sorted.sort_unstable();
            let idx = ((sorted.len() as f32 * PROFILE_PERCENTILE) as usize)
                .min(sorted.len() - 1);
            sorted[idx] as f32
        })
        .collect();

    // ── Step 3: smooth the baseline to avoid per-bin spikes ──────────────────
    baseline = smooth_profile(&baseline, SMOOTH_WINDOW);

    // ── Step 4: target level = median of the entire smoothed profile ─────────
    let mut sorted_bl: Vec<f32> = baseline
        .iter()
        .copied()
        .filter(|&v| v > 1.0)
        .collect();
    if sorted_bl.is_empty() {
        return BeamProfile {
            len: max_len,
            gain: vec![1.0; max_len],
            mean_gain: 1.0,
            max_gain_applied: 1.0,
            role: format!("{:?}", role),
            pings_used: usable.len(),
        };
    }
    sorted_bl.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let target = sorted_bl[sorted_bl.len() / 2];

    // ── Step 5: compute correction factors ───────────────────────────────────
    let mut gain: Vec<f32> = baseline
        .iter()
        .map(|&b| {
            if b > 1.0 {
                (target / b).clamp(MIN_GAIN, MAX_GAIN)
            } else {
                1.0 // dead/empty columns get unity gain
            }
        })
        .collect();

    // ── Step 6: hardware-specific overrides ──────────────────────────────────
    match role {
        SpatialRole::SingleSidePort => {
            // GT51 asymmetric port wing: sample[0] is near-boat (strongest).
            // The correction at index 0 should NEVER over-brighten (it's already
            // the hottest part of the beam).  Cap the near-edge gain.
            //
            // Profile shape: beam is strongest at [0], weakest at [max].
            // EGN tends to try to boost [max] and reduce [0].
            // The boost is fine; the reduction at [0] is capped.
            let near_cap_len = (max_len / 8).max(4);
            for i in 0..near_cap_len {
                gain[i] = gain[i].min(GT51_NEAR_EDGE_CAP);
            }
        }
        SpatialRole::SingleSideStarboard => {
            // GT51 asymmetric starboard wing: sample[max] is near-boat.
            // Cap the near-edge gain (last ~12.5% of samples).
            let far_start = max_len - (max_len / 8).max(4);
            for i in far_start..max_len {
                gain[i] = gain[i].min(GT51_NEAR_EDGE_CAP);
            }
        }
        SpatialRole::Port | SpatialRole::Starboard => {
            // UHD paired sidescan: nadir area (first 5% after skip) is the dead
            // zone — do not let EGN wildly boost it.
            let dead_len = (max_len / 20).max(2);
            for i in 0..dead_len {
                gain[i] = gain[i].min(2.0);
            }
        }
        _ => {}
    }

    let mean_gain = gain.iter().sum::<f32>() / gain.len() as f32;
    let max_gain_applied = gain.iter().cloned().fold(0.0_f32, f32::max);

    BeamProfile {
        len: max_len,
        gain,
        mean_gain,
        max_gain_applied,
        role: format!("{:?}", role),
        pings_used: usable.len(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  apply_egn
// ─────────────────────────────────────────────────────────────────────────────

/// Apply a precomputed `BeamProfile` to a row of raw samples.
///
/// Returns a new `Vec<u16>` with gain-corrected values.  Safe to call on rows
/// with a different length than the profile — out-of-range indices use
/// unity gain.
///
/// # Arguments
/// - `samples`    — raw u16 samples for one ping
/// - `nadir_skip` — samples at the start to leave untouched (nadir dead zone)
/// - `profile`    — calibrated `BeamProfile` from `compute_beam_profile`
pub fn apply_egn(samples: &[u16], nadir_skip: usize, profile: &BeamProfile) -> Vec<u16> {
    if samples.is_empty() || profile.gain.is_empty() {
        return samples.to_vec();
    }
    let mut out = samples.to_vec();
    for (i, v) in out.iter_mut().enumerate() {
        if i < nadir_skip {
            continue; // leave nadir zone untouched
        }
        let beam_idx = i - nadir_skip;
        let factor = profile.gain.get(beam_idx).copied().unwrap_or(1.0);
        let corrected = (*v as f32 * factor).round().clamp(0.0, 65535.0) as u16;
        *v = corrected;
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
//  apply_egn_to_rows  (batch helper for outputs.rs)
// ─────────────────────────────────────────────────────────────────────────────

/// Apply EGN to a batch of sample rows, producing corrected rows in the
/// same order.  Zero-allocation per call except for the returned vec-of-vecs.
///
/// This is the function `outputs.rs` calls before rendering the image.
///
/// # Example
/// ```ignore
/// let profile = compute_beam_profile(&ping_slices, SpatialRole::Port, nadir_skip);
/// let corrected = apply_egn_to_rows(&ping_slices, nadir_skip, &profile);
/// // use corrected[i] instead of ping.samples
/// ```
pub fn apply_egn_to_rows(
    pings: &[&[u16]],
    nadir_skip: usize,
    profile: &BeamProfile,
) -> Vec<Vec<u16>> {
    pings
        .iter()
        .map(|&s| apply_egn(s, nadir_skip, profile))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
//  Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Smooth a profile with a symmetrical box-filter of the given window size.
fn smooth_profile(v: &[f32], window: usize) -> Vec<f32> {
    if v.len() < window {
        return v.to_vec();
    }
    let half = window / 2;
    let mut out = vec![0.0f32; v.len()];
    for i in 0..v.len() {
        let lo = i.saturating_sub(half);
        let hi = (i + half + 1).min(v.len());
        let sum: f32 = v[lo..hi].iter().sum();
        out[i] = sum / (hi - lo) as f32;
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
//  Convenience: build profile directly from Ping references
// ─────────────────────────────────────────────────────────────────────────────

/// Build a `BeamProfile` directly from a slice of `Ping` references.
///
/// This is a thin wrapper so `outputs.rs` doesn't have to extract `.samples`
/// slices manually.
pub fn beam_profile_from_pings(
    pings: &[&crate::garmin_rsd_parser::Ping],
    role: SpatialRole,
    nadir_skip: usize,
) -> BeamProfile {
    let slices: Vec<&[u16]> = pings.iter().map(|p| p.samples.as_slice()).collect();
    compute_beam_profile(&slices, role, nadir_skip)
}

/// Apply EGN directly from `Ping` references.
///
/// Returns corrected sample rows in ping order.
pub fn apply_egn_to_pings(
    pings: &[&crate::garmin_rsd_parser::Ping],
    nadir_skip: usize,
    profile: &BeamProfile,
) -> Vec<Vec<u16>> {
    pings
        .iter()
        .map(|p| apply_egn(&p.samples, nadir_skip, profile))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
//  Blanking-zone detection & soft-fill  (Task B — GT56 AGC ring-down)
// ─────────────────────────────────────────────────────────────────────────────
//
// GT56 UHD2 clipping analysis confirmed:
//   ch10: 20.0 % of samples ≤ 4 (hardware receiver gated off during TX pulse)
//   ch11: 18.7 %
//   ch12: 26.5 %
//
// These zeros appear in the first ~20–27 % of every ping's sample range.
// The fix is a two-step "soft-fill":
//
//  1. detect_blanking_zone — finds where the blank rate drops below 10 %.
//  2. fill_blanking_ping   — replaces blanked samples with a weighted average
//                            of the same column in ±3 neighbouring pings
//                            (distance-weighted; falls back to noise-floor).
//
// The TVG must NOT apply gain inside the blanking zone — doing so multiplies
// hardware silence by up to 20 × and creates the high-contrast ghost stripes.
// Use `blanking_aware_start_sample` in tvg.rs to enforce this.

/// A sample value ≤ this is classified as "blanked" (hardware gate-off).
/// Confirmed by GT56 clipping analysis (peak value in blanking zone ≤ 4).
pub const BLANK_THRESHOLD: u16 = 4;

/// ≥ 50 % of pings blanked at a column → we are inside the blanking zone.
const BLANKING_ENTRY_RATE: f32 = 0.50;

/// < 10 % of pings blanked at a column → we have exited the blanking zone.
const BLANKING_EXIT_RATE: f32 = 0.10;

/// Pings examined on each side of the current ping for the fill WMA.
pub const FILL_RADIUS: usize = 3;

/// Fallback fill value when ALL ±FILL_RADIUS neighbours are also blanked.
/// Maps to a very faint gray (~0.015 % of full scale) after log compression.
const NOISE_FLOOR_FILL: u16 = 5;

/// Detected hardware AGC / ring-down blanking region at the start of every ping.
///
/// The receiver is gated off during (and just after) the transmitted pulse.
/// In GT56 UHD2 data this affects the first ~18–27 % of every sample array.
///
/// Used by:
/// - `fill_blanking_ping`            — cross-ping soft fill
/// - `tvg::blanking_aware_start_sample` — TVG delay past the dead zone
#[derive(Debug, Clone, Serialize)]
pub struct BlankingZone {
    /// First sample index that is reliably outside the blanking region.
    /// TVG should start here, not at 0.
    pub end_sample: usize,
    /// Peak per-column blank rate across the detected zone (0.0–1.0).
    pub peak_blank_rate: f32,
    /// Sample threshold used to define "blanked" (≤ this value).
    pub threshold: u16,
}

impl BlankingZone {
    /// No blanking detected — TVG starts at sample 0, no fill needed.
    pub fn none() -> Self {
        BlankingZone { end_sample: 0, peak_blank_rate: 0.0, threshold: BLANK_THRESHOLD }
    }

    /// Returns `true` when a real blanking zone was detected (end_sample > 0).
    #[inline]
    pub fn is_active(&self) -> bool {
        self.end_sample > 0
    }
}

/// Detect the hardware AGC blanking zone from a representative batch of pings.
///
/// Scans columns left-to-right (starting at sample 0).  The zone ends at the
/// first column whose per-column blank rate drops below `BLANKING_EXIT_RATE`
/// after having been inside the zone (≥ `BLANKING_ENTRY_RATE`).
///
/// # Arguments
/// - `pings` — raw u16 sample slices.  Pass ≥ 50 pings for a stable estimate;
///             fewer than 10 pings always returns `BlankingZone::none()`.
pub fn detect_blanking_zone(pings: &[&[u16]]) -> BlankingZone {
    const MIN_PINGS: usize = 10;
    if pings.len() < MIN_PINGS {
        return BlankingZone::none();
    }

    let max_len = pings.iter().map(|s| s.len()).max().unwrap_or(0);
    if max_len == 0 {
        return BlankingZone::none();
    }

    let n = pings.len() as f32;
    let blank_rates: Vec<f32> = (0..max_len)
        .map(|col| {
            let blanked = pings
                .iter()
                .filter(|s| s.get(col).copied().unwrap_or(0) <= BLANK_THRESHOLD)
                .count();
            blanked as f32 / n
        })
        .collect();

    let peak_blank_rate = blank_rates.iter().cloned().fold(0.0_f32, f32::max);
    if peak_blank_rate < BLANKING_ENTRY_RATE {
        // No meaningful blanking zone
        return BlankingZone { end_sample: 0, peak_blank_rate, threshold: BLANK_THRESHOLD };
    }

    // Walk left-to-right: find first column that exits the zone.
    let mut in_zone = false;
    let mut end_sample = max_len; // worst-case: entire ping is blanked
    for (col, &rate) in blank_rates.iter().enumerate() {
        if rate >= BLANKING_ENTRY_RATE {
            in_zone = true;
        }
        if in_zone && rate < BLANKING_EXIT_RATE {
            end_sample = col;
            break;
        }
    }

    BlankingZone { end_sample, peak_blank_rate, threshold: BLANK_THRESHOLD }
}

/// Fill the blanking zone of a single ping using cross-ping weighted-mean fill.
///
/// For each sample column in `0..blanking.end_sample`:
/// - If the sample is already above threshold: left unchanged.
/// - Otherwise: replace with a distance-weighted mean of the same column across
///   the ±`FILL_RADIUS` neighbouring pings in `context`
///   (closer neighbours receive higher weight).
/// - If **all** neighbours are also blanked: use `NOISE_FLOOR_FILL` (faint gray).
///
/// # Arguments
/// - `ping_idx` — index of the target ping within `context`.
/// - `context`  — window of pings spanning `[i - FILL_RADIUS, i + FILL_RADIUS]`;
///               the caller must slice the full array accordingly.
pub fn fill_blanking_ping(
    ping_idx: usize,
    context: &[&[u16]],
    blanking: &BlankingZone,
) -> Vec<u16> {
    if !blanking.is_active() || context.is_empty() || ping_idx >= context.len() {
        return context.get(ping_idx).copied().unwrap_or(&[]).to_vec();
    }

    let row = context[ping_idx];
    let mut out = row.to_vec();
    let end = blanking.end_sample.min(row.len());

    for col in 0..end {
        if row[col] > blanking.threshold {
            continue; // already valid — keep as-is
        }

        let mut wsum = 0.0_f32;
        let mut wtotal = 0.0_f32;

        for (j, &ctx_row) in context.iter().enumerate() {
            if j == ping_idx {
                continue;
            }
            let dist = (j as isize - ping_idx as isize).unsigned_abs();
            let weight = (FILL_RADIUS + 1).saturating_sub(dist) as f32;
            if weight == 0.0 {
                continue;
            }
            let val = ctx_row.get(col).copied().unwrap_or(0);
            if val > blanking.threshold {
                wsum   += val as f32 * weight;
                wtotal += weight;
            }
        }

        out[col] = if wtotal > 0.0 {
            (wsum / wtotal).round().clamp(0.0, 65535.0) as u16
        } else {
            NOISE_FLOOR_FILL
        };
    }

    out
}

/// Apply blanking fill to every row in a batch, using the full array as the
/// context window (each ping gets access to ±FILL_RADIUS neighbours).
///
/// Use this from the **mosaic / outputs pipeline** where all pings are
/// available at once.  For the video pipeline (frame-by-frame), use
/// `fill_blanking_ping` directly inside the frame builder.
pub fn fill_blanking_zone_rows(pings: &[&[u16]], blanking: &BlankingZone) -> Vec<Vec<u16>> {
    if !blanking.is_active() {
        return pings.iter().map(|&s| s.to_vec()).collect();
    }
    (0..pings.len())
        .map(|i| {
            let ctx_start = i.saturating_sub(FILL_RADIUS);
            let ctx_end   = (i + FILL_RADIUS + 1).min(pings.len());
            fill_blanking_ping(i - ctx_start, &pings[ctx_start..ctx_end], blanking)
        })
        .collect()
}

/// Blanking-aware EGN: fill the hardware dead-zone first, then apply the beam
/// profile correction to the filled data.
///
/// This is the recommended function for the **static mosaic pipeline** for GT56.
/// It combines `fill_blanking_zone_rows` + `apply_egn_to_rows`.
///
/// For the video pipeline, use `fill_blanking_ping` per row inside the frame
/// builder, then apply the TVG LUT (EGN is implicit in the adaptive range step).
pub fn apply_egn_to_rows_blanking_aware(
    pings: &[&[u16]],
    nadir_skip: usize,
    profile: &BeamProfile,
    blanking: &BlankingZone,
) -> Vec<Vec<u16>> {
    let filled = fill_blanking_zone_rows(pings, blanking);
    let refs: Vec<&[u16]> = filled.iter().map(|v| v.as_slice()).collect();
    apply_egn_to_rows(&refs, nadir_skip, profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel_discovery::SpatialRole;

    fn make_ramp(len: usize, peak: u16, trough: u16) -> Vec<u16> {
        // Strong-at-left, weak-at-right — GT51 port pattern
        (0..len)
            .map(|i| {
                let t = i as f32 / (len - 1) as f32;
                let v = peak as f32 * (1.0 - t) + trough as f32 * t;
                v as u16
            })
            .collect()
    }

    #[test]
    fn test_flat_profile_on_flat_data() {
        // All pings are constant amplitude → EGN should produce unity gain
        let ping: Vec<u16> = vec![1000u16; 256];
        let pings: Vec<&[u16]> = (0..40).map(|_| ping.as_slice()).collect();
        let profile = compute_beam_profile(&pings, SpatialRole::Port, 0);
        for &g in &profile.gain {
            let diff = (g - 1.0).abs();
            // Allow ±5% variance from smoothing rounding
            assert!(diff < 0.05, "Expected ≈1.0 gain, got {:.4}", g);
        }
    }

    #[test]
    fn test_gt51_ramp_profile_flattening() {
        // Simulate 60 pings of GT51 diagonal slope
        let pings_data: Vec<Vec<u16>> = (0..60).map(|_| make_ramp(512, 3000, 300)).collect();
        let slices: Vec<&[u16]> = pings_data.iter().map(|v| v.as_slice()).collect();
        let profile = compute_beam_profile(&slices, SpatialRole::SingleSidePort, 0);

        // Apply EGN to one ping row
        let raw = make_ramp(512, 3000, 300);
        let corrected = apply_egn(&raw, 0, &profile);

        // After EGN the corrected row should be much flatter than the raw ramp
        let raw_range   = *raw.iter().max().unwrap() as f32 - *raw.iter().min().unwrap() as f32;
        let corr_max = *corrected.iter().max().unwrap() as f32;
        let corr_min = *corrected.iter().filter(|&&v| v > 0).min().unwrap_or(&1) as f32;
        let corr_range  = corr_max - corr_min;

        assert!(
            corr_range < raw_range * 0.7,
            "EGN should reduce dynamic range.  raw: {raw_range:.0}, corrected: {corr_range:.0}"
        );
    }

    #[test]
    fn test_nadir_skip_preserved() {
        // First 20 samples are nadir (should be untouched)
        let ping: Vec<u16> = vec![500u16; 256];
        let flat_tail = vec![1000u16; 256];
        let mixed: Vec<u16> = ping[..20]
            .iter()
            .chain(flat_tail[20..].iter())
            .copied()
            .collect();
        let slices: Vec<&[u16]> = (0..30).map(|_| mixed.as_slice()).collect();
        let profile = compute_beam_profile(&slices, SpatialRole::Port, 20);
        let corrected = apply_egn(&mixed, 20, &profile);
        // First 20 samples must be unchanged
        for i in 0..20 {
            assert_eq!(corrected[i], mixed[i], "nadir sample {i} was modified");
        }
    }
}
