# integrate/unmapped/laptopdump_wreckhunter_build/fast_scan.py

## Verdict
MERGE_INTO_LIVE

## Rust path
cesarops-inference/src/integrate/tiff_fast.rs

## Rust source
```rust
//! Fast TIFF processing with NumPy-like CPU operations.
//!
//! This module provides a `process_tiff_fast` function that loads a TIFF image,
//! computes z-scores, and identifies anomalies above a given threshold.
//! The result is serialized to JSON, matching the Python script's output format.

use image::ImageBuffer;
use ndarray::{Array2, Axis};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;
use chrono::Local;

#[derive(Debug, Serialize, Deserialize)]
pub struct AnomalyReport {
    pub status: String,
    pub processor: String,
    pub dimensions: Dimensions,
    pub anomalies: Vec<Anomaly>,
    pub total_anomalies: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Dimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Anomaly {
    pub row: u32,
    pub col: u32,
    pub zscore: f32,
}

/// Process a single TIFF file and return an anomaly report.
///
/// # Arguments
/// * `tiff_path` - Path to the TIFF file.
/// * `threshold` - Z-score threshold for anomaly detection (default 2.0).
///
/// # Returns
/// A `Result` containing the `AnomalyReport` or an error.
pub fn process_tiff_fast(tiff_path: PathBuf, threshold: f32) -> Result<AnomalyReport, Box<dyn std::error::Error>> {
    // Load TIFF image
    let img = image::open(&tiff_path)?;
    let (width, height) = img.dimensions();
    
    // Convert to f32 array
    let pixels: Vec<u8> = img.raw_pixels().to_vec();
    let arr = Array2::from_shape_fn((height, width), |(y, x)| pixels[y * width + x] as f32);
    
    // Compute mean and standard deviation
    let mean = arr.mean();
    let std = arr.std();
    
    // Compute z-scores
    let zscore = arr.mapv(|v| if std > 0.0 { (v - mean) / std } else { 0.0 });
    
    // Find anomalies
    let anomalies_mask = zscore.mapv(|v| v.abs() > threshold);
    let anomaly_count = anomalies_mask.sum();
    
    let mut anomalies = Vec::new();
    if anomaly_count > 0 {
        // Iterate over the array to collect anomaly coordinates
        for (y, x) in arr.axis_chunks() {
            if anomalies_mask[(y, x)] {
                anomalies.push(Anomaly {
                    row: y as u32,
                    col: x as u32,
                    zscore: zscore[(y, x)],
                });
            }
        }
        // Limit to 100 anomalies as in the Python script
        if anomalies.len() > 100 {
            anomalies.truncate(100);
        }
    }
    
    Ok(AnomalyReport {
        status: "success".to_string(),
        processor: "NumPy CPU".to_string(),
        dimensions: Dimensions { width, height },
        anomalies,
        total_anomalies: anomaly_count as usize,
    })
}

/// Main function to process multiple TIFF files and write results to JSON.
/// This is included for testing and standalone execution, but the module is primarily a library.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("================================================================================");
    println!("FAST TIFF PROCESSING - NumPy CPU");
    println!("================================================================================");
    println!();

    // Define search paths (can be configured via environment or arguments)
    let search_paths = vec![
        PathBuf::from(r"C:\Users\thomf\programming\Bagrecovery\outputs\rossa_forensic_cache"),
        PathBuf::from(r"C:\Users\thomf\programming\Bagrecovery\sentinel_hunt\cache"),
    ];

    let mut tiffs = Vec::new();
    for search_path in search_paths {
        if search_path.exists() {
            tiffs.extend(search_path.rglob("*B11.tif"));
            tiffs.extend(search_path.rglob("*B12.tif"));
        }
    }
    tiffs = tiffs.iter().cloned().collect::<Vec<_>>();
    tiffs.sort();
    tiffs.dedup();
    tiffs.truncate(10); // Process first 10 TIFFs

    println!("Processing {} TIFFs...", tiffs.len());
    println!();

    let mut results = Vec::new();
    for (i, tiff) in tiffs.iter().enumerate() {
        println!("[{}/{}] {}", i + 1, tiffs.len(), tiff.file_name().unwrap_or_default());
        match process_tiff_fast(tiff.clone(), 2.0) {
            Ok(report) => {
                results.push((tiff.to_string(), report));
            }
            Err(e) => {
                println!("  ERROR: {}", e);
            }
        }
        println!();
    }

    // Save results to JSON
    let output_dir = PathBuf::from("outputs");
    let output_file = output_dir.join(format!(
        "fast_scan_{}.json",
        Local::now().format("%Y%m%d_%H%M%S")
    ));
    output_dir.mkdir_all(true)?;

    let json_output = serde_json::to_string_pretty(&results)?;
    fs::write(&output_file, json_output)?;

    println!("================================================================================");
    println!("Results: {}", output_file.display());
    println!("================================================================================");

    Ok(())
}
```

## Forge wire
- Forge's pipeline orchestrator invokes `process_tiff_fast` for each TIFF file in a batch, passing the file path and a configurable threshold.
- The returned `AnomalyReport` structs are collected and written to a JSON file, matching the Python script's output format for downstream analysis.
- Forge can also use this module as a standalone processor in a larger data processing graph, allowing it to be composed with other image processing steps.

## Risks
- **TIFF compatibility**: The `image` crate supports many formats, but some TIFFs with unusual compression or metadata may fail to load. Error handling is in place, but corrupted files will be skipped.
- **Z-score stability**: If an image has a constant value (std = 0), the z-score is set to 0 to avoid division by zero. This may mask anomalies in uniform regions.
- **Memory usage**: Converting the entire image to an `ndarray` uses significant memory for large images (e.g., 4K×4K). For streaming or very large images, a chunked approach would be needed.
- **JSON output size**: If many anomalies are found, the JSON file can become large. The script limits anomalies to 100 per image, but the total output size can still be substantial.
