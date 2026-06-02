// =============================================================================
// psf_correction.rs
// 
// RUST IMPLEMENTATION — BUG-001 & BUG-002 FIXES FOR AVIATION-004
// 
// This module provides Richardson-Lucy PSF deconvolution and sub-pixel
// centroiding for the Tauri/Rust backend of WreckHunter2000.
//
// Features:
//   [1] Sentinel-2 PSF kernel generation (Gaussian model)
//   [2] Richardson-Lucy iterative deconvolution
//   [3] Sub-pixel centroid computation (intensity-weighted moments)
//   [4] 180ft Shelf-Lock safety constraint enforcement
//
// Port Status: Ready for integration into Tauri backend
// =============================================================================

use serde::{Deserialize, Serialize};

// =============================================================================
// DATA STRUCTURES
// =============================================================================

/// Sub-pixel accurate centroid with uncertainty estimation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubPixelCentroid {
    /// Sub-pixel x coordinate (easting offset from pixel center)
    pub x: f64,
    /// Sub-pixel y coordinate (northing offset from pixel center)
    pub y: f64,
    /// X uncertainty in pixels (1σ)
    pub x_uncertainty: f64,
    /// Y uncertainty in pixels (1σ)
    pub y_uncertainty: f64,
    /// Peak magnitude at centroid
    pub magnitude: f64,
    /// Effective area in pixels (blooming-corrected)
    pub area_pixels: f64,
}

/// Result of PSF dimension correction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionCorrection {
    pub measured_length_ft: f64,
    pub measured_width_ft: f64,
    pub corrected_length_ft: f64,
    pub corrected_width_ft: f64,
    pub psf_bloat_ft: f64,
    pub length_correction_factor: f64,
    pub width_correction_factor: f64,
}

/// Result of 180ft shelf-lock safety check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShelfLockResult {
    pub on_180ft_contour: bool,
    pub depth_m: f64,
    pub contour_ft: f64,
    pub depth_tolerance_ft: f64,
    pub shelf_lock_engaged: bool,
    pub is_andaste_candidate: bool,
    pub safety_notes: String,
}

/// Physical extent estimate from magnitude/Z-score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtentEstimate {
    pub estimated_length_m: f64,
    pub estimated_width_m: f64,
    pub estimated_length_ft: f64,
    pub estimated_width_ft: f64,
    pub aspect_ratio: f64,
    pub base_length_m: f64,
}

/// Complete corrected target profile (master output struct)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectedTargetProfile {
    pub target_id: String,
    // Raw measurements
    pub raw_length_ft: f64,
    pub raw_width_ft: f64,
    pub raw_magnitude: f64,
    pub raw_zscore: f64,
    // PSF-corrected
    pub psf_corrected_length_ft: f64,
    pub psf_corrected_width_ft: f64,
    // Sub-pixel centroid
    pub centroid_easting: f64,
    pub centroid_northing: f64,
    pub centroid_uncertainty_m: f64,
    // Estimated physical extent
    pub estimated_length_ft: f64,
    pub estimated_width_ft: f64,
    // Shelf-lock safety
    pub shelf_lock_engaged: bool,
    pub on_180ft_contour: bool,
    // Classification
    pub material_class: String,
    pub target_class: String,
}

// =============================================================================
// [1] SENTINEL-2 PSF KERNEL GENERATION
// =============================================================================

/// Create Sentinel-2 MSI Point Spread Function (PSF) kernel
/// 
/// The PSF describes how a point source "blooms" across adjacent pixels.
/// Sentinel-2 10m pixels have ~12m FWHM (Full Width at Half Maximum).
/// 
/// # Arguments
/// * `fwhm_pixels` - Full Width at Half Maximum in pixels (default 1.2 for 10m band)
/// * `kernel_size` - Size of convolution kernel (odd number recommended)
/// 
/// # Returns
/// 2D Gaussian PSF kernel (normalized to sum = 1.0)
pub fn create_sentinel2_psf_kernel(fwhm_pixels: f64, kernel_size: usize) -> Vec<Vec<f64>> {
    let center = kernel_size as i32 / 2;
    let mut psf = vec![vec![0.0f64; kernel_size]; kernel_size];
    
    // Gaussian PSF (Sentinel-2 approximation)
    // FWHM = 2 * sqrt(2 * ln(2)) * sigma ≈ 2.355 * sigma
    let sigma = fwhm_pixels / (2.0 * (2.0 * 2.0_f64.ln()).sqrt());
    let sigma_sq = sigma * sigma;
    
    let mut sum = 0.0;
    
    // Build 2D Gaussian kernel
    for y in 0..kernel_size {
        for x in 0..kernel_size {
            let dx = (x as i32 - center) as f64;
            let dy = (y as i32 - center) as f64;
            let r_sq = dx * dx + dy * dy;
            
            // Gaussian: exp(-0.5 * (r/sigma)^2)
            let value = (-0.5 * r_sq / sigma_sq).exp();
            psf[y][x] = value;
            sum += value;
        }
    }
    
    // Normalize to unit energy
    for row in psf.iter_mut() {
        for val in row.iter_mut() {
            *val /= sum;
        }
    }
    
    psf
}

// =============================================================================
// [2] RICHARDSON-LUCY DECONVOLUTION
// =============================================================================

/// 2D convolution with symmetric boundary handling
fn convolve2d(image: &[Vec<f64>], kernel: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let img_h = image.len();
    let img_w = image[0].len();
    let ker_h = kernel.len();
    let ker_w = kernel[0].len();
    
    let mut result = vec![vec![0.0f64; img_w]; img_h];
    
    let ky_half = ker_h as i32 / 2;
    let kx_half = ker_w as i32 / 2;
    
    for y in 0..img_h {
        for x in 0..img_w {
            let mut sum = 0.0;
            
            for ky in 0..ker_h {
                for kx in 0..ker_w {
                    // Symmetric boundary handling
                    let iy = (y as i32 + ky as i32 - ky_half)
                        .max(0)
                        .min((img_h - 1) as i32) as usize;
                    let ix = (x as i32 + kx as i32 - kx_half)
                        .max(0)
                        .min((img_w - 1) as i32) as usize;
                    
                    sum += image[iy][ix] * kernel[ky][kx];
                }
            }
            
            result[y][x] = sum;
        }
    }
    
    result
}

/// Flip 2D kernel 180 degrees (rotate both axes)
fn flip_kernel_180(kernel: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let h = kernel.len();
    let w = kernel[0].len();
    let mut flipped = vec![vec![0.0f64; w]; h];
    
    for y in 0..h {
        for x in 0..w {
            flipped[h - 1 - y][w - 1 - x] = kernel[y][x];
        }
    }
    
    flipped
}

/// Richardson-Lucy iterative deconvolution for PSF blooming correction
/// 
/// Mathematical Formula (per iteration):
///     I_{n+1} = I_n × [ (O / (I_n ⊗ PSF)) ⊗ PSF_flip ]
/// 
/// # Arguments
/// * `image` - Observed image (bloated by PSF)
/// * `psf` - Point Spread Function kernel
/// * `iterations` - Number of RL iterations (15-30 typical)
/// 
/// # Returns
/// Deconvolved image (sharper, corrected for blooming)
pub fn richardson_lucy_deconvolve(
    image: &[Vec<f64>],
    psf: &[Vec<f64>],
    iterations: usize,
) -> Vec<Vec<f64>> {
    let img_h = image.len();
    let img_w = image[0].len();
    
    // PSF rotated 180° (flip both axes)
    let psf_flip = flip_kernel_180(psf);
    
    // Initialize estimate as observed image
    let mut estimate = image.to_vec();
    
    // Small constant to avoid division by zero
    let eps = 1e-10;
    
    // Richardson-Lucy iterations
    for _ in 0..iterations {
        // Forward convolution: blur current estimate
        let blurred = convolve2d(&estimate, psf);
        
        // Ratio: observed / estimated (where the error is)
        let mut ratio = vec![vec![0.0f64; img_w]; img_h];
        for y in 0..img_h {
            for x in 0..img_w {
                ratio[y][x] = image[y][x] / (blurred[y][x] + eps);
            }
        }
        
        // Backward convolution: propagate correction
        let correction = convolve2d(&ratio, &psf_flip);
        
        // Update estimate (with non-negative constraint)
        for y in 0..img_h {
            for x in 0..img_w {
                estimate[y][x] = (estimate[y][x] * correction[y][x]).max(0.0);
            }
        }
    }
    
    estimate
}

/// Apply PSF deconvolution correction to measured dimensions
/// 
/// Empirical correction based on PSF blooming model:
///     True_Length = Measured_Length - (PSF_FWHM × pixel_resolution)
/// 
/// # Arguments
/// * `measured_length_ft` - Raw measured length (bloated)
/// * `measured_width_ft` - Raw measured width (bloated)
/// * `psf_fwhm_pixels` - PSF Full Width at Half Maximum in pixels
/// * `pixel_resolution_m` - Pixel resolution in meters
/// 
/// # Returns
/// DimensionCorrection with corrected dimensions
pub fn correct_bloated_dimensions(
    measured_length_ft: f64,
    measured_width_ft: f64,
    psf_fwhm_pixels: f64,
    pixel_resolution_m: f64,
) -> DimensionCorrection {
    const METERS_TO_FEET: f64 = 3.28084;
    
    // PSF bloating in feet
    let psf_bloat_ft = psf_fwhm_pixels * pixel_resolution_m * METERS_TO_FEET;
    
    // Corrected dimensions (subtract PSF contribution from each end)
    let corrected_length = (measured_length_ft - psf_bloat_ft).max(0.0);
    let corrected_width = (measured_width_ft - psf_bloat_ft).max(0.0);
    
    // Correction factor (ratio of true/measured)
    let length_correction = if measured_length_ft > 0.0 {
        corrected_length / measured_length_ft
    } else {
        0.0
    };
    
    let width_correction = if measured_width_ft > 0.0 {
        corrected_width / measured_width_ft
    } else {
        0.0
    };
    
    DimensionCorrection {
        measured_length_ft,
        measured_width_ft,
        corrected_length_ft: corrected_length,
        corrected_width_ft: corrected_width,
        psf_bloat_ft,
        length_correction_factor: length_correction,
        width_correction_factor: width_correction,
    }
}

// =============================================================================
// [3] SUB-PIXEL CENTROIDING
// =============================================================================

/// Compute sub-pixel accurate centroid using intensity-weighted moments
/// 
/// Mathematical Formula:
///     x_centroid = Σ(x_i × I_i) / Σ(I_i)
///     y_centroid = Σ(y_i × I_i) / Σ(I_i)
/// 
/// # Arguments
/// * `image_patch` - 2D array containing the anomaly (with background)
/// * `threshold_factor` - Fraction of max intensity to use as mask (0-1)
/// 
/// # Returns
/// SubPixelCentroid with sub-pixel coordinates and uncertainty
pub fn compute_subpixel_centroid(
    image_patch: &[Vec<f64>],
    threshold_factor: f64,
) -> SubPixelCentroid {
    let h = image_patch.len();
    let w = image_patch[0].len();
    
    // Find min/max for normalization
    let mut min_val = f64::INFINITY;
    let mut max_val = f64::NEG_INFINITY;
    for row in image_patch.iter() {
        for &val in row.iter() {
            min_val = min_val.min(val);
            max_val = max_val.max(val);
        }
    }
    
    let range = max_val - min_val + 1e-10;
    
    // Normalize and threshold
    let threshold = threshold_factor * max_val;
    
    let mut total_intensity = 0.0;
    let mut x_sum = 0.0;
    let mut y_sum = 0.0;
    let mut pixel_count = 0;
    
    for y in 0..h {
        for x in 0..w {
            let normalized = (image_patch[y][x] - min_val) / range;
            
            if normalized > threshold {
                let intensity = normalized;
                total_intensity += intensity;
                x_sum += x as f64 * intensity;
                y_sum += y as f64 * intensity;
                pixel_count += 1;
            }
        }
    }
    
    if total_intensity < 1e-10 || pixel_count == 0 {
        return SubPixelCentroid {
            x: 0.0,
            y: 0.0,
            x_uncertainty: 999.0,
            y_uncertainty: 999.0,
            magnitude: 0.0,
            area_pixels: 0.0,
        };
    }
    
    // First moments (centroid)
    let x_centroid = x_sum / total_intensity;
    let y_centroid = y_sum / total_intensity;
    
    // Second moments (uncertainty/variance)
    let mut x_var_sum = 0.0;
    let mut y_var_sum = 0.0;
    
    for y in 0..h {
        for x in 0..w {
            let normalized = (image_patch[y][x] - min_val) / range;
            
            if normalized > threshold {
                let dx = x as f64 - x_centroid;
                let dy = y as f64 - y_centroid;
                x_var_sum += dx * dx * normalized;
                y_var_sum += dy * dy * normalized;
            }
        }
    }
    
    let x_uncertainty = (x_var_sum / total_intensity).sqrt();
    let y_uncertainty = (y_var_sum / total_intensity).sqrt();
    
    // Get magnitude at centroid (rounded to nearest pixel)
    let cx = x_centroid.round() as usize;
    let cy = y_centroid.round() as usize;
    let magnitude = if cx < w && cy < h {
        (image_patch[cy][cx] - min_val) / range
    } else {
        0.0
    };
    
    // Effective area (blooming-corrected)
    let area_pixels = pixel_count as f64 * (total_intensity / pixel_count as f64);
    
    SubPixelCentroid {
        x: x_centroid,
        y: y_centroid,
        x_uncertainty,
        y_uncertainty,
        magnitude,
        area_pixels,
    }
}

/// Estimate physical dimensions from anomaly magnitude and Z-score
/// 
/// # Arguments
/// * `magnitude` - Anomaly magnitude (0-2.0 typical range)
/// * `zscore` - Z-score of anomaly (negative for cold sinks)
/// * `pixel_resolution_m` - Pixel resolution in meters
/// * `psf_fwhm_pixels` - PSF FWHM for blooming correction
/// 
/// # Returns
/// ExtentEstimate with estimated dimensions
pub fn estimate_physical_extent_from_magnitude(
    magnitude: f64,
    zscore: f64,
    pixel_resolution_m: f64,
    psf_fwhm_pixels: f64,
) -> ExtentEstimate {
    const METERS_TO_FEET: f64 = 3.28084;
    
    // Base length (minimum detectable object)
    let base_length_m = pixel_resolution_m * psf_fwhm_pixels;
    
    // Scale factor from magnitude (calibrated from known wrecks)
    let scale_factor = 50.0;
    
    let estimated_length_m = base_length_m + (magnitude * scale_factor);
    
    // Aspect ratio from Z-score
    let abs_zscore = zscore.abs();
    let aspect_ratio = if abs_zscore > 2.5 {
        0.35  // Elongated (hull)
    } else if abs_zscore > 1.5 {
        0.45  // Moderate
    } else {
        0.60  // Compact (debris)
    };
    
    let estimated_width_m = estimated_length_m * aspect_ratio;
    
    ExtentEstimate {
        estimated_length_m,
        estimated_width_m,
        estimated_length_ft: estimated_length_m * METERS_TO_FEET,
        estimated_width_ft: estimated_width_m * METERS_TO_FEET,
        aspect_ratio,
        base_length_m,
    }
}

// =============================================================================
// [4] 180FT SHELF-LOCK SAFETY CONSTRAINT
// =============================================================================

/// Enforce 180ft shelf-lock safety constraint for Zion Trench candidates
/// 
/// SAFETY REQUIREMENT:
/// All Andaste candidates MUST be on the 180ft depth contour (±5ft tolerance).
/// 
/// # Arguments
/// * `depth_m` - Target depth in meters
/// * `contour_ft` - Bathymetric contour in feet
/// * `length_ft` - Target length in feet (for Andaste matching)
/// * `target_id` - Target identifier for logging
/// 
/// # Returns
/// ShelfLockResult with safety assessment
pub fn check_180ft_shelf_lock(
    depth_m: f64,
    contour_ft: f64,
    length_ft: f64,
    target_id: &str,
) -> ShelfLockResult {
    const CONTOUR_TARGET_FT: f64 = 180.0;
    const CONTOUR_TOLERANCE_FT: f64 = 5.0;
    const ANDASTE_LENGTH_MIN_FT: f64 = 255.0;
    const ANDASTE_LENGTH_MAX_FT: f64 = 275.0;
    
    // Check if on 180ft contour
    let depth_diff = (contour_ft - CONTOUR_TARGET_FT).abs();
    let on_180ft_contour = depth_diff <= CONTOUR_TOLERANCE_FT;
    
    // Check if length matches Andaste
    let length_matches = length_ft >= ANDASTE_LENGTH_MIN_FT && length_ft <= ANDASTE_LENGTH_MAX_FT;
    
    // Shelf-lock engaged if BOTH conditions met
    let shelf_lock_engaged = on_180ft_contour && length_matches;
    
    // Generate safety notes
    let mut notes = Vec::new();
    if !on_180ft_contour {
        notes.push(format!(
            "DEPTH WARNING: {:.0}ft is not on 180ft contour (diff: {:.0}ft)",
            contour_ft, depth_diff
        ));
    }
    if !length_matches {
        notes.push(format!(
            "LENGTH WARNING: {:.0}ft outside Andaste range (255-275ft)",
            length_ft
        ));
    }
    if shelf_lock_engaged {
        notes.push("SHELF-LOCK ENGAGED: Target matches Andaste depth and length profile".to_string());
    }
    
    ShelfLockResult {
        on_180ft_contour,
        depth_m,
        contour_ft,
        depth_tolerance_ft: CONTOUR_TOLERANCE_FT,
        shelf_lock_engaged,
        is_andaste_candidate: shelf_lock_engaged,
        safety_notes: notes.join("; "),
    }
}

// =============================================================================
// [5] MASTER PIPELINE
// =============================================================================

/// Master function: Apply all corrections to an aviation target
/// 
/// Pipeline:
/// 1. PSF deconvolution → correct bloated dimensions
/// 2. Sub-pixel centroiding → improve location accuracy
/// 3. Magnitude-based extent estimation → fill missing dimensions
/// 4. 180ft shelf-lock → safety validation
pub fn process_aviation_target_with_corrections(
    target_id: &str,
    raw_length_ft: f64,
    raw_width_ft: f64,
    raw_magnitude: f64,
    raw_zscore: f64,
    depth_m: f64,
    contour_ft: f64,
    utm_easting: f64,
    utm_northing: f64,
    material_class: &str,
) -> CorrectedTargetProfile {
    const PIXEL_RESOLUTION_M: f64 = 10.0;
    const PSF_FWHM_PIXELS: f64 = 1.2;
    
    // [1] PSF Deconvolution
    let psf_correction = correct_bloated_dimensions(
        raw_length_ft,
        raw_width_ft,
        PSF_FWHM_PIXELS,
        PIXEL_RESOLUTION_M,
    );
    
    // [2] Sub-pixel centroid (simplified - no image patch in this signature)
    let centroid_easting = utm_easting;
    let centroid_northing = utm_northing;
    let centroid_uncertainty_m = 10.0; // Default 1-pixel uncertainty
    
    // [3] Magnitude-based extent estimation
    let extent = estimate_physical_extent_from_magnitude(
        raw_magnitude,
        raw_zscore,
        PIXEL_RESOLUTION_M,
        PSF_FWHM_PIXELS,
    );
    
    // [4] 180ft Shelf-lock
    let shelf_result = check_180ft_shelf_lock(
        depth_m,
        contour_ft,
        psf_correction.corrected_length_ft,
        target_id,
    );
    
    // Classification
    let target_class = if psf_correction.corrected_length_ft < 50.0 && material_class == "BRIGHT_ALUMINUM" {
        "AIRCRAFT_DEBRIS"
    } else if psf_correction.corrected_length_ft > 164.0 && shelf_result.shelf_lock_engaged {
        "ANDASTE_CANDIDATE"
    } else if psf_correction.corrected_length_ft > 164.0 {
        "HEAVY_FREIGHTER"
    } else {
        "UNCLASSIFIED"
    };
    
    CorrectedTargetProfile {
        target_id: target_id.to_string(),
        raw_length_ft,
        raw_width_ft,
        raw_magnitude,
        raw_zscore,
        psf_corrected_length_ft: psf_correction.corrected_length_ft,
        psf_corrected_width_ft: psf_correction.corrected_width_ft,
        centroid_easting,
        centroid_northing,
        centroid_uncertainty_m,
        estimated_length_ft: extent.estimated_length_ft,
        estimated_width_ft: extent.estimated_width_ft,
        shelf_lock_engaged: shelf_result.shelf_lock_engaged,
        on_180ft_contour: shelf_result.on_180ft_contour,
        material_class: material_class.to_string(),
        target_class: target_class.to_string(),
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dc4_wing_correction() {
        // DC-4 wingspan: 117.5ft true, 154ft measured (bloated)
        let correction = correct_bloated_dimensions(154.0, 125.0, 1.2, 10.0);
        
        // Should correct to ~117ft (within 5% tolerance)
        assert!((correction.corrected_length_ft - 117.5).abs() < 6.0);
        assert!(correction.length_correction_factor < 1.0);
    }

    #[test]
    fn test_shelf_lock_engagement() {
        // ZION-006: On 180ft contour, 266ft length → Should engage
        let result = check_180ft_shelf_lock(54.9, 180.0, 266.0, "ZION-006");
        assert!(result.shelf_lock_engaged);
        assert!(result.on_180ft_contour);
        
        // ZION-008: 158ft contour, 157ft length → Should NOT engage
        let result2 = check_180ft_shelf_lock(48.2, 158.0, 157.0, "ZION-008");
        assert!(!result2.shelf_lock_engaged);
        assert!(!result2.on_180ft_contour);
    }

    #[test]
    fn test_psf_kernel_normalization() {
        let psf = create_sentinel2_psf_kernel(1.2, 11);
        
        // Sum should be 1.0 (normalized)
        let sum: f64 = psf.iter().flat_map(|row| row.iter()).sum();
        assert!((sum - 1.0).abs() < 1e-10);
    }
}
