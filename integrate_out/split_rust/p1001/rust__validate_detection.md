# integrate/unmapped/laptopdump_wreckhunter_build/validate_detection.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/thermal_validation.rs

## Rust source
```rust
//! Thermal anomaly repeatability validation module
//!
//! Re-processes Landsat 8 thermal bands (B10/B11) to validate
//! whether a previously detected anomaly is repeatable across
//! multiple processing runs.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, Utc};
use image::{DynamicImage, ImageBuffer, ImageFormat};
use ndarray::{Array, Array2, Axis};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Configuration for thermal validation processing
#[derive(Debug, Clone, Deserialize)]
pub struct ThermalValidationConfig {
    /// Target latitude for validation
    pub target_lat: f64,
    /// Target longitude for validation
    pub target_lon: f64,
    /// Original z-score from initial detection
    pub original_zscore: f64,
    /// Base tile identifier
    pub tile_base: String,
    /// Directory containing raw thermal bands
    pub tile_dir: PathBuf,
    /// Output directory for validation results
    pub output_dir: PathBuf,
    /// Z-score threshold for anomaly detection
    pub zscore_threshold: f64,
    /// Tolerance for repeatability check (as fraction of original)
    pub repeatability_tolerance: f64,
}

impl Default for ThermalValidationConfig {
    fn default() -> Self {
        Self {
            target_lat: 42.948873,
            target_lon: -86.976619,
            original_zscore: 5.46,
            tile_base: "HLS.L30.T16TDN.2021198T162826.v2.0".to_string(),
            tile_dir: PathBuf::from("wreckhunter2000/data/cache/census_raw/2021_low_water"),
            output_dir: PathBuf::from("outputs/validation_test"),
            zscore_threshold: 2.5,
            repeatability_tolerance: 0.8,
        }
    }
}

/// Errors that can occur during thermal validation
#[derive(Debug, Error)]
pub enum ThermalValidationError {
    #[error("Failed to load thermal band: {0}")]
    LoadBandError(String),
    #[error("Missing thermal band file: {0}")]
    MissingBandError(String),
    #[error("Failed to convert image to array: {0}")]
    ConversionError(String),
    #[error("Invalid z-score calculation: {0}")]
    ZScoreError(String),
    #[error("Output directory creation failed: {0}")]
    OutputDirError(String),
}

/// Result of thermal validation processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalValidationResult {
    /// Mean brightness temperature
    pub mean_brightness: f32,
    /// Standard deviation of brightness
    pub std_brightness: f32,
    /// Maximum absolute z-score in tile
    pub max_zscore: f32,
    /// Number of anomalies detected
    pub anomaly_count: usize,
    /// Z-score map (flattened for storage)
    pub zscore_map: Vec<f32>,
    /// Validation pass/fail status
    pub validation_status: ValidationStatus,
    /// Timestamp of validation
    pub timestamp: DateTime<Utc>,
}

/// Status of validation result
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationStatus {
    /// Anomaly is repeatable and real
    Pass,
    /// Anomaly is not repeatable, may be noise
    Fail,
}

impl ThermalValidationResult {
    /// Create a new validation result
    pub fn new(
        mean_brightness: f32,
        std_brightness: f32,
        max_zscore: f32,
        anomaly_count: usize,
        zscore_map: Vec<f32>,
        validation_status: ValidationStatus,
    ) -> Self {
        Self {
            mean_brightness,
            std_brightness,
            max_zscore,
            anomaly_count,
            zscore_map,
            validation_status,
            timestamp: Utc::now(),
        }
    }
}

/// Load a specific thermal band from disk
fn load_band(band: &str, tile_dir: &Path) -> Result<(Array2<f32>, PathBuf), ThermalValidationError> {
    let tif_path = tile_dir.join(format!("{}.tif", band));
    
    if !tif_path.exists() {
        return Err(ThermalValidationError::MissingBandError(tif_path.display().to_string()));
    }
    
    // Load image using image crate
    let img = match DynamicImage::open(&tif_path) {
        Ok(img) => img,
        Err(e) => {
            return Err(ThermalValidationError::LoadBandError(format!(
                "Failed to open {}: {}",
                tif_path.display(),
                e
            )))
        }
    };
    
    // Convert to f32 array
    let array = match img.to_rgb8() {
        DynamicImage::Rgb8(rgb) => {
            let pixels: Vec<u8> = rgb.into_raw();
            let width = rgb.width() as usize;
            let height = rgb.height() as usize;
            
            // Convert u8 to f32 (assuming 0-255 maps to 0.0-1.0)
            let mut data = Vec::with_capacity(width * height);
            for &pixel in &pixels {
                data.push((pixel as f32) / 255.0);
            }
            
            Array::from_shape_vec((height, width), data)
                .map_err(|e| ThermalValidationError::ConversionError(e.to_string()))?
        }
        _ => {
            return Err(ThermalValidationError::ConversionError(
                "Unsupported image format".to_string(),
            ))
        }
    };
    
    Ok((array, tif_path))
}

/// Process thermal bands to detect anomalies
fn process_thermal(
    b10_data: &Array2<f32>,
    b11_data: &Array2<f32>,
) -> Result<ThermalValidationResult, ThermalValidationError> {
    // Calculate brightness temperature (simplified average)
    let brightness_temp = (b10_data + b11_data) / 2.0;
    
    // Calculate statistics
    let mean_val = brightness_temp.mean();
    let std_val = brightness_temp.std();
    
    // Calculate z-scores with numerical stability
    let zscore = (brightness_temp - mean_val) / (std_val + 1e-6);
    
    // Find anomalies
    let anomalies = zscore.map(|z| z.abs() > 2.5);
    let anomaly_count = anomalies.sum::<usize>();
    
    // Get max absolute z-score
    let max_zscore = zscore.map(|z| z.abs()).max().unwrap_or(0.0);
    
    // Flatten z-score map for storage
    let zscore_map = zscore.into_raw_vec().to_vec();
    
    // Determine validation status
    let validation_status = if max_zscore >= 2.5 {
        ValidationStatus::Pass
    } else {
        ValidationStatus::Fail
    };
    
    Ok(ThermalValidationResult::new(
        mean_val as f32,
        std_val as f32,
        max_zscore as f32,
        anomaly_count,
        zscore_map,
        validation_status,
    ))
}

/// Main validation function
pub fn validate_thermal_anomaly(config: &ThermalValidationConfig) -> Result<(), ThermalValidationError> {
    // Create output directory
    let output_dir = &config.output_dir;
    std::fs::create_dir_all(output_dir)
        .map_err(|e| ThermalValidationError::OutputDirError(e.to_string()))?;
    
    // Load thermal bands
    let (b10_data, b10_path) = load_band("B10", &config.tile_dir)?;
    let (b11_data, b11_path) = load_band("B11", &config.tile_dir)?;
    
    println!("[1/3] Loaded thermal bands");
    println!("  ✓ B10: {} ({})", b10_path.file_name().unwrap().to_string_lossy(), b10_data.shape());
    println!("  ✓ B11: {} ({})", b11_path.file_name().unwrap().to_string_lossy(), b11_data.shape());
    println!();
    
    // Process thermal data
    let result = process_thermal(&b10_data, &b11_data)?;
    
    println!("[2/3] Processed thermal data");
    println!("  Mean brightness: {:.2}", result.mean_brightness);
    println!("  Std deviation: {:.2}", result.std_brightness);
    println!("  Max Z-score: {:.2}", result.max_zscore);
    println!("  Anomalies found: {}", result.anomaly_count);
    println!();
    
    // Check repeatability
    println!("[3/3] Checking repeatability");
    println!("  Original Z-score: {}", config.original_zscore);
    println!("  Max Z-score in tile: {:.2}", result.max_zscore);
    println!();
    
    // Determine if repeatable
    let is_repeatable = result.max_zscore >= config.original_zscore * config.repeatability_tolerance;
    
    println!("=");
    println!("VALIDATION RESULT");
    println!("=");
    
    if is_repeatable {
        println!("  ✓ REPEATABLE!");
        println!("    Max Z-score ({:.2}) is close to original ({})", 
                 result.max_zscore, config.original_zscore);
        println!("    Anomaly is REAL - appears in re-processing");
        println!();
        println!("  Validation: {}", result.validation_status);
    } else {
        println!("  ✗ NOT REPEATABLE");
        println!("    Max Z-score ({:.2}) is much lower than original ({})", 
                 result.max_zscore, config.original_zscore);
        println!("    Original detection may have been noise");
        println!();
        println!("  Validation: {}", result.validation_status);
    }
    
    // Save results
    let output_file = output_dir.join(format!(
        "validation_{}.json",
        config.tile_base
    ));
    
    let output_data = serde_json::json!({
        "target_lat": config.target_lat,
        "target_lon": config.target_lon,
        "original_zscore": config.original_zscore,
        "tile": config.tile_base,
        "validation_result": match result.validation_status {
            ValidationStatus::Pass => "PASS",
            ValidationStatus::Fail => "FAIL",
        },
        "max_zscore": result.max_zscore,
        "anomaly_count": result.anomaly_count,
        "timestamp": result.timestamp.to_rfc3339(),
    });
    
    std::fs::write(&output_file, serde_json::to_string_pretty(&output_data)?)
        .map_err(|e| ThermalValidationError::OutputDirError(e.to_string()))?;
    
    println!();
    println!("Results saved to: {}", output_file.display());
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_process_thermal() {
        // Create simple test arrays
        let b10 = Array2::from_shape_vec((10, 10), vec![1.0; 100]).unwrap();
        let b11 = Array2::from_shape_vec((10, 10), vec![2.0; 100]).unwrap();
        
        let result = process_thermal(&b10, &b11).unwrap();
        
        assert!(result.mean_brightness > 0.0);
        assert!(result.std_brightness > 0.0);
        assert!(result.max_zscore > 0.0);
        assert!(result.anomaly_count >= 0);
        assert!(result.zscore_map.len() > 0);
    }
}
```

## Forge wire
- **Pipeline Integration**: Called from `cesarops-inference/src/pipeline/thermal_validation_pipeline.rs` after initial anomaly detection
- **Config Loading**: Reads `thermal_validation_config.json` from pipeline config directory
- **Output Handling**: Results written to `outputs/validation_test/validation_<tile>.json` for downstream analysis
- **Error Propagation**: Errors bubble up to pipeline orchestrator for retry or skip decisions

## Risks
- **GeoTIFF Format**: Relies on `image` crate which may not handle all GeoTIFF variants (compression, metadata)
- **Numerical Stability**: Z-score calculation uses `1e-6` floor to prevent division by zero, but extreme values could still cause issues
- **Memory Usage**: Flattening z-score map to Vec<f32> for JSON serialization could be memory-intensive for large tiles
- **Coordinate System**: Simplified geotransform handling in Python was removed; actual pixel-to-latlon conversion requires proper CRS handling
- **Band Calibration**: Simplified brightness temperature calculation doesn't use Landsat 8 calibration constants
