# tests/hardware/test_gpu_validation.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/gpu_detection.rs

## Rust source
```rust
use std::process::{Command, Stdio};
use std::path::Path;

pub fn test_rust_gpu() -> bool {
    println!("{}", "="; 80);
    println!("TESTING RUST GPU ENGINE");
    println!("{}", "="; 80);
    println!();

    let rust_exe = Path::new("target").join("release").join("cesarops-gpu.exe");

    if !rust_exe.exists() {
        println!("ERROR: Rust engine not built");
        println!("Run: build_gpu.bat");
        return false;
    }

    println!("Running GPU engine (no TIFF - just initialization test)...");
    println!();

    let output = Command::new(rust_exe)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to run GPU engine");

    println!("{}", String::from_utf8_lossy(&output.stdout));

    if String::from_utf8_lossy(&output.stdout).contains("Quadro M2200") {
        println!("✓ SUCCESS: Quadro M2200 detected and active");
        true
    } else if String::from_utf8_lossy(&output.stdout).contains("NVIDIA") {
        println!("⚠ WARNING: NVIDIA GPU detected but not Quadro M2200");
        println!("Check if correct GPU is selected");
        false
    } else {
        println!("✗ FAILURE: No NVIDIA GPU detected");
        println!("GPU may not be accessible or drivers missing");
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_gpu_detection() {
        // This is a placeholder test. In a real scenario, you would mock the GPU detection.
        assert!(test_rust_gpu());
    }
}
```

## mod.rs wire
```rust
pub mod gpu_detection;
```

## Risks
- The Rust code assumes the existence of `cesarops-gpu.exe` in the specified path. This needs to be ensured during the build process.
- The test function `test_rust_gpu` does not handle all edge cases, such as different GPU models or missing drivers.
- The `wgpu` test is not ported as it relies on a Python library. This functionality would need to be implemented in Rust if required.
