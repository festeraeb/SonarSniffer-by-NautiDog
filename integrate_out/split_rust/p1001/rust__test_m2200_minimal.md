# integrate/unmapped/laptopdump_wreckhunter_build/test_m2200_minimal.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/gpu_test.rs

## Rust source
```rust
//! GPU test harness for M2200 validation
//! Creates synthetic thermal data and runs GPU inference binary
//!
//! Usage: cargo run --bin cesarops-gpu-test -- --threshold 1.5

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};
use image::imageops::FilterType;
use image::io::Reader;
use log::{debug, info, warn, error};

/// Configuration for GPU test
#[derive(Debug, Clone)]
pub struct GpuTestConfig {
    /// Path to the GPU inference binary
    pub gpu_binary: PathBuf,
    /// Default threshold for anomaly detection
    pub default_threshold: f32,
    /// Test image dimensions
    pub test_width: u32,
    /// Test image dimensions
    pub test_height: u32,
    /// Number of cold anomalies to inject
    pub num_anomalies: usize,
    /// Anomaly temperature offset (Kelvin)
    pub anomaly_offset: f32,
}

impl Default for GpuTestConfig {
    fn default() -> Self {
        Self {
            gpu_binary: PathBuf::from("target/release/cesarops-gpu.exe"),
            default_threshold: 1.5,
            test_width: 100,
            test_height: 100,
            num_anomalies: 5,
            anomaly_offset: 50.0,
        }
    }
}

/// Creates a synthetic thermal image with known anomalies
pub fn create_test_thermal_image(
    width: u32,
    height: u32,
    num_anomalies: usize,
    anomaly_offset: f32,
) -> RgbaImage {
    // Base temperature: 285.0 K (12°C)
    let base_temp = 285.0;
    let temp_scale = 200.0; // 1 unit = 0.005 K
    let min_val = 200.0;
    let max_val = 65535.0;

    // Create image with base temperature
    let mut image = ImageBuffer::new(width, height);
    let base_u16 = ((base_temp - min_val) * temp_scale).clamp(0.0, max_val) as u16;

    for (y, x) in image.pixels_mut() {
        *y = Rgba([base_u16, base_u16, base_u16, 255]);
    }

    // Inject cold anomalies
    let anomaly_radius = 3;
    let anomaly_temp = base_temp - anomaly_offset;
    let anomaly_u16 = ((anomaly_temp - min_val) * temp_scale).clamp(0.0, max_val) as u16;

    let anomaly_positions = [
        (25, 25),
        (25, 75),
        (50, 50),
        (75, 25),
        (75, 75),
    ];

    for (y, x) in anomaly_positions {
        for dy in -anomaly_radius..=anomaly_radius {
            for dx in -anomaly_radius..=anomaly_radius {
                let ny = y as i32 + dy;
                let nx = x as i32 + dx;
                if ny >= 0 && ny < height as i32 && nx >= 0 && nx < width as i32 {
                    let pixel = image.get_pixel_mut(ny as u32, nx as u32);
                    *pixel = Rgba([anomaly_u16, anomaly_u16, anomaly_u16, 255]);
                }
            }
        }
    }

    debug!(
        "Created test image: {}x{} with {} anomalies at {}K offset",
        width,
        height,
        num_anomalies,
        anomaly_offset
    );

    image
}

/// Saves thermal image to TIFF file
pub fn save_thermal_image(image: &RgbaImage, output_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    // Convert to RGB for TIFF compatibility
    let rgb_image = image.to_rgb();
    let rgb_data = rgb_image.into_raw();

    // Create TIFF writer
    let mut writer = image::io::Writer::new_tiff(output_path, &rgb_image, image::image_format::Tiff::default())?;
    writer.write_all(&rgb_data)?;
    writer.finish()?;

    info!("Saved test image to: {:?}", output_path);
    Ok(())
}

/// Runs the GPU inference binary with test data
pub fn run_gpu_test(
    binary_path: &PathBuf,
    input_path: &PathBuf,
    threshold: f32,
) -> Result<GpuTestResult, Box<dyn std::error::Error>> {
    info!("Running GPU test binary: {:?}", binary_path);

    // Check if binary exists
    if !binary_path.exists() {
        error!("GPU binary not found: {:?}", binary_path);
        return Err("GPU binary not found".into());
    }

    // Build command
    let mut cmd = Command::new(binary_path);
    cmd.arg(input_path)
        .arg("--threshold")
        .arg(threshold.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    info!("Executing: {:?}", cmd);

    // Run command
    let output = cmd.output()?;

    let elapsed = output.duration;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse results
    let m2200_active = stdout.contains("Quadro M2200") && stdout.contains("is active");
    let intel_detected = stdout.contains("Intel") && stdout.contains("Evaluating");
    let anomalies_found = parse_anomalies_from_output(&stdout);

    let success = m2200_active && anomalies_found > 0;

    info!(
        "GPU test completed in {:.3}s - M2200: {}, Intel: {}, anomalies: {}",
        elapsed.as_secs_f32(),
        m2200_active,
        intel_detected,
        anomalies_found
    );

    if !success {
        warn!("GPU test did not pass validation");
        if !m2200_active {
            error!("M2200 GPU not detected in output");
        }
        if anomalies_found == 0 {
            error!("No anomalies detected in output");
        }
    }

    Ok(GpuTestResult {
        elapsed,
        m2200_active,
        intel_detected,
        anomalies_found,
        success,
        stdout,
        stderr,
    })
}

/// Parses anomalies count from GPU output
fn parse_anomalies_from_output(output: &str) -> usize {
    output
        .lines()
        .filter_map(|line| {
            if line.contains("Detected") && line.contains("anomalies") {
                line.split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse::<usize>().ok())
            } else {
                None
            }
        })
        .next()
        .unwrap_or(0)
}

/// Result of GPU test execution
#[derive(Debug, Clone)]
pub struct GpuTestResult {
    /// Time taken to process
    pub elapsed: Duration,
    /// Whether M2200 GPU was detected as active
    pub m2200_active: bool,
    /// Whether Intel GPU was detected
    pub intel_detected: bool,
    /// Number of anomalies found
    pub anomalies_found: usize,
    /// Overall test success
    pub success: bool,
    /// Raw stdout
    pub stdout: String,
    /// Raw stderr
    pub stderr: String,
}

impl GpuTestResult {
    /// Returns true if test passed
    pub fn passed(&self) -> bool {
        self.success
    }

    /// Returns a human-readable summary
    pub fn summary(&self) -> String {
        format!(
            "GPU Test: {} | M2200: {} | Intel: {} | Anomalies: {} | Time: {:.3}s",
            if self.success { "PASS" } else { "FAIL" },
            if self.m2200_active { "YES" } else { "NO" },
            if self.intel_detected { "YES" } else { "NO" },
            self.anomalies_found,
            self.elapsed.as_secs_f32()
        )
    }
}

/// Main test runner
pub fn run_test(config: &GpuTestConfig) -> Result<GpuTestResult, Box<dyn std::error::Error>> {
    info!("Starting GPU test with config: {:?}", config);

    // Create test image
    let test_image = create_test_thermal_image(
        config.test_width,
        config.test_height,
        config.num_anomalies,
        config.anomaly_offset,
    );

    // Save to temporary file
    let temp_path = std::env::temp_dir().join(format!(
        "cesarops_gpu_test_{}.tif",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    ));
    save_thermal_image(&test_image, &temp_path)?;

    // Run GPU test
    let result = run_gpu_test(
        &config.gpu_binary,
        &temp_path,
        config.default_threshold,
    )?;

    // Cleanup
    let _ = std::fs::remove_file(&temp_path);

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_test_image() {
        let image = create_test_thermal_image(100, 100, 5, 50.0);
        assert_eq!(image.width(), 100);
        assert_eq!(image.height(), 100);
        
        // Check base temperature (285K -> ~11500 in u16)
        let base_pixel = image.get_pixel(0, 0);
        let expected_base = ((285.0 - 200.0) * 200.0) as u16;
        assert_eq!(base_pixel.0[0], expected_base);
    }

    #[test]
    fn test_anomaly_injection() {
        let image = create_test_thermal_image(100, 100, 5, 50.0);
        
        // Check anomaly at (25, 25)
        let anomaly_pixel = image.get_pixel(25, 25);
        let expected_anomaly = ((235.0 - 200.0) * 200.0) as u16;
        assert_eq!(anomaly_pixel.0[0], expected_anomaly);
    }
}
```

## Forge wire
- **Pipeline integration**: `cesarops-inference` binary calls `run_test()` during GPU validation stage
- **Config injection**: Forge passes `GpuTestConfig` via environment variables or command-line flags
- **Result reporting**: `GpuTestResult.summary()` output is logged to pipeline metrics and dashboard

## Risks
- **Binary dependency**: Requires `cesarops-gpu.exe` to be built and available at runtime
- **Path resolution**: Windows-specific `.exe` suffix may need abstraction for cross-platform builds
- **Temp file cleanup**: Must ensure temporary TIFF files are removed on failure to avoid disk space issues
- **Threshold tuning**: Default 1.5 may need calibration for different GPU generations (M2200 vs newer)
- **Error handling**: GPU binary failures should be caught and reported without blocking pipeline
