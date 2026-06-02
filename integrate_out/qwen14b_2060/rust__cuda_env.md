# tools/cuda_env.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/cuda_env.rs

## Rust source
```rust
use std::env;
use std::path::Path;

// Prioritized CUDA install dirs (recent first) - can be extended
const CUDA_CANDIDATES: [&str; 4] = [
    r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2",
    r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.0",
    r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.2",
    r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v11.8",
];

pub fn configure_cuda_environment() -> Result<std::collections::HashMap<String, String>, std::io::Error> {
    let mut env_vars = std::collections::HashMap::new();

    let cuda_path = env::var("CUDA_PATH").unwrap_or_default().trim();

    let selected = if !cuda_path.is_empty() && Path::new(cuda_path).exists() {
        cuda_path.to_string()
    } else {
        CUDA_CANDIDATES
            .iter()
            .find(|&&candidate| Path::new(candidate).exists())
            .map(|&candidate| candidate.to_string())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, 
                "CUDA toolkit not found. Set CUDA_PATH to a valid CUDA installation directory or install CUDA 13.2/13.0/12.2/11.8."))?
    };

    let cuda_bin = Path::new(&selected).join("bin").to_string_lossy().to_string();

    env_vars.insert("CUDA_PATH".to_string(), selected);
    env_vars.insert("PATH".to_string(), format!("{}{}", cuda_bin, env::var("PATH").unwrap_or_default()));

    // Windows-specific: add DLL directory
    if cfg!(windows) {
        if let Ok(path) = std::ffi::OsStr::new(&cuda_bin).to_str() {
            if let Err(e) = unsafe { add_dll_directory(path) } {
                eprintln!("Failed to add DLL directory: {}", e);
            }
        }
    }

    Ok(env_vars)
}

#[cfg(windows)]
unsafe fn add_dll_directory(path: &str) -> Result<(), std::io::Error> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
    let result = winapi::um::libloaderapi::AddDllDirectory(wide.as_ptr());
    if result.is_null() {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_configure_cuda_environment() {
        // Mock environment variables for testing
        env::set_var("CUDA_PATH", "");

        let result = configure_cuda_environment();
        assert!(result.is_err());

        env::set_var("CUDA_PATH", r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2");
        let result = configure_cuda_environment();
        assert!(result.is_ok());

        let env_vars = result.unwrap();
        assert_eq!(env_vars["CUDA_PATH"], r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2");
        assert_eq!(env_vars["PATH"].contains(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2\bin"), true);
    }
}
```

## mod.rs wire
```rust
pub mod cuda_env;
```

## Risks
- The `add_dll_directory` function is unsafe and relies on the `winapi` crate, which may not be available or compatible with all environments.
- The error handling for `add_dll_directory` is minimal and may not cover all edge cases.
- The test cases assume a specific environment setup and may not be portable across different systems.
