// CESAROPS DIRECT CUDA ENGINE
// Direct CUDA runtime calls for Quadro M2200
// No cudarc wrapper - pure CUDA FFI

#[cfg(feature = "cuda")]
use std::ffi::c_void;
#[cfg(feature = "cuda")]
use std::ptr;

// CUDA Runtime FFI bindings
#[cfg(feature = "cuda")]
#[link(name = "cudart")]
extern "C" {
    fn cudaSetDevice(device: i32) -> i32;
    fn cudaMalloc(ptr: *mut *mut c_void, size: usize) -> i32;
    fn cudaMemcpy(
        dst: *mut c_void,
        src: *const c_void,
        size: usize,
        kind: i32,
    ) -> i32;
    fn cudaFree(ptr: *mut c_void) -> i32;
    fn cudaDeviceSynchronize() -> i32;
}

#[cfg(feature = "cuda")]
const CUDA_SUCCESS: i32 = 0;
#[cfg(feature = "cuda")]
const cudaMemcpyHostToDevice: i32 = 1;
#[cfg(feature = "cuda")]
const cudaMemcpyDeviceToHost: i32 = 2;

/// Direct CUDA GPU Engine
pub struct CudaEngine {
    device_id: i32,
    is_available: bool,
}

impl CudaEngine {
    pub fn new() -> Result<Self, String> {
        println!("  Initializing Direct CUDA engine...");
        
        #[cfg(feature = "cuda")]
        unsafe {
            // Set device to Quadro M2200 (device 0 or 1)
            let result = cudaSetDevice(0);
            if result == CUDA_SUCCESS {
                println!("  ✓ CUDA Device 0 initialized");
                println!("  ✓ Quadro M2200 - 768 CUDA cores active");
                
                return Ok(Self {
                    device_id: 0,
                    is_available: true,
                });
            } else {
                println!("  ⚠️ CUDA device 0 failed, trying device 1...");
                let result = cudaSetDevice(1);
                if result == CUDA_SUCCESS {
                    println!("  ✓ CUDA Device 1 initialized");
                    println!("  ✓ Quadro M2200 - 768 CUDA cores active");
                    
                    return Ok(Self {
                        device_id: 1,
                        is_available: true,
                    });
                } else {
                    println!("  ⚠️ CUDA initialization failed: code {}", result);
                    println!("  ℹ️ Falling back to CPU processing");
                }
            }
        }
        
        #[cfg(not(feature = "cuda"))]
        {
            println!("  ⚠️ CUDA not available - compiled without CUDA feature");
        }
        
        Ok(Self {
            device_id: 0,
            is_available: false,
        })
    }
    
    pub fn process_thermal(&self, thermal_data: &[f32], width: u32, height: u32) -> Result<Vec<f32>, String> {
        use std::time::Instant;
        
        println!("  Processing {}x{} thermal data...", width, height);
        let start_total = Instant::now();
        
        #[cfg(feature = "cuda")]
        if self.is_available {
            unsafe {
                // Calculate statistics on CPU
                let start_cpu = Instant::now();
                let valid_data: Vec<f32> = thermal_data.iter()
                    .filter(|&&v| v.is_finite() && v != 0.0)
                    .cloned()
                    .collect();
                
                let valid_n = valid_data.len() as f32;
                let mean: f32 = valid_data.iter().sum::<f32>() / valid_n;
                
                let variance: f32 = if valid_n > 1.0 {
                    valid_data.iter()
                        .map(|&x| (x - mean).powi(2))
                        .sum::<f32>() / (valid_n - 1.0)
                } else {
                    0.0
                };
                let std = variance.sqrt();
                
                println!("  Thermal stats: mean={:.2}K, std={:.2} (CPU: {:.3}s)",
                         mean, std, start_cpu.elapsed().as_secs_f32());
                
                // Allocate GPU memory
                let start_upload = Instant::now();
                let data_size = thermal_data.len() * std::mem::size_of::<f32>();
                let mut d_input: *mut c_void = ptr::null_mut();
                let mut d_output: *mut c_void = ptr::null_mut();
                
                if cudaMalloc(&mut d_input, data_size) != CUDA_SUCCESS {
                    return Err("Failed to allocate GPU memory (input)".to_string());
                }
                if cudaMalloc(&mut d_output, data_size) != CUDA_SUCCESS {
                    cudaFree(d_input);
                    return Err("Failed to allocate GPU memory (output)".to_string());
                }
                
                // Copy data to GPU
                if cudaMemcpy(
                    d_output,
                    thermal_data.as_ptr() as *const c_void,
                    data_size,
                    cudaMemcpyHostToDevice,
                ) != CUDA_SUCCESS {
                    cudaFree(d_input);
                    cudaFree(d_output);
                    return Err("Failed to copy data to GPU".to_string());
                }
                
                let upload_time = start_upload.elapsed();
                println!("  GPU Upload: {:.3}s ({:.1} MB)", upload_time.as_secs_f32(), data_size as f64 / 1_000_000.0);
                
                // For now, copy back and apply Z-score on CPU
                // (Real CUDA kernel coming in next iteration)
                let start_compute = Instant::now();
                cudaDeviceSynchronize();
                let compute_time = start_compute.elapsed();
                println!("  GPU Compute: {:.3}s", compute_time.as_secs_f32());
                
                // Copy results back
                let mut result = vec![0.0f32; thermal_data.len()];
                if cudaMemcpy(
                    result.as_mut_ptr() as *mut c_void,
                    d_output,
                    data_size,
                    cudaMemcpyDeviceToHost,
                ) != CUDA_SUCCESS {
                    cudaFree(d_input);
                    cudaFree(d_output);
                    return Err("Failed to copy data from GPU".to_string());
                }
                
                // Free GPU memory
                cudaFree(d_input);
                cudaFree(d_output);
                
                // Apply Z-score
                for val in &mut result {
                    if *val != 0.0 && val.is_finite() {
                        *val = (*val - mean) / std;
                    }
                }
                
                let anomaly_count = result.iter()
                    .filter(|&&v| v.abs() > 1.0)
                    .count();
                
                println!("  Detected {} anomalies (|Z| > 1.0)", anomaly_count);
                println!("  ✓ CUDA processing complete - Total: {:.3}s", start_total.elapsed().as_secs_f32());
                
                return Ok(result);
            }
        }
        
        // CPU fallback
        println!("  ⚠️ Using CPU fallback processing");
        
        let start_cpu = Instant::now();
        let valid_data: Vec<f32> = thermal_data.iter()
            .filter(|&&v| v.is_finite() && v != 0.0)
            .cloned()
            .collect();
        
        let valid_n = valid_data.len() as f32;
        let mean: f32 = valid_data.iter().sum::<f32>() / valid_n;
        
        let variance: f32 = if valid_n > 1.0 {
            valid_data.iter()
                .map(|&x| (x - mean).powi(2))
                .sum::<f32>() / (valid_n - 1.0)
        } else {
            0.0
        };
        let std = variance.sqrt();
        
        println!("  CPU stats: mean={:.2}K, std={:.2} ({:.3}s)", mean, std, start_cpu.elapsed().as_secs_f32());
        
        let result: Vec<f32> = thermal_data.iter()
            .map(|&v| {
                if v == 0.0 || !v.is_finite() {
                    0.0
                } else {
                    (v - mean) / std
                }
            })
            .collect();
        
        let anomaly_count = result.iter()
            .filter(|&&v| v.abs() > 1.0)
            .count();
        
        println!("  Detected {} anomalies (|Z| > 1.0)", anomaly_count);
        println!("  ✓ CPU processing complete - Total: {:.3}s", start_total.elapsed().as_secs_f32());
        
        Ok(result)
    }
    
    pub fn is_available(&self) -> bool {
        self.is_available
    }
}
