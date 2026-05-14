//! Noise reduction filters for sonar imagery.
//!
//! Implements:
//! - **Median filter**: Fast speckle noise removal (3×3, 5×5, 7×7 kernels)
//! - **Bilateral filter**: Edge-preserving smoothing

use crate::video_enhanced::SonarProcessingParams;

/// Apply median filter to 2D image data.
///
/// # Arguments
/// - `data`: Row-major 2D image (height × width)
/// - `width`, `height`: Image dimensions
/// - `kernel_size`: Kernel size (3, 5, or 7)
///
/// # Returns
/// Filtered image (same dimensions)
pub fn median_filter(data: &[f32], width: usize, height: usize, kernel_size: usize) -> Vec<f32> {
    if kernel_size < 3 || kernel_size % 2 == 0 {
        // Invalid kernel size: return copy
        return data.to_vec();
    }
    
    let mut output = vec![0.0f32; data.len()];
    let radius = kernel_size / 2;
    let mut window = Vec::with_capacity(kernel_size * kernel_size);
    
    for y in 0..height {
        for x in 0..width {
            window.clear();
            
            // Collect neighborhood
            for dy in -(radius as isize)..=(radius as isize) {
                for dx in -(radius as isize)..=(radius as isize) {
                    let ny = (y as isize + dy).clamp(0, height as isize - 1) as usize;
                    let nx = (x as isize + dx).clamp(0, width as isize - 1) as usize;
                    window.push(data[ny * width + nx]);
                }
            }
            
            // Compute median
            window.sort_by(|a, b| a.partial_cmp(b).unwrap());
            output[y * width + x] = window[window.len() / 2];
        }
    }
    
    output
}

/// Apply bilateral filter (edge-preserving smoothing).
///
/// # Arguments
/// - `data`: Row-major 2D image
/// - `width`, `height`: Dimensions
/// - `spatial_sigma`: Spatial extent (typically 3-5)
/// - `range_sigma`: Intensity similarity (typically 0.1-0.3)
///
/// # Theory
/// ```text
/// I_out(x) = Σ [w_spatial(x,y) × w_range(x,y) × I(y)] / Σ w(x,y)
/// ```
/// Where:
/// - w_spatial = exp(-||x-y||² / (2σ_s²))
/// - w_range = exp(-(I(x)-I(y))² / (2σ_r²))
pub fn bilateral_filter(
    data: &[f32],
    width: usize,
    height: usize,
    spatial_sigma: f32,
    range_sigma: f32,
) -> Vec<f32> {
    let mut output = vec![0.0f32; data.len()];
    
    // Kernel radius (3 standard deviations)
    let radius = (spatial_sigma * 3.0).ceil() as isize;
    
    let two_spatial_sigma_sq = 2.0 * spatial_sigma * spatial_sigma;
    let two_range_sigma_sq = 2.0 * range_sigma * range_sigma;
    
    for y in 0..height {
        for x in 0..width {
            let center_val = data[y * width + x];
            let mut weighted_sum = 0.0f32;
            let mut weight_sum = 0.0f32;
            
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let ny = (y as isize + dy).clamp(0, height as isize - 1) as usize;
                    let nx = (x as isize + dx).clamp(0, width as isize - 1) as usize;
                    
                    let neighbor_val = data[ny * width + nx];
                    
                    // Spatial weight
                    let spatial_dist_sq = (dx * dx + dy * dy) as f32;
                    let w_spatial = (-spatial_dist_sq / two_spatial_sigma_sq).exp();
                    
                    // Range (intensity) weight
                    let intensity_diff = neighbor_val - center_val;
                    let w_range = (-(intensity_diff * intensity_diff) / two_range_sigma_sq).exp();
                    
                    let weight = w_spatial * w_range;
                    weighted_sum += weight * neighbor_val;
                    weight_sum += weight;
                }
            }
            
            output[y * width + x] = if weight_sum > 0.0 {
                weighted_sum / weight_sum
            } else {
                center_val
            };
        }
    }
    
    output
}

/// Apply filtering based on processing parameters.
pub fn apply_filters(
    data: &[f32],
    width: usize,
    height: usize,
    params: &SonarProcessingParams,
) -> Vec<f32> {
    let mut filtered = data.to_vec();
    
    // Median filter (if enabled)
    if params.median_filter_enabled {
        filtered = median_filter(&filtered, width, height, params.median_kernel_size);
    }
    
    // Bilateral filter (if enabled)
    if params.bilateral_filter_enabled {
        filtered = bilateral_filter(
            &filtered,
            width,
            height,
            params.bilateral_spatial_sigma,
            params.bilateral_range_sigma,
        );
    }
    
    filtered
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_median_filter_removes_outliers() {
        // 3×3 image with center outlier
        let data = vec![
            1.0, 1.0, 1.0,
            1.0, 100.0, 1.0, // Center is outlier
            1.0, 1.0, 1.0,
        ];
        
        let filtered = median_filter(&data, 3, 3, 3);
        
        // Center should be pulled toward 1.0
        assert!(filtered[4] < 10.0);
    }
    
    #[test]
    fn test_bilateral_preserves_edges() {
        // Simple edge: left half = 0, right half = 100
        let mut data = vec![0.0f32; 10 * 10];
        for y in 0..10 {
            for x in 5..10 {
                data[y * 10 + x] = 100.0;
            }
        }
        
        let filtered = bilateral_filter(&data, 10, 10, 2.0, 20.0);
        
        // Edge should remain sharp (minimal blurring across high intensity diff)
        let left_avg = filtered[5 * 10 + 3]; // Left of edge
        let right_avg = filtered[5 * 10 + 6]; // Right of edge
        
        assert!(left_avg < 30.0);
        assert!(right_avg > 70.0);
    }
}
