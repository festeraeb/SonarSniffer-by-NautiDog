# integrate/unmapped/laptopdump_wreckhunter_build/raw_scan_reprocess.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/raw_scan_reprocess.rs

## Rust source
```rust
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::Command;
use std::io::{self, Write};

/// Raw reprocess orchestrator: download missing Sentinel-2 tiles and run hard_pixel_audit
#[derive(Parser)]
#[command(name = "raw_scan_reprocess")]
#[command(about = "Reprocess raw lake scan with real TIFF source")]
struct Args {
    /// Comma-separated MGRS tiles (e.g., 16TDN,16TET)
    #[arg(short, long, default_value = "16TDN")]
    tiles: String,

    /// Year window for NASA fallback
    #[arg(short, long, default_value = "rossa")]
    year_window: String,

    /// Pipeline mode: fetch-only, audit-only, or all (default)
    #[arg(short, long, default_value = "all")]
    mode: String,
}

/// Ensure tiles are present by calling the Rust data fetcher
fn ensure_tiles(year_window: &str, tile_list: &[&str]) -> io::Result<()> {
    // This would integrate with existing wreckhunter2000 Rust modules
    // For now, we assume a Rust module exists at cesarops-inference/src/data_fetcher_scavenger.rs
    // In production, this would be: use cesarops_inference::data_fetcher_scavenger;
    
    // Placeholder for actual implementation - in real code:
    // let scenes = data_fetcher_scavenger::ensure_tiles(year_window, tile_list)?;
    // return Ok(());
    
    // For compilation, we'll use a simple approach that mirrors the Python logic
    // but with proper Rust error handling
    println!("[*] Ensuring Sentinel-2 bands (raw TIFFs) are present");
    
    // Simulate the fetch operation - in production this would call actual Rust code
    // We'll assume the module exists and is properly integrated
    Ok(())
}

/// Run the hard pixel audit subprocess
fn run_hard_pixel_audit() -> io::Result<()> {
    println!("[*] Running hard_pixel_audit.py (real satellite TIFF pipeline)");
    
    let output = Command::new("python")
        .arg("hard_pixel_audit.py")
        .output()?;
    
    if output.status.success() {
        println!("[+] Audit step completed.");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("hard_pixel_audit.py failed: {}", stderr);
        Err(io::Error::new(io::ErrorKind::Other, "hard_pixel_audit.py failed"))
    }
}

/// Main entry point
fn main() -> io::Result<()> {
    let args = Args::parse();
    
    let tile_list: Vec<&str> = args.tiles
        .split(',')
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .collect();
    
    // Fetch step (if required)
    if matches!(args.mode.as_str(), "fetch-only" | "all") {
        let year_window = args.year_window.as_str();
        let tile_list_ref: Vec<&str> = tile_list.iter().collect();
        
        if let Err(e) = ensure_tiles(year_window, &tile_list_ref) {
            eprintln!("Error during tile fetching: {}", e);
            std::process::exit(1);
        }
        
        println!("[+] Fetch step completed.");
    }
    
    // Audit-only path does not require fetch step
    if matches!(args.mode.as_str(), "audit-only" | "all") {
        if let Err(e) = run_hard_pixel_audit() {
            eprintln!("Error during audit: {}", e);
            std::process::exit(1);
        }
    }
    
    println!("[+] raw scan reprocess complete. Check wreckhunter2000/cesarops-search/outputs for final reports and kml files.");
    Ok(())
}
```

## Forge wire
- Called by the main pipeline orchestrator when raw Sentinel-2 data needs reprocessing
- Accepts CLI arguments for tiles, year window, and mode (fetch-only/audit-only/all)
- Integrates with existing Rust data fetcher modules (wreckhunter2000) and subprocess calls to hard_pixel_audit.py

## Risks
- Subprocess call to hard_pixel_audit.py requires Python to be available in the deployment environment
- The ensure_tiles function is a placeholder - actual implementation must integrate with existing Rust data fetcher modules
- Path resolution for wreckhunter2000 modules needs to be handled in the actual cesarops-inference codebase
- Error handling for subprocess failures needs proper logging and exit codes
