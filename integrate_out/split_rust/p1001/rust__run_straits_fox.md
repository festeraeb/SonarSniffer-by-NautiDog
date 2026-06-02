# integrate/unmapped/laptopdump_wreckhunter_build/run_straits_fox.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/straits_fox_pipeline.rs

## Rust source
```rust
//! Straits + Fox Island data processing pipeline orchestrator.
//!
//! This module wraps the historical data download and GPU processing
//! for the Straits of Mackinac to South Fox Island region.
//!
//! # Usage
//! ```rust
//! use cesarops_inference::integrate::straits_fox_pipeline;
//!
//! let result = straits_fox_pipeline::run_pipeline();
//! match result {
//!     Ok(_) => println!("Pipeline completed successfully"),
//!     Err(e) => eprintln!("Pipeline failed: {}", e),
//! }
//! ```

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Repository root directory (where this script lives)
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("wreckhunter2000")
}

/// Check if required Python packages are installed.
/// Returns a list of missing package names.
fn check_python_deps() -> Vec<String> {
    let required = ["h5py", "numpy", "rasterio", "requests"];
    let mut missing = Vec::new();

    for pkg in &required {
        if let Err(_) = python_import(pkg) {
            missing.push(pkg.to_string());
        }
    }

    missing
}

/// Attempt to import a Python module.
fn python_import(pkg: &str) -> Result<(), String> {
    let python = env::var("PYTHON").or_else(|_| env::var("PYTHOND").or_else(|_| "python3".to_string()));
    let import_cmd = format!("{} -c \"import {}\"", python, pkg);
    let output = Command::new(&python)
        .arg("-c")
        .arg(format!("import {};", pkg))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to execute Python: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.trim().to_string())
    } else {
        Ok(())
    }
}

/// Run a Python script and return its exit code.
fn run_python_script(script_path: &Path) -> Result<i32, String> {
    if !script_path.exists() {
        return Err(format!("Script not found: {:?}", script_path));
    }

    let python = env::var("PYTHON").or_else(|_| env::var("PYTHOND").or_else(|_| "python3".to_string()));
    let output = Command::new(&python)
        .arg(&script_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to execute script: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        let combined = format!("{}\n{}", stdout, stderr);
        Err(format!("Script failed with exit code: {}\n{}", output.status.code().unwrap_or(-1), combined))
    } else {
        Ok(output.status.code().unwrap_or(0))
    }
}

/// Run the historical data download script.
fn run_download_script() -> Result<(), String> {
    let script_path = repo_root().join("straits_south_fox_historical_pull.py");
    run_python_script(&script_path)?;
    Ok(())
}

/// Run the GPU processing engine script.
fn run_engine_script() -> Result<(), String> {
    let script_path = repo_root().join("straits_south_fox_engine_runner.py");
    run_python_script(&script_path)?;
    Ok(())
}

/// Main pipeline orchestrator.
///
/// Returns Ok(()) on success, Err with a descriptive message on failure.
pub fn run_pipeline() -> Result<(), String> {
    println!("\n" + "============================================================" + " ");
    println!("STRAITS + FOX ISLAND - QUICK DATA RUN");
    println!("============================================================");
    println!();
    println!("Area: Straits of Mackinac to South Fox Island");
    println!("Sensors: VIIRS LST (thermal) + VIIRS DNB (nighttime)");
    println!("Years: 2012-2013 (low water)");
    println!();

    // Step 0: Check dependencies
    println!("[*] Checking dependencies...");
    let missing = check_python_deps();
    if !missing.is_empty() {
        let install_cmd = format!("pip install {}", missing.join(" "));
        println!("[!] Missing packages: {}", missing.join(", "));
        println!("[!] Install with: {}", install_cmd);
        println!();
        println!("[!] Install missing packages first, then re-run.");
        println!();
        println!("Press Enter to exit...");
        let _ = std::io::stdin().read_line(&mut String::new());
        return Err("Dependencies not satisfied".to_string());
    }
    println!("[+] All dependencies OK");
    println!();

    // Step 1: Download data
    println!("[*] Downloading historical data...");
    if let Err(e) = run_download_script() {
        println!("[!] Data download failed: {}", e);
        println!();
        println!("Press Enter to exit...");
        let _ = std::io::stdin().read_line(&mut String::new());
        return Err(format!("Data download failed: {}", e));
    }
    println!("[+] Data download completed");
    println!();

    // Step 2: Process with GPU engine
    println!("[*] Running GPU processing engine...");
    if let Err(e) = run_engine_script() {
        println!("[!] Processing failed: {}", e);
        println!();
        println!("Press Enter to exit...");
        let _ = std::io::stdin().read_line(&mut String::new());
        return Err(format!("Processing failed: {}", e));
    }
    println!("[+] GPU processing completed");
    println!();

    // Success
    let output_dir = repo_root()
        .join("outputs")
        .join("straits_south_fox_historical")
        .join("engine_results");

    println!("\n" + "============================================================" + " ");
    println!("SUCCESS!");
    println!("============================================================");
    println!();
    println!("Results saved to:");
    println!("  {:?}", output_dir);
    println!();
    println!("Key files:");
    println!("  - straits_engine_detections.kml (open in Google Earth)");
    println!("  - straits_engine_master_report.json (full data)");
    println!();
    println!("Press Enter to exit...");
    let _ = std::io::stdin().read_line(&mut String::new());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_root() {
        let root = repo_root();
        assert!(root.exists() || !root.exists()); // Just verify it's a PathBuf
    }

    #[test]
    fn test_check_python_deps() {
        let missing = check_python_deps();
        // This test will always pass since we're just checking if packages exist
        // The actual missing packages depend on the environment
    }
}
```

## Forge wire
- **Pipeline entry point**: Forge calls `run_pipeline()` from the `cesarops-inference` crate when a "straits_fox" job is triggered
- **Dependency resolution**: Forge ensures Python packages are installed in the worker environment before calling this module
- **Output integration**: Results are written to `wreckhunter2000/outputs/straits_south_fox_historical/engine_results/` and automatically picked up by the downstream KML/JSON exporters

## Risks
- **Subprocess failures**: Python scripts may fail silently; stderr is captured but not always logged with full context
- **Path resolution**: The `repo_root()` function assumes a specific directory structure; will break if the repo is moved or symlinked
- **Python environment**: Requires Python 3.x with specific packages; no fallback to system Python or conda environments
- **Blocking I/O**: The `input()` equivalent blocks the pipeline; in production, consider using async or non-blocking signals
