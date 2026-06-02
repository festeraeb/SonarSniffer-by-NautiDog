// CESAROPS Curvelet Transform - Real Implementation
// Discrete Curvelet Transform using FFT-based ridgelet analysis
// For multi-scale geometric analysis of satellite imagery

use ndarray::{Array2, Array3, s};
use rustfft::{FftPlanner, num_complex::Complex};
use rayon::prelude::*;

/// Anomaly detection result from curvelet analysis
#[derive(Debug, Clone)]
pub struct Anomaly {
    pub x: usize,
    pub y: usize,
    pub magnitude: f64,
    pub scale: usize,
    pub orientation: usize,
}

/// Curvelet Transform for multi-scale geometric analysis
/// 
/// Implements a discrete curvelet transform using:
/// 1. FFT-based decomposition
/// 2. Multi-scale partitioning (dyadic scales)
/// 3. Directional decomposition at each scale
/// 
/// Curvelets excel at representing edges and geometric structures
/// in satellite imagery, making them ideal for detecting
/// linear anomalies like shipwrecks and aircraft.
pub struct CurveletTransform {
    num_scales: usize,
    num_orientations: Vec<usize>,
    coefficients: Vec<Array3<f64>>,
    image_dims: Option<(usize, usize)>,
    fft_planner: FftPlanner<f64>,
}

impl CurveletTransform {
    /// Create a new curvelet transform with specified scales and orientations
    /// 
    /// # Arguments
    /// * `num_scales` - Number of dyadic scales (typically 4-5)
    /// * `base_orientations` - Base number of orientations at coarsest scale
    pub fn new(num_scales: usize, base_orientations: usize) -> Self {
        // Orientations double at each finer scale (dyadic parabolic scaling)
        let mut num_orientations = Vec::with_capacity(num_scales);
        for scale in 0..num_scales {
            num_orientations.push(base_orientations * (1 << scale));
        }

        Self {
            num_scales,
            num_orientations,
            coefficients: Vec::new(),
            image_dims: None,
            fft_planner: FftPlanner::new(),
        }
    }

    /// Forward curvelet transform
    /// 
    /// Decomposes the image into curvelet coefficients at multiple
    /// scales and orientations.
    /// 
    /// # Algorithm
    /// 1. Apply 2D FFT to input image
    /// 2. For each scale:
    ///    - Apply bandpass filter in frequency domain
    ///    - For each orientation:
    ///      - Apply directional wedge filter
    ///      - Inverse FFT to get spatial coefficients
    /// 
    /// # Arguments
    /// * `image` - Input image as 2D array
    pub fn forward(&mut self, image: &Array2<f64>) -> Result<(), String> {
        let (height, width) = image.dim();
        self.image_dims = Some((height, width));
        self.coefficients.clear();

        // Pad image to next power of 2 for efficient FFT
        let padded_width = width.next_power_of_two();
        let padded_height = height.next_power_of_two();

        // Create padded and windowed image
        let mut padded = Array2::zeros((padded_height, padded_width));
        let window = hann_window_2d(height, width);
        
        for row in 0..height {
            for col in 0..width {
                padded[[row, col]] = image[[row, col]] * window[[row, col]];
            }
        }

        // Apply 2D FFT
        let fft_result = fft2d_2d(&padded, &mut self.fft_planner);

        // Decompose into scales and orientations
        // We need to extract data first to avoid borrow conflicts
        let scales_data: Vec<(usize, usize)> = (0..self.num_scales)
            .map(|scale| (scale, self.num_orientations[scale]))
            .collect();

        for (scale, num_orients) in scales_data {
            let scale_coeffs = Self::decompose_scale_static(
                &fft_result,
                scale,
                num_orients,
                padded_width,
                padded_height,
                &mut self.fft_planner,
            );
            self.coefficients.push(scale_coeffs);
        }

        Ok(())
    }

    /// Decompose FFT result into curvelet coefficients at one scale (static version)
    fn decompose_scale_static(
        fft_data: &Array2<Complex<f64>>,
        scale: usize,
        num_orientations: usize,
        width: usize,
        height: usize,
        fft_planner: &mut FftPlanner<f64>,
    ) -> Array3<f64> {
        let num_orientations_half = num_orientations / 2;
        let mut coeffs = Array3::zeros((num_orientations_half, height, width));

        // Frequency bounds for this scale (dyadic)
        let freq_min = (width as f64 / (2.0f64.powi(scale as i32 + 1) as f64)) as usize;
        let freq_max = (width as f64 / (2.0f64.powi(scale as i32) as f64)) as usize;

        // Create angular wedges
        for orient_idx in 0..num_orientations_half {
            let angle_step = std::f64::consts::PI / num_orientations_half as f64;
            let center_angle = orient_idx as f64 * angle_step;

            // Apply wedge filter in frequency domain
            let filtered = Self::apply_wedge_filter_static(
                fft_data,
                center_angle,
                angle_step,
                freq_min,
                freq_max,
                width,
                height,
            );

            // Inverse FFT to get spatial coefficients
            let spatial = ifft2d_2d(&filtered, fft_planner);

            // Take magnitude
            for row in 0..height {
                for col in 0..width {
                    coeffs[[orient_idx, row, col]] = spatial[[row, col]].norm();
                }
            }
        }

        coeffs
    }

    /// Apply directional wedge filter in frequency domain (static version)
    #[allow(clippy::too_many_arguments)]
    fn apply_wedge_filter_static(
        fft_data: &Array2<Complex<f64>>,
        center_angle: f64,
        angle_width: f64,
        freq_min: usize,
        freq_max: usize,
        width: usize,
        height: usize,
    ) -> Array2<Complex<f64>> {
        let mut filtered = Array2::from_elem((height, width), Complex::new(0.0, 0.0));
        let center_x = width as f64 / 2.0;
        let center_y = height as f64 / 2.0;

        for row in 0..height {
            for col in 0..width {
                let fx = (col as f64 - center_x) / center_x;
                let fy = (row as f64 - center_y) / center_y;

                let freq = (fx * fx + fy * fy).sqrt();
                let angle = fy.atan2(fx);

                // Normalize angle to [-PI, PI]
                let mut angle_diff = angle - center_angle;
                while angle_diff > std::f64::consts::PI {
                    angle_diff -= 2.0 * std::f64::consts::PI;
                }
                while angle_diff < -std::f64::consts::PI {
                    angle_diff += 2.0 * std::f64::consts::PI;
                }

                // Radial bandpass (smooth)
                let radial_window = if freq_min > 0 && freq_max > freq_min {
                    let f_norm = freq * width as f64 / 2.0;
                    if f_norm < freq_min as f64 {
                        0.0
                    } else if f_norm > freq_max as f64 {
                        0.0
                    } else {
                        // Smooth transition
                        let t = (f_norm - freq_min as f64) / (freq_max - freq_min) as f64;
                        (std::f64::consts::PI * (t - 0.5)).sin() * 0.5 + 0.5
                    }
                } else {
                    1.0
                };

                // Angular window (smooth)
                let angular_window = if angle_diff.abs() < angle_width {
                    (std::f64::consts::PI * angle_diff / (2.0 * angle_width)).cos().max(0.0)
                } else {
                    0.0
                };

                let weight = radial_window * angular_window;
                filtered[[row, col]] = fft_data[[row, col]] * weight;
            }
        }

        filtered
    }

    /// Detect anomalies from curvelet coefficients
    /// 
    /// Uses statistical thresholding to identify significant
    /// curvelet coefficients that indicate geometric anomalies.
    /// 
    /// # Arguments
    /// * `threshold` - Number of standard deviations above mean
    pub fn detect_anomalies(&self, threshold: f64) -> Vec<Anomaly> {
        let mut anomalies = Vec::new();

        if self.coefficients.is_empty() {
            return anomalies;
        }

        // Combine coefficients across scales and orientations
        let (height, width) = self.image_dims.unwrap_or((0, 0));
        if height == 0 || width == 0 {
            return anomalies;
        }

        let mut combined = Array2::zeros((height, width));

        for scale_coeffs in &self.coefficients {
            // Sum energy across orientations at this scale
            for orient_idx in 0..scale_coeffs.dim().0 {
                for row in 0..height.min(scale_coeffs.dim().1) {
                    for col in 0..width.min(scale_coeffs.dim().2) {
                        combined[[row, col]] += scale_coeffs[[orient_idx, row, col]].powi(2);
                    }
                }
            }
        }

        // Take square root to get magnitude
        combined.mapv_inplace(|v: f64| v.sqrt());

        // Calculate statistics
        let mut sum = 0.0f64;
        let mut count = 0;
        for row in 0..height {
            for col in 0..width {
                let val = combined[[row, col]];
                if val.is_finite() {
                    sum += val;
                    count += 1;
                }
            }
        }

        let mean = if count > 0 { sum / count as f64 } else { 0.0 };

        let mut sum_sq = 0.0f64;
        for row in 0..height {
            for col in 0..width {
                let val = combined[[row, col]];
                if val.is_finite() {
                    sum_sq += (val - mean).powi(2);
                }
            }
        }

        let stddev = if count > 0 { (sum_sq / count as f64).sqrt() } else { 0.0 };
        let threshold_value = mean + threshold * stddev;

        // Find anomalies above threshold
        for row in 0..height {
            for col in 0..width {
                let val = combined[[row, col]];
                if val.is_finite() && val > threshold_value {
                    anomalies.push(Anomaly {
                        x: col,
                        y: row,
                        magnitude: val,
                        scale: 0,
                        orientation: 0,
                    });
                }
            }
        }

        // Sort by magnitude (descending)
        anomalies.sort_by(|a, b| b.magnitude.partial_cmp(&a.magnitude).unwrap_or(std::cmp::Ordering::Equal));

        // Non-maximum suppression (keep only local maxima)
        anomalies = self.non_max_suppression(anomalies);

        anomalies
    }

    /// Non-maximum suppression to keep only local maxima
    fn non_max_suppression(&self, anomalies: Vec<Anomaly>) -> Vec<Anomaly> {
        let suppression_radius = 5;
        let mut result = Vec::new();
        let mut suppressed = vec![false; anomalies.len()];

        for i in 0..anomalies.len() {
            if suppressed[i] {
                continue;
            }

            let anomaly_i = &anomalies[i];
            result.push(anomaly_i.clone());

            // Suppress nearby anomalies
            for j in (i + 1)..anomalies.len() {
                if suppressed[j] {
                    continue;
                }

                let anomaly_j = &anomalies[j];
                let dx = (anomaly_i.x as i32 - anomaly_j.x as i32).abs();
                let dy = (anomaly_i.y as i32 - anomaly_j.y as i32).abs();

                if dx <= suppression_radius && dy <= suppression_radius {
                    suppressed[j] = true;
                }
            }
        }

        result
    }

    /// Get coefficients at a specific scale
    pub fn get_scale_coefficients(&self, scale: usize) -> Option<&Array3<f64>> {
        self.coefficients.get(scale)
    }

    /// Reconstruct image from curvelet coefficients (inverse transform)
    pub fn inverse(&self) -> Option<Array2<f64>> {
        if self.coefficients.is_empty() {
            return None;
        }

        let (height, width) = self.image_dims?;
        let mut reconstructed = Array2::zeros((height, width));

        // Sum coefficients across all scales and orientations
        for scale_coeffs in &self.coefficients {
            for orient_idx in 0..scale_coeffs.dim().0 {
                for row in 0..height.min(scale_coeffs.dim().1) {
                    for col in 0..width.min(scale_coeffs.dim().2) {
                        reconstructed[[row, col]] += scale_coeffs[[orient_idx, row, col]];
                    }
                }
            }
        }

        Some(reconstructed)
    }

    /// Apply curvelet-based denoising
    pub fn denoise(&mut self, threshold: f64) -> Option<Array2<f64>> {
        // Soft threshold coefficients
        for scale_coeffs in &mut self.coefficients {
            for orient_idx in 0..scale_coeffs.dim().0 {
                for row in 0..scale_coeffs.dim().1 {
                    for col in 0..scale_coeffs.dim().2 {
                        let val = scale_coeffs[[orient_idx, row, col]];
                        // Soft thresholding: sign(val) * max(0, |val| - threshold)
                        let thresholded = if val.abs() > threshold {
                            val.signum() * (val.abs() - threshold)
                        } else {
                            0.0
                        };
                        scale_coeffs[[orient_idx, row, col]] = thresholded;
                    }
                }
            }
        }

        self.inverse()
    }
}

/// Generate 2D Hann window
fn hann_window_2d(height: usize, width: usize) -> Array2<f64> {
    let mut window = Array2::zeros((height, width));

    for row in 0..height {
        let w_row = 0.5 * (1.0 - (2.0 * std::f64::consts::PI * row as f64 / (height - 1) as f64).cos());
        for col in 0..width {
            let w_col = 0.5 * (1.0 - (2.0 * std::f64::consts::PI * col as f64 / (width - 1) as f64).cos());
            window[[row, col]] = (w_row * w_col).sqrt();
        }
    }

    window
}

/// 2D FFT using rustfft
fn fft2d_2d(data: &Array2<f64>, planner: &mut FftPlanner<f64>) -> Array2<Complex<f64>> {
    let (height, width) = data.dim();
    let mut complex_data = Array2::from_elem((height, width), Complex::new(0.0, 0.0));

    // Convert to complex
    for row in 0..height {
        for col in 0..width {
            complex_data[[row, col]] = Complex::new(data[[row, col]], 0.0);
        }
    }

    // FFT along rows
    let fft = planner.plan_fft_forward(width);
    for row in 0..height {
        let mut row_data: Vec<Complex<f64>> = complex_data.row(row).to_vec();
        fft.process(&mut row_data);
        for (col, &val) in row_data.iter().enumerate() {
            complex_data[[row, col]] = val;
        }
    }

    // FFT along columns
    let fft_col = planner.plan_fft_forward(height);
    for col in 0..width {
        let mut col_data: Vec<Complex<f64>> = (0..height)
            .map(|row| complex_data[[row, col]])
            .collect();
        fft_col.process(&mut col_data);
        for (row, &val) in col_data.iter().enumerate() {
            complex_data[[row, col]] = val;
        }
    }

    complex_data
}

/// 2D Inverse FFT
fn ifft2d_2d(data: &Array2<Complex<f64>>, planner: &mut FftPlanner<f64>) -> Array2<Complex<f64>> {
    let (height, width) = data.dim();
    let mut complex_data = data.clone();

    // IFFT along rows
    let fft = planner.plan_fft_inverse(width);
    for row in 0..height {
        let mut row_data: Vec<Complex<f64>> = complex_data.row(row).to_vec();
        fft.process(&mut row_data);
        for (col, &val) in row_data.iter().enumerate() {
            complex_data[[row, col]] = val / width as f64;
        }
    }

    // IFFT along columns
    let fft_col = planner.plan_fft_inverse(height);
    for col in 0..width {
        let mut col_data: Vec<Complex<f64>> = (0..height)
            .map(|row| complex_data[[row, col]])
            .collect();
        fft_col.process(&mut col_data);
        for (row, &val) in col_data.iter().enumerate() {
            complex_data[[row, col]] = val / height as f64;
        }
    }

    complex_data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_curvelet_transform() {
        let mut image = Array2::zeros((64, 64));

        // Add a line anomaly (curvelets are good at detecting lines)
        for i in 0..64 {
            image[[32, i]] = 10.0;
        }

        let mut curvelet = CurveletTransform::new(3, 8);
        curvelet.forward(&image).unwrap();

        let anomalies = curvelet.detect_anomalies(3.0);

        assert!(!anomalies.is_empty());
        
        // Most anomalies should be near the line (y=32)
        let near_line = anomalies.iter()
            .filter(|a| (a.y as i32 - 32).abs() <= 5)
            .count();
        assert!(near_line > anomalies.len() / 2);
    }

    #[test]
    fn test_point_anomaly() {
        let mut image = Array2::zeros((64, 64));
        image[[32, 32]] = 100.0; // Point anomaly

        let mut curvelet = CurveletTransform::new(3, 8);
        curvelet.forward(&image).unwrap();

        let anomalies = curvelet.detect_anomalies(3.0);
        assert!(!anomalies.is_empty());
    }

    #[test]
    fn test_hann_window() {
        let window = hann_window_2d(10, 10);
        
        // Center should be maximum
        let center_val = window[[5, 5]];
        let edge_val = window[[0, 0]];
        
        assert!(center_val > edge_val);
        assert!(edge_val < 0.1);
    }
}
