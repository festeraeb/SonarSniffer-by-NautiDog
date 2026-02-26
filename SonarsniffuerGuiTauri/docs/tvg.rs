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
