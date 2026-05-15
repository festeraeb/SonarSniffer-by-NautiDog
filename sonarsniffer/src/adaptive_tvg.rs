//! Adaptive Time-Varied Gain (TVG) module for SonarSniffer.
//! 
//! This module provides functionality to calculate and apply gain compensation 
//! based on acoustic signal loss due to absorption and geometric spreading.

/// Configuration parameters for the TVG algorithm.
#[derive(Debug, Clone, Copy)]
pub struct TvgConfig {
    /// Geometric spreading factor (e.g., 15 for shallow, 20 for standard).
    pub spreading_factor: f32,
    /// Absorption coefficient in dB/m.
    pub absorption_db_per_m: f32,
    /// Operating frequency in Hz.
    pub frequency_hz: u32,
}

/// Returns a `TvgConfig` based on the sonar channel ID and optional frequency hint.
/// 
/// # Mapping Logic:
/// - Channels 4-7 (GT54): 800kHz, α ≈ 0.30 dB/m
/// - Channels 0-3 (GT56): 455kHz, α ≈ 0.10 dB/m
/// - Channels 8+ (UHD2): 800kHz, α ≈ 0.30 dB/m
/// - Default: 455kHz, α ≈ 0.10 dB/m
pub fn config_for_transducer(channel_id: u32, frequency_hint: Option<u32>) -> TvgConfig {
    // If a specific frequency is provided, use it to determine absorption
    let freq = frequency_hint.unwrap_or_else(|| {
        match channel_id {
            0..=3 => 455_000,
            4..=7 => 800_000,
            _ => 455_000, // Default to GT56 profile
        }
    });

    let (absorption, spreading) = if freq >= 800_000 {
        (0.30, 20.0) // GT54 / UHD2 profile
    } else {
        (0.10, 20.0) // GT56 profile
    };

    TvgConfig {
        spreading_factor: spreading,
        absorption_db_per_m: absorption,
        frequency_hz: freq,
    }
}

/// Computs a Look-Up Table (LUT) of gain multipliers.
/// 
/// The gain is calculated using the formula:
/// `Gain_dB = (2 * Spreading_Factor * Distance) + (Absorption * Distance)`
/// 
/// Returns a `Vec<f32>` where each element is the linear multiplier `10^(Gain_dB / 20)`.
pub fn compute_tvg_lut(config: &TvgConfig, n_samples: usize, sample_spacing_m: f32) -> Vec<f32> {
    let mut lut = Vec::with_capacity(n_samples);

    for i in 0..n_samples {
        let distance = i as f32 * sample_spacing_m;
        
        // Total loss in dB: Spreading loss + Absorption loss
        // Note: Spreading loss is often modeled as 2 * N * distance in dB for active sonar
        let spreading_loss_db = 2.0 * config.spreading_factor * distance;
        let absorption_loss_db = config.absorption_db_per_m * distance;
        
        let total_loss_db = spreading_loss_db + absorption_loss_db;
        
        // Convert dB loss to a linear multiplier to boost the signal
        // Gain (multiplier) = 10^(Loss_dB / 20)
        let multiplier = 10.0f32.powf(total_loss_db / 20.0);
        lut.push(multiplier);
    }

    lut
}

/// Applies the computed TVG LUT to a slice of raw sonar samples.
/// 
/// # Arguments
/// * `samples` - A slice of `u16` representing the raw acoustic amplitude.
/// * `lut` - A slice of `f32` gain multipliers.
/// 
/// # Returns
/// A `Vec<f32>` of the compensated signal.
pub fn apply_adaptive_tvg(samples: &[u16], lut: &[f32]) -> Vec<f32> {
    let n = samples.len().min(lut.len());
    let mut compensated = Vec::with_capacity(n);

    for i in 0..n {
        let raw_val = samples[i] as f32;
        let gain = lut[i];
        compensated.push(raw_val * gain);
    }

    compensated
}
