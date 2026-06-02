# integrate/unmapped/laptopdump_wreckhunter_build/diagnose_gpu.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/gpu_diagnostic.rs

## Rust source
```rust
//! GPU Diagnostic Module - Verify Quadro M2200 CUDA cores are active
//!
//! This module provides GPU health checks for the cesarops-inference pipeline:
//! - NVIDIA driver verification via nvidia-smi
//! - Vulkan runtime validation via vulkaninfo
//! - Rust GPU engine initialization test
//!
//! Returns structured diagnostic results for pipeline integration.

use std::process::{Command, Output, Stdio};
use std::path::Path;
use std::time::Duration;
use serde::{Deserialize, Serialize};

/// GPU diagnostic result with structured status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuDiagnosticResult {
    pub nvidia_driver: GpuCheckResult,
    pub vulkan_runtime: GpuCheckResult,
    pub rust_gpu_engine: GpuCheckResult,
    pub overall_status: OverallStatus,
}

/// Individual GPU check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuCheckResult {
    pub passed: bool,
    pub message: String,
    pub details: Option<String>,
}

/// Overall diagnostic status
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverallStatus {
    Ready,
    Partial,
    Failed,
}

impl GpuDiagnosticResult {
    /// Create a new diagnostic result
    pub fn new(
        nvidia_driver: GpuCheckResult,
        vulkan_runtime: GpuCheckResult,
        rust_gpu_engine: GpuCheckResult,
    ) -> Self {
        let passed_count = [
            nvidia_driver.passed,
            vulkan_runtime.passed,
            rust_gpu_engine.passed,
        ]
        .iter()
        .filter(|&&p| p)
        .count();

        let overall_status = match passed_count {
            3 => OverallStatus::Ready,
            2 => OverallStatus::Partial,
            _ => OverallStatus::Failed,
        };

        Self {
            nvidia_driver,
            vulkan_runtime,
            rust_gpu_engine,
            overall_status,
        }
    }

    /// Check if GPU is ready for inference
    pub fn is_ready(&self) -> bool {
        self.overall_status == OverallStatus::Ready
    }

    /// Get detailed diagnostic report
    pub fn report(&self) -> String {
        format!(
            "GPU Diagnostic Report\n\
             =================\n\
             NVIDIA Driver:    {}\n\
             Vulkan Runtime:   {}\n\
             Rust GPU Engine:  {}\n\
             Overall Status:   {}\n\
             \n\
             Details:\n\
             {}\n\
             {}",
            self.nvidia_driver.message,
            self.vulkan_runtime.message,
            self.rust_gpu_engine.message,
            match self.overall_status {
                OverallStatus::Ready => "✓ ALL SYSTEMS GO - QUADRO M2200 READY FOR GPU PROCESSING",
                OverallStatus::Partial => "⚠ PARTIAL - SOME COMPONENTS NEED ATTENTION",
                OverallStatus::Failed => "✗ GPU NOT READY - FIX ISSUES ABOVE",
            },
            self.nvidia_driver.details
                .as_ref()
                .map(|d| format!("  - NVIDIA: {}\n", d))
                .unwrap_or_default(),
            self.vulkan_runtime.details
                .as_ref()
                .map(|d| format!("  - Vulkan: {}\n", d))
                .unwrap_or_default(),
            self.rust_gpu_engine.details
                .as_ref()
                .map(|d| format!("  - Rust Engine: {}\n", d))
                .unwrap_or_default(),
        )
    }
}

impl GpuCheckResult {
    fn new(passed: bool, message: &str, details: Option<String>) -> Self {
        Self {
            passed,
            message: message.to_string(),
            details,
        }
    }
}

/// Check NVIDIA GPU via nvidia-smi
pub fn check_nvidia_smi() -> GpuCheckResult {
    let output = Command::new("nvidia-smi")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| {
            GpuCheckResult::new(
                false,
                "nvidia-smi not found (NVIDIA drivers not installed)",
                Some(format!("Error: {}", e)),
            )
        })
        .and_then(|out| {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);

            if out.status.success() {
                if stdout.contains("Quadro M2200") {
                    GpuCheckResult::new(
                        true,
                        "✓ Quadro M2200 detected by nvidia-smi",
                        Some(stdout.clone()),
                    )
                } else if stdout.contains("NVIDIA") {
                    GpuCheckResult::new(
                        false,
                        "⚠ NVIDIA GPU detected but not Quadro M2200",
                        Some(stdout.clone()),
                    )
                } else {
                    GpuCheckResult::new(
                        false,
                        "✗ No NVIDIA GPU detected",
                        Some(stdout.clone()),
                    )
                }
            } else {
                GpuCheckResult::new(
                    false,
                    "✗ nvidia-smi execution failed",
                    Some(format!(
                        "Exit code: {}, stdout: {}, stderr: {}",
                        out.status.code().unwrap_or(-1),
                        stdout,
                        stderr
                    )),
                )
            }
        });

    output
}

/// Check Vulkan runtime via vulkaninfo
pub fn check_vulkan() -> GpuCheckResult {
    let output = Command::new("vulkaninfo")
        .arg("--summary")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| {
            GpuCheckResult::new(
                false,
                "✗ vulkaninfo not found",
                Some(format!("Error: {}", e)),
            )
        })
        .and_then(|out| {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);

            if out.status.success() {
                if stdout.contains("NVIDIA") {
                    GpuCheckResult::new(
                        true,
                        "✓ Vulkan runtime installed with NVIDIA driver",
                        Some(stdout.clone()),
                    )
                } else {
                    GpuCheckResult::new(
                        false,
                        "⚠ Vulkan installed but no NVIDIA driver",
                        Some(stdout.clone()),
                    )
                }
            } else {
                GpuCheckResult::new(
                    false,
                    "✗ Vulkan runtime not working",
                    Some(format!(
                        "Exit code: {}, stdout: {}, stderr: {}",
                        out.status.code().unwrap_or(-1),
                        stdout,
                        stderr
                    )),
                )
            }
        });

    output
}

/// Check Rust GPU engine executable
pub fn check_rust_gpu_engine() -> GpuCheckResult {
    // Determine the path to the GPU engine executable
    let exe_path = match std::env::consts::OS {
        "windows" => {
            // Windows: look for cesarops-gpu.exe in target/release
            let target_dir = Path::new("target")
                .join("release")
                .join("cesarops-gpu.exe");
            if target_dir.exists() {
                target_dir
            } else {
                // Fallback: try to find it in the current directory or parent
                Path::new("cesarops-gpu.exe")
            }
        }
        _ => {
            // Linux/macOS: look for cesarops-gpu in target/release
            let target_dir = Path::new("target")
                .join("release")
                .join("cesarops-gpu");
            if target_dir.exists() {
                target_dir
            } else {
                // Fallback: try cesarops-gpu in current directory
                Path::new("cesarops-gpu")
            }
        }
    };

    let output = Command::new(&exe_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .timeout(Some(Duration::from_secs(30)))
        .output()
        .map_err(|e| {
            GpuCheckResult::new(
                false,
                "✗ Rust GPU engine not built or not executable",
                Some(format!("Error: {}", e)),
            )
        })
        .and_then(|out| {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);

            if out.status.success() {
                let mut gpu_detected = false;
                let mut quadro_active = false;

                for line in stdout.lines() {
                    if line.contains("Quadro M2200") {
                        gpu_detected = true;
                        if line.contains("🟢") || line.to_lowercase().contains("active") {
                            quadro_active = true;
                        }
                    } else if line.contains("NVIDIA") && line.contains("vendor=0x10de") {
                        gpu_detected = true;
                    }
                }

                if quadro_active {
                    GpuCheckResult::new(
                        true,
                        "✓ Quadro M2200 is ACTIVE and will be used for processing",
                        Some(format!(
                            "GPU detected: Quadro M2200\n{}",
                            stdout
                        )),
                    )
                } else if gpu_detected {
                    GpuCheckResult::new(
                        false,
                        "⚠ NVIDIA GPU detected but Quadro M2200 not confirmed active",
                        Some(format!(
                            "GPU detected: NVIDIA (vendor=0x10de)\n{}",
                            stdout
                        )),
                    )
                } else {
                    GpuCheckResult::new(
                        false,
                        "✗ No NVIDIA GPU detected by Rust engine",
                        Some(format!(
                            "Engine output:\n{}",
                            stdout
                        )),
                    )
                }
            } else {
                GpuCheckResult::new(
                    false,
                    "✗ Rust GPU engine execution failed",
                    Some(format!(
                        "Exit code: {}, stdout: {}, stderr: {}",
                        out.status.code().unwrap_or(-1),
                        stdout,
                        stderr
                    )),
                )
            }
        });

    output
}

/// Run full GPU diagnostic
pub fn run_diagnostic() -> GpuDiagnosticResult {
    let nvidia = check_nvidia_smi();
    let vulkan = check_vulkan();
    let rust = check_rust_gpu_engine();

    GpuDiagnosticResult::new(nvidia, vulkan, rust)
}

/// Run diagnostic and print to stdout
pub fn run_diagnostic_and_print() -> GpuDiagnosticResult {
    let result = run_diagnostic();
    println!("{}", result.report());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_result_creation() {
        let nvidia = GpuCheckResult::new(true, "NVIDIA OK", None);
        let vulkan = GpuCheckResult::new(true, "Vulkan OK", None);
        let rust = GpuCheckResult::new(true, "Rust OK", None);

        let result = GpuDiagnosticResult::new(nvidia, vulkan, rust);
        assert!(result.is_ready());
        assert_eq!(result.overall_status, OverallStatus::Ready);
    }

    #[test]
    fn test_diagnostic_result_partial() {
        let nvidia = GpuCheckResult::new(true, "NVIDIA OK", None);
        let vulkan = GpuCheckResult::new(false, "Vulkan FAIL", None);
        let rust = GpuCheckResult::new(true, "Rust OK", None);

        let result = GpuDiagnosticResult::new(nvidia, vulkan, rust);
        assert!(!result.is_ready());
        assert_eq!(result.overall_status, OverallStatus::Partial);
    }

    #[test]
    fn test_diagnostic_result_failed() {
        let nvidia = GpuCheckResult::new(false, "NVIDIA FAIL", None);
        let vulkan = GpuCheckResult::new(false, "Vulkan FAIL", None);
        let rust = GpuCheckResult::new(false, "Rust FAIL", None);

        let result = GpuDiagnosticResult::new(nvidia, vulkan, rust);
        assert!(!result.is_ready());
        assert_eq!(result.overall_status, OverallStatus::Failed);
    }
}
```

## Forge wire
- **Pipeline startup**: `cesarops-inference` binary calls `run_diagnostic_and_print()` during initialization to verify GPU readiness before accepting inference requests
- **Health check endpoint**: Forge exposes `/health/gpu` endpoint that returns `GpuDiagnosticResult` JSON for monitoring systems
- **Graceful degradation**: If GPU checks fail, pipeline falls back to CPU-only inference with appropriate error messages

## Risks
- **Binary path assumptions**: The Rust GPU engine path is hardcoded to `target/release/cesarops-gpu` - needs CI/CD to ensure binary is deployed alongside the inference service
- **Cross-platform compatibility**: `nvidia-smi` and `vulkaninfo` availability varies by OS - Windows uses `.exe`, Linux/macOS uses native binaries
- **Timeout handling**: GPU engine check has 30s timeout - if engine is hung, diagnostic will fail but won't block pipeline indefinitely
- **Error message localization**: All diagnostic messages are in English - may need i18n for non-English deployments
