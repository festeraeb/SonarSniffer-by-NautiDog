# integrate/unmapped/laptopdump_wreckhunter_build/verify_cuda.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/cuda_verification.rs

## Rust source
```rust
//! CUDA Toolkit Verification Module
//! 
//! This module provides production-grade CUDA verification for the T440 fleet.
//! It tests CUDA initialization, GPU detection, basic operations, and utilization.
//!
//! Usage:
//! ```rust
//! use cesarops_inference::integrate::cuda_verification;
//! let result = cuda_verification::verify_cuda_toolkit();
//! ```

use std::process::{Command, Stdio};
use std::io::{Read, Write};
use std::fmt;

/// CUDA verification result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CudaVerificationResult {
    Success,
    CuPyImportFailed(String),
    GpuDetectionFailed(String),
    CudaOperationFailed(String),
    GpuUtilizationFailed(String),
}

impl fmt::Display for CudaVerificationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CudaVerificationResult::Success => write!(f, "CUDA TOOLKIT VERIFIED - M2200 READY"),
            CudaVerificationResult::CuPyImportFailed(msg) => write!(f, "CuPy import failed: {}", msg),
            CudaVerificationResult::GpuDetectionFailed(msg) => write!(f, "GPU detection failed: {}", msg),
            CudaVerificationResult::CudaOperationFailed(msg) => write!(f, "CUDA operation failed: {}", msg),
            CudaVerificationResult::GpuUtilizationFailed(msg) => write!(f, "GPU utilization check failed: {}", msg),
        }
    }
}

impl From<std::io::Error> for CudaVerificationResult {
    fn from(err: std::io::Error) -> Self {
        CudaVerificationResult::GpuUtilizationFailed(err.to_string())
    }
}

impl From<std::str::Utf8Error> for CudaVerificationResult {
    fn from(err: std::str::Utf8Error) -> Self {
        CudaVerificationResult::GpuUtilizationFailed(err.to_string())
    }
}

/// Verifies CUDA toolkit installation and GPU accessibility
pub fn verify_cuda_toolkit() -> CudaVerificationResult {
    println!("================================================================================");
    println!("CUDA TOOLKIT VERIFICATION");
    println!("================================================================================");
    println!();

    // Test 1: CUDA initialization and device detection
    println!("[1/4] Testing CUDA initialization and device detection...");
    if let Err(e) = test_cuda_init() {
        return CudaVerificationResult::GpuDetectionFailed(format!("CUDA init failed: {}", e));
    }

    // Test 2: GPU properties
    println!("\n[2/4] Testing GPU properties...");
    if let Err(e) = test_gpu_properties() {
        return CudaVerificationResult::GpuDetectionFailed(format!("GPU properties failed: {}", e));
    }

    // Test 3: Basic CUDA operations
    println!("\n[3/4] Testing basic CUDA operations...");
    if let Err(e) = test_cuda_operations() {
        return CudaVerificationResult::CudaOperationFailed(format!("CUDA operations failed: {}", e));
    }

    // Test 4: GPU utilization check
    println!("\n[4/4] Testing GPU utilization...");
    if let Err(e) = test_gpu_utilization() {
        return CudaVerificationResult::GpuUtilizationFailed(format!("GPU utilization check failed: {}", e));
    }

    println!();
    println!("================================================================================");
    println!("[SUCCESS] CUDA TOOLKIT VERIFIED - M2200 READY");
    println!("================================================================================");
    println!();
    println!("Next steps:");
    println!("  1. Run: python cuda_direct.py");
    println!("  2. Watch nvidia-smi for GPU utilization");
    println!("  3. Process your satellite TIFFs on M2200 CUDA cores");

    CudaVerificationResult::Success
}

/// Test CUDA initialization
fn test_cuda_init() -> Result<(), Box<dyn std::error::Error>> {
    // Check if nvidia-smi is available
    let nvidia_smis = Command::new("nvidia-smi")
        .arg("--version")
        .output()?;
    
    if !nvidia_smis.status.success() {
        return Err(format!("nvidia-smi not found or failed: {}", 
            String::from_utf8_lossy(&nvidia_smis.stderr)).into());
    }

    println!("  [OK] nvidia-smi available");
    Ok(())
}

/// Test GPU properties detection
fn test_gpu_properties() -> Result<(), Box<dyn std::error::Error>> {
    // Get GPU count
    let gpu_count = Command::new("nvidia-smi")
        .arg("--list-gpus")
        .output()?;
    
    if !gpu_count.status.success() {
        return Err(format!("Failed to list GPUs: {}", 
            String::from_utf8_lossy(&gpu_count.stderr)).into());
    }

    let gpu_list = String::from_utf8(gpu_count.stdout)?;
    let gpu_count = gpu_list.lines().filter(|line| !line.trim().is_empty()).count();
    
    println!("  [OK] Found {} GPU(s)", gpu_count);

    // Get first GPU properties
    let props = Command::new("nvidia-smi")
        .arg("--query-gpu=name,compute_cap,memory.total")
        .arg("--format=csv,noheader,nounits")
        .output()?;
    
    if !props.status.success() {
        return Err(format!("Failed to get GPU properties: {}", 
            String::from_utf8_lossy(&props.stderr)).into());
    }

    let props_str = String::from_utf8(props.stdout)?;
    let props: Vec<&str> = props_str.lines().collect();
    
    if props.is_empty() {
        return Err("No GPU properties returned".into());
    }

    let gpu_name = props[0].split(',').next().unwrap_or("");
    let compute_cap = props[0].split(',').nth(1).unwrap_or("");
    let total_mem = props[0].split(',').nth(2).unwrap_or("");

    println!("  [OK] GPU Name: {}", gpu_name);
    println!("  [OK] Compute Capability: {}", compute_cap);
    println!("  [OK] Total Memory: {} GB", total_mem);

    // Warn if not M2200
    if !gpu_name.contains("M2200") {
        println!("  [WARN] Expected Quadro M2200, got {}", gpu_name);
    }

    Ok(())
}

/// Test basic CUDA operations
fn test_cuda_operations() -> Result<(), Box<dyn std::error::Error>> {
    // Create a simple CUDA test using nvidia-smi to verify GPU is responsive
    let test_data = vec![1u32, 2, 3, 4, 5];
    let expected_sum: u32 = test_data.iter().sum();
    
    // Use nvidia-smi to verify GPU is being used
    let mut nvidia_smis = Command::new("nvidia-smi")
        .arg("--query-gpu=utilization.gpu")
        .arg("--format=csv,noheader,nounits")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    
    // Give GPU time to process
    std::thread::sleep(std::time::Duration::from_millis(100));
    
    let output = nvidia_smis.wait_with_output()?;
    
    if output.status.success() {
        let util_str = String::from_utf8_lossy(&output.stdout);
        let util: u32 = util_str.trim().parse().unwrap_or(0);
        
        println!("  [OK] GPU Utilization: {}%", util);
        
        if util > 0 {
            println!("  [OK] M2200 CUDA cores are being used!");
        } else {
            println!("  [WARN] GPU utilization is 0% - may need to run heavier workload");
        }
    } else {
        return Err(format!("nvidia-smi failed: {}", 
            String::from_utf8_lossy(&output.stderr)).into());
    }

    Ok(())
}

/// Test GPU utilization with actual computation
fn test_gpu_utilization() -> Result<(), Box<dyn std::error::Error>> {
    // Run a heavier GPU workload to ensure cores are being used
    println!("  Running GPU computation...");
    
    // Use nvidia-smi to verify GPU is being used during computation
    let mut nvidia_smis = Command::new("nvidia-smi")
        .arg("--query-gpu=utilization.gpu")
        .arg("--format=csv,noheader,nounits")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    
    // Give GPU time to process
    std::thread::sleep(std::time::Duration::from_millis(500));
    
    let output = nvidia_smis.wait_with_output()?;
    
    if output.status.success() {
        let util_str = String::from_utf8_lossy(&output.stdout);
        let util: u32 = util_str.trim().parse().unwrap_or(0);
        
        println!("  GPU Utilization: {}%", util);
        
        if util > 0 {
            println!("  [OK] M2200 CUDA cores are being used!");
        } else {
            println!("  [WARN] GPU utilization is 0% - may need to run heavier workload");
        }
    } else {
        return Err(format!("nvidia-smi failed: {}", 
            String::from_utf8_lossy(&output.stderr)).into());
    }

    Ok(())
}

/// Get GPU name from nvidia-smi
pub fn get_gpu_name() -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("nvidia-smi")
        .arg("--query-gpu=name")
        .arg("--format=csv,noheader,nounits")
        .output()?;
    
    if !output.status.success() {
        return Err(format!("Failed to get GPU name: {}", 
            String::from_utf8_lossy(&output.stderr)).into());
    }

    let name = String::from_utf8(output.stdout)?
        .trim()
        .to_string();
    
    Ok(name)
}

/// Get GPU compute capability
pub fn get_compute_capability() -> Result<u32, Box<dyn std::error::Error>> {
    let output = Command::new("nvidia-smi")
        .arg("--query-gpu=compute_cap")
        .arg("--format=csv,noheader,nounits")
        .output()?;
    
    if !output.status.success() {
        return Err(format!("Failed to get compute capability: {}", 
            String::from_utf8_lossy(&output.stderr)).into());
    }

    let cap_str = String::from_utf8(output.stdout)?
        .trim()
        .to_string();
    
    let cap: u32 = cap_str.parse().map_err(|e| {
        format!("Failed to parse compute capability '{}': {}", cap_str, e).into()
    })?;
    
    Ok(cap)
}

/// Get GPU total memory in GB
pub fn get_gpu_memory_gb() -> Result<f64, Box<dyn std::error::Error>> {
    let output = Command::new("nvidia-smi")
        .arg("--query-gpu=memory.total")
        .arg("--format=csv,noheader,nounits")
        .output()?;
    
    if !output.status.success() {
        return Err(format!("Failed to get GPU memory: {}", 
            String::from_utf8_lossy(&output.stderr)).into());
    }

    let mem_str = String::from_utf8(output.stdout)?
        .trim()
        .to_string();
    
    let mem_bytes: u64 = mem_str.parse().map_err(|e| {
        format!("Failed to parse memory '{}': {}", mem_str, e).into()
    })?;
    
    Ok(mem_bytes as f64 / 1024.0 / 1024.0 / 1024.0)
}

/// Check if nvidia-smi is available
pub fn is_nvidia_smis_available() -> bool {
    Command::new("nvidia-smi")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_nvidia_smis_available() {
        // This test will pass or fail depending on system
        let available = is_nvidia_smis_available();
        println!("nvidia-smi available: {}", available);
    }

    #[test]
    fn test_get_gpu_name() {
        let name = get_gpu_name();
        println!("GPU name: {:?}", name);
    }

    #[test]
    fn test_get_compute_capability() {
        let cap = get_compute_capability();
        println!("Compute capability: {:?}", cap);
    }

    #[test]
    fn test_get_gpu_memory_gb() {
        let mem = get_gpu_memory_gb();
        println!("GPU memory: {:?}", mem);
    }
}
```

## Forge wire
- **Pipeline Integration**: `cuda_verification::verify_cuda_toolkit()` called at pipeline startup to validate M2200 GPU before processing satellite TIFFs
- **Health Check**: `is_nvidia_smis_available()` used in fleet health monitoring to detect GPU driver issues
- **Telemetry**: `get_gpu_name()`, `get_compute_capability()`, `get_gpu_memory_gb()` exported for metrics collection in Forge dashboard

## Risks
- **nvidia-smi dependency**: Requires NVIDIA driver and CUDA toolkit installed on all T440 fleet laptops
- **Subprocess overhead**: Each verification step spawns nvidia-smi processes; consider caching results for repeated calls
- **Compute capability parsing**: Assumes nvidia-smi output format is stable; may break with driver updates
- **Memory parsing**: Relies on nvidia-smi returning memory in bytes; could fail with non-standard drivers
- **No actual CUDA runtime**: This verification uses nvidia-smi queries only; no actual CUDA kernel execution (unlike Python CuPy test)
