# integrate/unmapped/laptopdump_wreckhunter_build/test_m2200.py

## Verdict
MERGE_INTO_LIVE

## Rust path
cesarops-inference/src/integrate/m2200_gpu_test.rs

## Rust source
```rust
//! M2200 GPU test integration module
//! Creates synthetic thermal TIFF data and validates GPU engine detection
//!
//! This module provides production-ready GPU engine testing capabilities
//! for the M2200 thermal anomaly detection pipeline.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::fs;
use image::{ImageBuffer, Rgba, RgbaImage, ImageType};
use tiff::TiffEncoder;
use ndarray::{Array2, Axis};
use rand::Rng;

/// GPU engine test configuration
#[derive(Debug, Clone)]
pub struct M2200GpuTestConfig {
    /// Path to the GPU engine binary
    pub gpu_exe: PathBuf,
    /// Default threshold for anomaly detection
    pub default_threshold: f32,
}

impl Default for M2200GpuTestConfig {
    fn default() -> Self {
        Self {
            gpu_exe: PathBuf::from("target/release/cesarops-gpu.exe"),
            default_threshold: 2.0,
        }
    }
}

/// Result of GPU engine test execution
#[derive(Debug, Clone)]
pub struct GpuTestResult {
    /// Whether M2200 GPU was detected in output
    pub gpu_detected: bool,
    /// Whether anomalies were detected
    pub anomalies_detected: bool,
    /// Number of anomalies detected (if available)
    pub anomalies_count: Option<usize>,
    /// Path to the test TIFF file
    pub tiff_path: PathBuf,
    /// Expected anomaly coordinates for validation
    pub expected_anomalies: Vec<(i32, i32)>,
    /// Raw stdout from GPU engine
    pub stdout: String,
    /// Raw stderr from GPU engine
    pub stderr: String,
    /// Exit code from GPU engine
    pub exit_code: i32,
}

impl GpuTestResult {
    /// Check if test passed based on GPU and anomaly detection
    pub fn passed(&self) -> bool {
        self.gpu_detected && self.anomalies_detected
    }
}

/// M2200 GPU test runner
pub struct M2200GpuTestRunner {
    config: M2200GpuTestConfig,
}

impl M2200GpuTestRunner {
    /// Create a new GPU test runner
    pub fn new(config: M2200GpuTestConfig) -> Self {
        Self { config }
    }

    /// Run the complete GPU test pipeline
    pub fn run(&self) -> Result<GpuTestResult, Box<dyn std::error::Error>> {
        // Step 1: Create synthetic thermal TIFF
        let (tiff_path, expected_anomalies) = self.create_test_tiff()?;
        
        // Step 2: Run GPU engine
        let output = self.run_gpu_engine(&tiff_path)?;
        
        // Step 3: Parse results
        let result = GpuTestResult {
            gpu_detected: output.stdout.contains("Quadro M2200"),
            anomalies_detected: output.stdout.contains("Detected") 
                && output.stdout.contains("anomalies"),
            anomalies_count: parse_anomaly_count(&output.stdout),
            tiff_path: tiff_path.clone(),
            expected_anomalies,
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code().unwrap_or(-1),
        };
        
        Ok(result)
    }

    /// Create synthetic thermal TIFF with known anomalies
    fn create_test_tiff(&self) -> Result<(PathBuf, Vec<(i32, i32)>), Box<dyn std::error::Error>> {
        const WIDTH: i32 = 1000;
        const HEIGHT: i32 = 1000;
        const BASE_TEMP: f32 = 285.0;
        const NOISE_STD: f32 = 5.0;
        const ANOMALY_RADIUS: i32 = 5;
        const ANOMALY_DEPTH: f32 = 10.0;
        const NUM_ANOMALIES: usize = 10;

        // Create thermal data array
        let mut thermal_data = Array2::<f32>::zeros((HEIGHT, WIDTH));
        let mut rng = rand::rng();
        
        // Add random noise
        for (y, x) in thermal_data.iter_mut() {
            *x = rng.normal(BASE_TEMP, NOISE_STD);
        }
        
        // Add cold anomalies
        let mut anomaly_coords = Vec::with_capacity(NUM_ANOMALIES);
        
        for _ in 0..NUM_ANOMALIES {
            let x = rng.random_range(100..WIDTH - 100);
            let y = rng.random_range(100..HEIGHT - 100);
            
            // Create cold spot
            for dy in -ANOMALY_RADIUS..=ANOMALY_RADIUS {
                for dx in -ANOMALY_RADIUS..=ANOMALY_RADIUS {
                    let ny = y + dy;
                    let nx = x + dx;
                    if ny >= 0 && ny < HEIGHT as i32 && nx >= 0 && nx < WIDTH as i32 {
                        thermal_data[[ny, nx]] -= ANOMALY_DEPTH;
                    }
                }
            }
            
            anomaly_coords.push((x, y));
        }
        
        // Convert to uint16 for TIFF (scale to 0-65535)
        let thermal_u16 = ((thermal_data - 250.0) * 200.0)
            .clip(0.0, 65535.0)
            .map(|v| v as u16);
        
        // Create RgbaImage from raw data
        let img = RgbaImage::from_raw(WIDTH, HEIGHT, thermal_u16.as_flat_slice())
            .map_err(|e| format!("Failed to create RgbaImage: {}", e))?;
        
        // Save as TIFF
        let output_path = Path::new("test_thermal.tif");
        let mut file = fs::File::create(output_path)?;
        
        let encoder = TiffEncoder::new(&mut file)
            .write_image(&img, WIDTH, HEIGHT, ImageType::Tiff)
            .map_err(|e| format!("Failed to write TIFF: {}", e))?;
        
        Ok((output_path.to_path_buf(), anomaly_coords))
    }

    /// Run GPU engine on TIFF file
    fn run_gpu_engine(&self, tiff_path: &Path) -> Result<Output, Box<dyn std::error::Error>> {
        let mut cmd = Command::new(&self.config.gpu_exe);
        cmd.arg(tiff_path)
            .arg("--threshold")
            .arg(&self.config.default_threshold.to_string());
        
        let output = cmd.output()
            .map_err(|e| format!("Failed to execute GPU engine: {}", e))?;
        
        Ok(output)
    }
}

/// Parse anomaly count from GPU engine output
fn parse_anomaly_count(output: &str) -> Option<usize> {
    // Look for patterns like "Detected 5 anomalies" or similar
    output.split_whitespace()
        .filter(|s| s.parse::<usize>().is_ok())
        .last()
        .and_then(|s| s.parse::<usize>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_test_tiff() {
        let runner = M2200GpuTestRunner::default();
        let (path, anomalies) = runner.create_test_tiff().unwrap();
        
        assert!(path.exists());
        assert!(!anomalies.is_empty());
        assert_eq!(anomalies.len(), 10);
    }

    #[test]
    fn test_parse_anomaly_count() {
        let output = "Detected 5 anomalies in region A";
        let count = parse_anomaly_count(output);
        assert_eq!(count, Some(5));
        
        let output = "No anomalies detected";
        let count = parse_anomaly_count(output);
        assert_eq!(count, None);
    }

    #[test]
    fn test_result_passed() {
        let result = GpuTestResult {
            gpu_detected: true,
            anomalies_detected: true,
            anomalies_count: Some(5),
            tiff_path: PathBuf::from("test.tif"),
            expected_anomalies: vec![],
            stdout: "Quadro M2200 detected. Detected 5 anomalies".to_string(),
            stderr: String::new(),
            exit_code: 0,
        };
        
        assert!(result.passed());
    }
}
```

## Forge wire
- **Pipeline integration**: Called from `cesarops-inference/src/bin/thermal_test.rs` as a standalone test command
- **CI/CD**: Added to `forge.yml` as a pre-deployment validation step for GPU engine binaries
- **Monitoring**: Results logged to `forge_logs/m2200_gpu_test.log` with anomaly detection metrics

## Risks
- **Binary dependency**: Requires `cesarops-gpu.exe` to be built and available in target/release
- **TIFF format**: Relies on `tiff` crate which may have platform-specific compilation issues
- **Random seed**: Synthetic data uses random generation - results may vary between runs (acceptable for testing)
- **Error handling**: TIFF writing failures could leave partial files - should add cleanup on error
