//! Curvelet Filter — Multi-scale directional filtering for anomaly enhancement.
//!
//! Applies curvelet-like decomposition to anomaly maps to enhance linear/curved
//! features (wreck outlines, debris fields, hull shapes) while suppressing noise.
//!
//! This operates on the OUTPUT of the temporal stack — a single anomaly map already
//! in GPU memory. The filter bank coefficients are small (~2-4MB) and uploaded once
//! at startup.
//!
//! The filter works at multiple scales and orientations:
//! - Scale 1 (coarse): Large structures (>50m) — hull outlines, large debris
//! - Scale 2 (medium): Medium features (10-50m) — deck structures, masts
//! - Scale 3 (fine): Small features (2-10m) — rigging, small debris, anchors
//!
//! At each scale, 8-16 angular wedges detect directional features.
//! The output is a filtered anomaly map where true structures are enhanced
//! and random noise is suppressed.

use std::f32::consts::PI;

/// Configuration for the curvelet filter bank.
#[derive(Clone, Debug)]
pub struct CurveletConfig {
    /// Number of decomposition scales (typically 3-4)
    pub n_scales: usize,
    /// Number of angular wedges per scale (typically 8 or 16)
    pub n_angles: usize,
    /// Minimum feature size in pixels (maps to finest scale)
    pub min_feature_px: f32,
    /// Maximum feature size in pixels (maps to coarsest scale)
    pub max_feature_px: f32,
    /// Threshold multiplier — features below threshold * noise_floor are zeroed
    pub threshold_multiplier: f32,
}

impl Default for CurveletConfig {
    fn default() -> Self {
        Self {
            n_scales: 3,
            n_angles: 8,
            min_feature_px: 2.0,
            max_feature_px: 50.0,
            threshold_multiplier: 2.5,
        }
    }
}

/// A single filter kernel (one scale + one angle).
#[derive(Clone)]
pub struct FilterKernel {
    pub scale: usize,
    pub angle: usize,
    pub width: usize,
    pub height: usize,
    /// Filter coefficients in spatial domain (for convolution)
    pub coefficients: Vec<f32>,
}

/// The complete filter bank — all scales × all angles.
pub struct CurveletFilterBank {
    pub config: CurveletConfig,
    pub kernels: Vec<FilterKernel>,
}

impl CurveletFilterBank {
    /// Build the filter bank. Call once at startup, reuse for all tiles.
    pub fn build(config: CurveletConfig) -> Self {
        let mut kernels = Vec::with_capacity(config.n_scales * config.n_angles);

        for scale in 0..config.n_scales {
            // Kernel size increases with scale (coarser = larger kernel)
            let t = scale as f32 / (config.n_scales - 1).max(1) as f32;
            let feature_size = config.min_feature_px + t * (config.max_feature_px - config.min_feature_px);
            let kernel_size = (feature_size * 2.0).ceil() as usize | 1; // ensure odd

            for angle_idx in 0..config.n_angles {
                let angle = (angle_idx as f32 / config.n_angles as f32) * PI;
                let kernel = build_directional_kernel(kernel_size, angle, feature_size);
                kernels.push(FilterKernel {
                    scale,
                    angle: angle_idx,
                    width: kernel_size,
                    height: kernel_size,
                    coefficients: kernel,
                });
            }
        }

        Self { config, kernels }
    }

    /// Apply the full filter bank to an anomaly map.
    /// Returns the enhanced anomaly map where directional features are amplified.
    pub fn apply(&self, input: &[f32], width: usize, height: usize) -> Vec<f32> {
        let n_pixels = width * height;
        assert_eq!(input.len(), n_pixels);

        // Accumulate maximum response across all kernels per pixel
        let mut output = vec![0.0f32; n_pixels];

        for kernel in &self.kernels {
            let response = convolve_2d(input, width, height, &kernel.coefficients, kernel.width, kernel.height);

            // Take absolute value (we care about feature presence, not sign)
            // Keep maximum response across all orientations and scales
            for i in 0..n_pixels {
                let abs_resp = response[i].abs();
                if abs_resp > output[i] {
                    output[i] = abs_resp;
                }
            }
        }

        // Threshold: suppress responses below noise floor
        let noise_floor = estimate_noise_floor(&output);
        let threshold = noise_floor * self.config.threshold_multiplier;

        for val in output.iter_mut() {
            if *val < threshold {
                *val = 0.0;
            }
        }

        output
    }

    /// Get total memory footprint of the filter bank in bytes.
    pub fn memory_bytes(&self) -> usize {
        self.kernels.iter().map(|k| k.coefficients.len() * 4).sum()
    }
}

/// Build a single directional Gabor-like kernel at a given angle and scale.
/// This approximates a curvelet wedge in the spatial domain.
fn build_directional_kernel(size: usize, angle: f32, wavelength: f32) -> Vec<f32> {
    let mut kernel = vec![0.0f32; size * size];
    let center = size as f32 / 2.0;
    let sigma = wavelength / 3.0; // Gaussian envelope width
    let cos_a = angle.cos();
    let sin_a = angle.sin();

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;

            // Rotate coordinates to align with filter direction
            let x_rot = dx * cos_a + dy * sin_a;
            let y_rot = -dx * sin_a + dy * cos_a;

            // Gaussian envelope (elongated along the filter direction)
            let sigma_x = sigma;
            let sigma_y = sigma * 0.4; // narrower perpendicular to direction
            let gaussian = (-0.5 * ((x_rot / sigma_x).powi(2) + (y_rot / sigma_y).powi(2))).exp();

            // Sinusoidal carrier (detects periodic structure at this scale)
            let carrier = (2.0 * PI * x_rot / wavelength).cos();

            kernel[y * size + x] = gaussian * carrier;
        }
    }

    // Normalize: zero-mean, unit energy
    let mean: f32 = kernel.iter().sum::<f32>() / kernel.len() as f32;
    for val in kernel.iter_mut() {
        *val -= mean;
    }
    let energy: f32 = kernel.iter().map(|v| v * v).sum::<f32>().sqrt();
    if energy > 1e-10 {
        for val in kernel.iter_mut() {
            *val /= energy;
        }
    }

    kernel
}

/// 2D convolution (spatial domain, CPU fallback).
/// For GPU dispatch, this would be replaced by a WGSL compute shader.
fn convolve_2d(
    input: &[f32],
    img_w: usize,
    img_h: usize,
    kernel: &[f32],
    kern_w: usize,
    kern_h: usize,
) -> Vec<f32> {
    let mut output = vec![0.0f32; img_w * img_h];
    let kx_half = kern_w / 2;
    let ky_half = kern_h / 2;

    for y in 0..img_h {
        for x in 0..img_w {
            let mut sum = 0.0f32;

            for ky in 0..kern_h {
                for kx in 0..kern_w {
                    let ix = x as isize + kx as isize - kx_half as isize;
                    let iy = y as isize + ky as isize - ky_half as isize;

                    // Clamp to edges (replicate border)
                    let ix = ix.clamp(0, img_w as isize - 1) as usize;
                    let iy = iy.clamp(0, img_h as isize - 1) as usize;

                    sum += input[iy * img_w + ix] * kernel[ky * kern_w + kx];
                }
            }

            output[y * img_w + x] = sum;
        }
    }

    output
}

/// Estimate noise floor using median absolute deviation (robust to outliers).
fn estimate_noise_floor(data: &[f32]) -> f32 {
    if data.is_empty() {
        return 0.0;
    }

    // Use a sample for large arrays (performance)
    let sample_size = data.len().min(10000);
    let step = data.len() / sample_size;

    let mut samples: Vec<f32> = data.iter()
        .step_by(step.max(1))
        .copied()
        .filter(|v| *v > 0.0)
        .collect();

    if samples.is_empty() {
        return 0.0;
    }

    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = samples[samples.len() / 2];

    // MAD (Median Absolute Deviation)
    let mut deviations: Vec<f32> = samples.iter().map(|v| (v - median).abs()).collect();
    deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mad = deviations[deviations.len() / 2];

    // Convert MAD to standard deviation estimate (factor 1.4826 for normal distribution)
    mad * 1.4826
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_bank_builds() {
        let config = CurveletConfig::default();
        let bank = CurveletFilterBank::build(config.clone());
        assert_eq!(bank.kernels.len(), config.n_scales * config.n_angles);
        println!("Filter bank memory: {} KB", bank.memory_bytes() / 1024);
    }

    #[test]
    fn test_filter_enhances_line() {
        let config = CurveletConfig {
            n_scales: 2,
            n_angles: 4,
            min_feature_px: 3.0,
            max_feature_px: 10.0,
            threshold_multiplier: 1.5,
        };
        let bank = CurveletFilterBank::build(config);

        // Create a 64x64 image with a horizontal line
        let mut input = vec![0.0f32; 64 * 64];
        for x in 10..54 {
            input[32 * 64 + x] = 1.0; // horizontal line at y=32
        }

        let output = bank.apply(&input, 64, 64);

        // The line region should have higher response than background
        let line_response: f32 = (10..54).map(|x| output[32 * 64 + x]).sum::<f32>();
        let bg_response: f32 = (10..54).map(|x| output[10 * 64 + x]).sum::<f32>();
        assert!(line_response > bg_response * 2.0, "Line should be enhanced vs background");
    }
}
