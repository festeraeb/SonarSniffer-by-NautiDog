# integrate/unmapped/laptopdump_wreckhunter_build/test_cuda_minimal.py

## Verdict
MERGE_INTO_LIVE

## Rust path
cesarops-inference/src/integrate/gpu_health.rs

## Rust source
```rust
//! Minimal GPU health check: detect GPU, transfer data, verify.
//!
//! This module provides a simple test to ensure the CUDA runtime is functional
//! and can be called by the inference pipeline during startup or diagnostics.

use nvidia_cuda::device::Device;
use nvidia_cuda::runtime::get_device_properties;
use nvidia_cuda::error::Error as CudaError;
use std::ffi::CString;
use std::os::raw::c_char;

/// Runs a minimal GPU test: detects GPU 0, transfers a small float array,
/// and verifies data integrity. Returns a Result with a success message.
pub fn test_gpu_minimal() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure CUDA is initialized (environment variables, library paths, etc.)
    // The pipeline should set these up before calling this function.
    // If needed, we could call a dedicated init function here.

    // Get device 0
    let device = Device::new(0)?;
    let props = get_device_properties(0)?;

    // Safely convert the GPU name from the raw pointer
    let gpu_name = unsafe {
        let name_ptr = props.name.as_ptr();
        let len = props.name_len;
        let cstr = CString::from_raw(name_ptr as *mut c_char);
        cstr.to_string_lossy().into_owned()
    };

    println!("GPU: {}", gpu_name);
    println!("Compute: {}.{}", props.major, props.minor);

    // CPU data
    let cpu_data: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    println!("CPU data: {:?}", cpu_data);

    // Transfer to GPU
    let gpu_data = device.copy_to_device(&cpu_data)?;
    println!("Uploaded to GPU: OK");

    // Transfer back
    let result: Vec<f32> = device.copy_from_device(&gpu_data)?;
    println!("Downloaded from GPU: {:?}", result);

    if cpu_data == result {
        println!("SUCCESS: GPU memory access working!");
        Ok(())
    } else {
        Err("Data mismatch".into())
    }
}
```

## Forge wire
- The inference pipeline calls `test_gpu_minimal()` during the `gpu_health_check` step (e.g., in the `init` or `warmup` phase).
- If the function returns an error, the pipeline logs a GPU failure and may skip model loading or trigger a fallback CPU path.
- The function is also exposed as a diagnostic endpoint for remote monitoring of GPU status.

## Risks
- **CUDA initialization**: If environment variables (e.g., `CUDA_VISIBLE_DEVICES`, `LD_LIBRARY_PATH`) are not set correctly, `Device::new(0)` will fail.
- **Device availability**: The test assumes device 0 exists; on a multi-GPU system or if the first GPU is disabled, it will panic or return an error.
- **Memory allocation**: On low-memory GPUs, the copy operations may fail with out-of-memory errors, which are not gracefully handled here.
- **Data mismatch**: A mismatch could indicate a corrupted GPU memory or a driver bug, but the test only checks a simple copy, so it may not catch all issues.
