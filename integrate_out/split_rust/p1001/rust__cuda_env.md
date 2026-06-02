# integrate/unmapped/laptopdump_programming_root/cuda_env.py

## Verdict
MERGE_INTO_LIVE

## Rust path
cesarops-inference/src/integrate/cuda_env.rs

## Rust source
```rust
use std::env;
use std::path::{Path, PathBuf};

/// Configuration for CUDA environment.
pub struct CudaConfig {
    pub cuda_path: PathBuf,
    pub cuda_bin: PathBuf,
}

/// Configures the CUDA environment and returns the configuration.
///
/// This function sets the `CUDA_PATH` and `PATH` environment variables
/// to point to the CUDA toolkit installation. It searches for CUDA in
/// common locations if `CUDA_PATH` is not set or invalid.
pub fn configure_cuda_environment() -> Result<CudaConfig, String> {
    // Get the CUDA_PATH from environment, if set
    let cuda_path_str = env::var("CUDA_PATH").unwrap_or_default();
    let cuda_path = if !cuda_path_str.is_empty() {
        PathBuf::from(cuda_path_str)
    } else {
        PathBuf::new()
    };

    let selected = if cuda_path.exists() {
        cuda_path
    } else {
        // Search for CUDA in common locations
        let candidates = [
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2",
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.0",
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.2",
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v11.8",
        ];
        for candidate in candidates {
            if Path::new(candidate).exists() {
                return Ok(CudaConfig {
                    cuda_path: PathBuf::from(candidate),
                    cuda_bin: PathBuf::from(candidate).join("bin"),
                });
            }
        }
        // If not found, return an error
        return Err(format!(
            "CUDA toolkit not found. Set CUDA_PATH to a valid CUDA installation directory "
            "or install CUDA 13.2/13.0/12.2/11.8."
        ));
    };

    let cuda_bin = selected.join("bin");

    // Set CUDA_PATH environment variable
    env::set_var("CUDA_PATH", selected.to_string_lossy());

    // Append CUDA bin directory to PATH
    let current_path = env::var_os("PATH").unwrap_or_default();
    let separator = if cfg!(windows) { ";" } else { ":" };
    let new_path = format!("{}{}{}", cuda_bin.to_string_lossy(), separator, current_path);
    env::set_var("PATH", new_path);

    // On Windows, we can also add the CUDA bin directory to the DLL search path
    // using the `windows` crate, but to avoid dependencies
