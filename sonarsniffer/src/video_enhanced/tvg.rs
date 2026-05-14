//! Time-Varied Gain (TVG) correction for sonar data.
//!
//! Compensates for:
//! 1. **Geometric spreading**: Intensity ∝ 1/r² (spherical spreading)
//! 2. **Absorption**: Intensity ∝ e^(-αr) (frequency-dependent attenuation)
//!
//! # Theory
//!
//! The sonar equation:
//! ```text
//! SL = RL + 2TL + TS
//! ```
//! Where:
//! - SL = Source Level (transmit power)
//! - RL = Received Level (what we measure)
//! - TL = Transmission Loss = 20log₁₀(r) + αr
//! - TS = Target Strength (what we want to recover)
//!
//! To recover target strength:
//! ```text
//! TS = RL + 2[20log₁₀(r) + αr]
//!    = RL + 40log₁₀(r) + 2αr
//! ```
//!
//! In linear units (what we implement):
//! ```text
//! I_corrected = I_measured × r^(spreading_factor/10) × 10^(α×r/10)
//! ```

use crate::video_enhanced::SonarProcessingParams;

// ─────────────────────────────────────────────────────────────────────────────
//  TransducerProfile — hardware-adaptive TVG parameters
// ─────────────────────────────────────────────────────────────────────────────

/// Identifies the physical transducer so TVG can use hardware-specific
/// absorption and spreading constants.
///
/// Derive from `ChannelDiscovery::FrequencyTier` + max sample value:
/// ```ignore
/// if tier == FrequencyTier::Detail && max_value < 4096 {
///     TransducerProfile::GT54_UHD1
/// } else if tier == FrequencyTier::Detail {
///     TransducerProfile::GT56_UHD2
/// } else {
///     TransducerProfile::GT51_Legacy
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum TransducerProfile {
    /// GT54 / UHD1 — 800 kHz.  High absorption due to high frequency.
    /// Typical in freshwater lakes; max sample value usually < 4096 on hardware.
    GT54_UHD1,
    /// GT56 / UHD2 — 455 kHz.  Medium absorption; standard modern transducer.
    GT56_UHD2,
    /// GT51 / Legacy — 260 kHz.  Low absorption; deep-water classic transducer.
    GT51_Legacy,
    /// Unknown / use safe conservative defaults.
    Unknown,
}

impl TransducerProfile {
    /// Freshwater absorption coefficient in dB/m at this frequency.
    ///
    /// Higher frequency = more energy absorbed per metre of propagation.
    /// Using too LOW an α for GT54 causes washout (over-brightened far range).
    /// Using too HIGH an α for GT51 causes blackout (under-brightened far range).
    pub fn absorption_db_per_m(self) -> f32 {
        match self {
            Self::GT54_UHD1 => 0.28,   // 800 kHz — strong freshwater absorption
            Self::GT56_UHD2 => 0.11,   // 455 kHz — moderate absorption
            Self::GT51_Legacy => 0.08, // 260 kHz — low absorption
            Self::Unknown => 0.10,     // safe middle ground
        }
    }

    /// Geometric spreading exponent (×10 scale) for `r^(n/10)`.
    ///
    /// Shallow-water cylindrical spreading (GT54 short range in lakes) uses a
    /// higher exponent than deep-water spherical spreading (GT51).
    pub fn spreading_factor(self) -> f32 {
        match self {
            Self::GT54_UHD1 => 25.0,   // near-field cylindrical + spherical transition
            Self::GT56_UHD2 => 20.0,   // standard spherical
            Self::GT51_Legacy => 18.0, // deep-water — slightly sub-spherical
            Self::Unknown => 20.0,
        }
    }

    /// Derive a profile from the channel's signal characteristics.
    ///
    /// `entropy_tier_is_detail`: true when Shannon entropy > 5.2 (UHD/CHIRP).
    /// `max_sample_value`: observed max u16 in a representative ping window.
    pub fn from_signal(entropy_tier_is_detail: bool, max_sample_value: u16) -> Self {
        if max_sample_value == 0 {
            return Self::Unknown;
        }
        if entropy_tier_is_detail && max_sample_value < 4096 {
            Self::GT54_UHD1 // high entropy, 12-bit range → GT54 UHD1 fingerprint
        } else if entropy_tier_is_detail {
            Self::GT56_UHD2 // high entropy, full 16-bit range → GT56 UHD2
        } else {
            Self::GT51_Legacy // low entropy → Legacy/Gen1/GT51
        }
    }
}

/// Apply TVG correction to a single ping's samples.
///
/// # Arguments
/// - `samples`: Raw u16 intensity samples (modified in place)
/// - `params`: Processing parameters (TVG settings)
///
/// # Returns
/// Corrected samples as Vec<f32> for downstream processing.
pub fn apply_tvg_correction(samples: &[u16], params: &SonarProcessingParams) -> Vec<f32> {
    if !params.tvg_enabled {
        // No correction: just convert to f32
        return samples.iter().map(|&s| s as f32).collect();
    }

    let n = samples.len();
    let mut corrected = Vec::with_capacity(n);

    let spreading_factor = params.tvg_spreading_factor;
    let absorption_db_per_m = params.tvg_absorption_db_per_m;
    let start_sample = params.tvg_start_sample;
    let sound_speed = params.sound_speed_m_per_s;
    let sample_rate = params.sample_rate_hz;

    for (i, &sample) in samples.iter().enumerate() {
        let value = if i < start_sample {
            // Skip near-field (no TVG correction)
            sample as f32
        } else {
            // Compute range in meters
            let range_m = if sample_rate > 0.0 {
                // Use actual sample rate for accurate range
                let time_s = i as f32 / sample_rate;
                (time_s * sound_speed) / 2.0 // Two-way travel
            } else {
                // Fallback: use sample index as proxy
                // Assume ~1 sample per meter (rough approximation)
                i as f32
            };

            // Avoid division by zero or negative range
            let range_m = range_m.max(1.0);

            // Geometric spreading correction: I × r^(spreading_factor/10)
            let spreading_gain = range_m.powf(spreading_factor / 10.0);

            // Absorption correction: I × 10^(α×r/10)
            let absorption_gain = 10.0_f32.powf((absorption_db_per_m * range_m) / 10.0);

            // Apply combined TVG
            let tvg_gain = spreading_gain * absorption_gain;
            sample as f32 * tvg_gain
        };

        corrected.push(value);
    }

    corrected
}

/// Precompute TVG lookup table for performance (if processing many pings with same params).
///
/// Returns a LUT where `lut[sample_idx]` = TVG gain factor.
pub fn precompute_tvg_lut(max_samples: usize, params: &SonarProcessingParams) -> Vec<f32> {
    if !params.tvg_enabled {
        return vec![1.0; max_samples];
    }

    let spreading_factor = params.tvg_spreading_factor;
    let absorption_db_per_m = params.tvg_absorption_db_per_m;
    let start_sample = params.tvg_start_sample;
    let sound_speed = params.sound_speed_m_per_s;
    let sample_rate = params.sample_rate_hz;

    let mut lut = Vec::with_capacity(max_samples);

    for i in 0..max_samples {
        let gain = if i < start_sample {
            1.0
        } else {
            let range_m = if sample_rate > 0.0 {
                let time_s = i as f32 / sample_rate;
                (time_s * sound_speed) / 2.0
            } else {
                i as f32
            };
            let range_m = range_m.max(1.0);

            let spreading_gain = range_m.powf(spreading_factor / 10.0);
            let absorption_gain = 10.0_f32.powf((absorption_db_per_m * range_m) / 10.0);
            spreading_gain * absorption_gain
        };
        lut.push(gain);
    }

    lut
}

/// Compute the TVG start sample from a hardware blanking zone.
///
/// The TVG must NOT apply gain inside the blanking zone — applying geometric
/// spreading and absorption corrections to hardware-silenced zeros produces
/// high-contrast ghost stripes.
///
/// Pass the return value as `SonarProcessingParams::tvg_start_sample` before
/// calling `precompute_tvg_lut`, or use it directly in the pipeline:
///
/// ```ignore
/// let blanking = egn::detect_blanking_zone(&representative_slices);
/// params.tvg_start_sample = tvg::blanking_aware_start_sample(&blanking);
/// let lut = tvg::precompute_tvg_lut(max_samples, &params);
/// ```
///
/// When no blanking zone is detected, returns the existing `tvg_start_sample`
/// unchanged (pass `params.tvg_start_sample` as `current_start`).
pub fn blanking_aware_start_sample(
    blanking: &crate::egn::BlankingZone,
    current_start: usize,
) -> usize {
    if blanking.is_active() {
        blanking.end_sample.max(current_start)
    } else {
        current_start
    }
}

/// Apply precomputed TVG LUT to samples (faster for batch processing).
pub fn apply_tvg_lut(samples: &[u16], lut: &[f32]) -> Vec<f32> {
    samples
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let gain = if i < lut.len() { lut[i] } else { 1.0 };
            s as f32 * gain
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tvg_disabled() {
        let samples = vec![100u16, 200, 300];
        let params = SonarProcessingParams {
            tvg_enabled: false,
            ..Default::default()
        };
        let corrected = apply_tvg_correction(&samples, &params);

        assert_eq!(corrected.len(), 3);
        assert_eq!(corrected[0], 100.0);
        assert_eq!(corrected[1], 200.0);
        assert_eq!(corrected[2], 300.0);
    }

    #[test]
    fn test_tvg_increases_with_range() {
        let samples = vec![100u16; 100];
        let params = SonarProcessingParams {
            tvg_enabled: true,
            tvg_spreading_factor: 20.0,
            tvg_absorption_db_per_m: 0.1,
            tvg_start_sample: 5,
            ..Default::default()
        };
        let corrected = apply_tvg_correction(&samples, &params);

        // Near-field unchanged
        assert_eq!(corrected[0], 100.0);
        assert_eq!(corrected[4], 100.0);

        // Far-field should increase (compensating for loss)
        assert!(corrected[10] > corrected[5]);
        assert!(corrected[50] > corrected[10]);
        assert!(corrected[99] > corrected[50]);
    }

    #[test]
    fn test_tvg_lut_matches_direct() {
        let samples = vec![100u16, 200, 300, 400];
        let params = SonarProcessingParams {
            tvg_enabled: true,
            tvg_spreading_factor: 20.0,
            ..Default::default()
        };

        let direct = apply_tvg_correction(&samples, &params);
        let lut = precompute_tvg_lut(samples.len(), &params);
        let from_lut = apply_tvg_lut(&samples, &lut);

        for (d, l) in direct.iter().zip(from_lut.iter()) {
            assert!((d - l).abs() < 0.01, "Direct: {}, LUT: {}", d, l);
        }
    }
}

/// Simple TVG LUT without full SonarProcessingParams.
/// `spreading_factor`: typically 15-30 (20 = spherical spreading)
/// `absorption_db_per_m`: typically 0.04-0.12 for typical sonar frequencies
pub fn precompute_tvg_lut_simple(
    max_samples: usize,
    spreading_factor: f32,
    absorption_db_per_m: f32,
) -> Vec<f32> {
    let mut lut = Vec::with_capacity(max_samples);
    for i in 0..max_samples {
        let range = (i as f32).max(1.0);
        let spreading_gain = range.powf(spreading_factor / 10.0);
        let absorption_gain = 10.0_f32.powf((absorption_db_per_m * range) / 10.0);
        lut.push(spreading_gain * absorption_gain);
    }
    lut
}
