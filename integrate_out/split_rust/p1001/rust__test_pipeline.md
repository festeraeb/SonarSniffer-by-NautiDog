# integrate/unmapped/laptopdump_wreckhunter_build/test_pipeline.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/pipeline_test.rs

## Rust source
```rust
//! End-to-End Pipeline Test Module
//! Tests: Build → GPU Detection → Single TIFF Processing
//!
//! This module provides production-ready pipeline validation for the
//! cesarops-gpu inference engine. It integrates with the existing GPU
//! detection and TIFF processing capabilities.

use std::process::Command;
use std::path::PathBuf;
use std::fs;
use std::env;
use cesarops_gpu::GpuDetector;
use cesarops_tiff::TiffProcessor;

/// Represents a pipeline test step result
#[derive(Debug, Clone)]
pub struct PipelineStep {
    pub number: u32,
    pub total: u32,
    pub message: String,
    pub success: bool,
    pub output: String,
}

/// Main pipeline test runner
pub fn run_pipeline_test(root: &PathBuf) -> Result<bool, Box<dyn std::error::Error>> {
    println!("================================================================================");
    println!("CESAROPS END-TO-END PIPELINE TEST");
    println!("================================================================================");

    // Step 1: Build
    let build_result = run_step(1, 4, "Building Rust GPU Engine", root)?;
    if !build_result.success {
        return Ok(false);
    }

    // Step 2: GPU Detection
    let gpu_result = run_step(2, 4, "Testing GPU Detection", root)?;
    if !gpu_result.success {
        return Ok(false);
    }

    // Step 3: Find Test TIFF
    let tiff_result = run_step(3, 4, "Finding Test TIFF", root)?;
    if !tiff_result.success {
        return Ok(false);
    }

    // Step 4: Process TIFF
    let process_result = run_step(4, 4, "Processing TIFF with GPU", root)?;
    if !process_result.success {
        return Ok(false);
    }

    // Summary
    println!("\n================================================================================");
    println!("✓ ALL TESTS PASSED");
    println!("================================================================================");
    println!("\nPipeline is ready. Run:");
    println!("  python cesarops_cli.py");
    Ok(true)
}

/// Execute a pipeline step with proper error handling
fn run_step(
    num: u32,
    total: u32,
    msg: &str,
    root: &PathBuf,
) -> Result<PipelineStep, Box<dyn std::error::Error>> {
    println!("\n[{num}/{total}] {msg}");
    println!("{}", "-".repeat(80));

    let mut step_output = String::new();
    let mut step_error = String::new();

    // Execute the underlying operation
    let result = match msg {
        "Building Rust GPU Engine" => {
            let build_cmd = Command::new("cargo")
                .args(&["build", "--release", "--bin", "cesarops-gpu"])
                .current_dir(root)
                .output()?;

            step_output.push_str(&String::from_utf8_lossy(&build_cmd.stdout));
            if !build_cmd.stderr.is_empty() {
                step_error.push_str(&String::from_utf8_lossy(&build_cmd.stderr));
            }

            let exe_path = root.join("target/release/cesarops-gpu.exe");
            if !build_cmd.status.success() {
                return Ok(PipelineStep {
                    number: num,
                    total,
                    message: msg.to_string(),
                    success: false,
                    output: step_output,
                });
            }

            if !exe_path.exists() {
                return Ok(PipelineStep {
                    number: num,
                    total,
                    message: msg.to_string(),
                    success: false,
                    output: step_output,
                });
            }

            println!("✓ Built: {}", exe_path.display());
            true
        }

        "Testing GPU Detection" => {
            let exe_path = root.join("target/release/cesarops-gpu.exe");
            let detect_cmd = Command::new(&exe_path)
                .output()?;

            step_output.push_str(&String::from_utf8_lossy(&detect_cmd.stdout));
            if !detect_cmd.stderr.is_empty() {
                step_error.push_str(&String::from_utf8_lossy(&detect_cmd.stderr));
            }

            let stdout = String::from_utf8_lossy(&detect_cmd.stdout);
            if stdout.contains("Quadro M2200") {
                println!("✓ Quadro M2200 detected");
            } else if stdout.contains("NVIDIA") {
                println!("⚠ NVIDIA GPU detected (not Quadro M2200)");
            } else {
                println!("✗ No NVIDIA GPU detected");
                return Ok(PipelineStep {
                    number: num,
                    total,
                    message: msg.to_string(),
                    success: false,
                    output: step_output,
                });
            }
            true
        }

        "Finding Test TIFF" => {
            let data_dir = PathBuf::from(r"C:\Users\thomf\programming\wreckhunter2000\data\cache\census_raw");
            let mut test_tiff = None;

            for pattern in ["**/*B10.tif", "**/*B11.tif"] {
                if let Ok(mut entries) = fs::read_dir(&data_dir) {
                    while let Ok(entry) = entries.next() {
                        let path = entry.path();
                        if path.extension().map_or(false, |ext| ext == "tif") {
                            test_tiff = Some(path);
                            break;
                        }
                    }
                }
                if test_tiff.is_some() {
                    break;
                }
            }

            if let Some(ref tiff) = test_tiff {
                println!("✓ Found test TIFF: {}", tiff.display());
                true
            } else {
                println!("✗ No thermal TIFFs found in {}", data_dir.display());
                println!("Run fetcher.py first to download satellite data");
                false
            }
        }

        "Processing TIFF with GPU" => {
            let exe_path = root.join("target/release/cesarops-gpu.exe");
            let tiff_path = if let Some(ref tiff) = test_tiff {
                tiff.clone()
            } else {
                return Ok(PipelineStep {
                    number: num,
                    total,
                    message: msg.to_string(),
                    success: false,
                    output: step_output,
                });
            };

            let process_cmd = Command::new(&exe_path)
                .arg(&tiff_path)
                .output()?;

            step_output.push_str(&String::from_utf8_lossy(&process_cmd.stdout));
            if !process_cmd.stderr.is_empty() {
                step_error.push_str(&String::from_utf8_lossy(&process_cmd.stderr));
            }

            if process_cmd.status.success() {
                println!("✓ GPU processing successful");
                true
            } else {
                println!("✗ GPU processing failed");
                false
            }
        }

        _ => {
            println!("✗ Unknown step: {}", msg);
            false
        }
    };

    Ok(PipelineStep {
        number: num,
        total,
        message: msg.to_string(),
        success: result,
        output: step_output,
    })
}

/// Run GPU detection using the native detector
pub fn detect_gpu() -> Result<String, Box<dyn std::error::Error>> {
    let detector = GpuDetector::new()?;
    let gpu_info = detector.detect()?;
    Ok(gpu_info.to_string())
}

/// Process a single TIFF file with GPU acceleration
pub fn process_tiff(tiff_path: &PathBuf) -> Result<String, Box<dyn std::error::Error>> {
    let processor = TiffProcessor::new()?;
    let result = processor.process(tiff_path)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_step_output() {
        let step = PipelineStep {
            number: 1,
            total: 4,
            message: "Test step".to_string(),
            success: true,
            output: "Test output".to_string(),
        };
        assert_eq!(step.number, 1);
        assert!(step.success);
    }
}
```

## Forge wire
- **Pipeline orchestration**: Forge calls `run_pipeline_test()` from the main CLI entry point when the `--test-pipeline` flag is provided
- **GPU detection integration**: The `detect_gpu()` function is exposed for standalone GPU verification without building the full pipeline
- **TIFF processing hook**: `process_tiff()` provides a direct API for processing individual thermal imagery files in production workflows

## Risks
- **Path hardcoding**: The test TIFF path is hardcoded to a specific user directory; needs environment variable fallback for production
- **Binary dependency**: Assumes `cesarops-gpu.exe` exists in target/release; needs graceful fallback to cargo build if missing
- **GPU detection logic**: Relies on stdout string matching; should integrate with the actual `GpuDetector` crate for robustness
- **Error propagation**: The current implementation uses `Result<bool>` which may lose detailed error information; consider using `anyhow` or `thiserror`
