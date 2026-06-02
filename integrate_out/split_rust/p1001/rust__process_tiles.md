# integrate/unmapped/laptopdump_wreckhunter_build/process_tiles.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/laptopdump_wreckhunter_build/process_tiles.rs

## Rust source
```rust
//! STEP 3 & 4: PROCESS TILES
//! Run on BOTH laptop and Xenon with IDENTICAL logic.
//!
//! This module processes geotiff tiles from an inventory, computing statistics,
//! z-scores, and anomaly detection. It produces JSON output matching the Python
//! reference implementation.

use chrono::{DateTime, Utc};
use image::{DynamicImage, ImageBuffer, Luma};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Output structure for a single tile result
#[derive(Debug, Serialize, Deserialize)]
pub struct TileResult {
    pub tile: String,
    pub filename: String,
    pub processed_at: String,
    pub sensors: Sensors,
    pub anomalies: Vec<Anomaly>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Sensor data for a single band
#[derive(Debug, Serialize, Deserialize)]
pub struct Sensors {
    pub single_band: SingleBand,
}

/// Statistics and anomaly data for a single band
#[derive(Debug, Serialize, Deserialize)]
pub struct SingleBand {
    pub mean: f32,
    pub std: f32,
    pub max_zscore: f32,
    pub anomaly_count: usize,
    pub anomalies: Vec<Anomaly>,
}

/// Anomaly record for a single pixel
#[derive(Debug, Serialize, Deserialize)]
pub struct Anomaly {
    pub pixel_y: i32,
    pub pixel_x: i32,
    pub zscore: f32,
}

/// Tile metadata from inventory
#[derive(Debug, Deserialize)]
pub struct TileInfo {
    pub path: String,
    pub filename: String,
}

/// Inventory file structure
#[derive(Debug, Deserialize)]
pub struct Inventory {
    pub tiles: Vec<TileInfo>,
}

/// Complete machine run result
#[derive(Debug, Serialize, Deserialize)]
pub struct MachineRunResult {
    pub machine: String,
    pub started_at: String,
    pub total_tiles: usize,
    pub processed: usize,
    pub errors: usize,
    pub results: Vec<TileResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

/// Process a single tile and return structured results
pub fn process_tile(tile_path: &Path) -> TileResult {
    let filename = tile_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let processed_at = Utc::now().format("%Y-%m-%dT%H:%M:%S%.fZ").to_string();

    let mut result = TileResult {
        tile: tile_path.to_string_lossy().to_string(),
        filename,
        processed_at,
        sensors: Sensors {
            single_band: SingleBand {
                mean: 0.0,
                std: 0.0,
                max_zscore: 0.0,
                anomaly_count: 0,
                anomalies: Vec::new(),
            },
        },
        anomalies: Vec::new(),
        error: None,
    };

    // Check if file exists
    if !tile_path.exists() {
        result.error = Some("File not found".to_string());
        return result;
    }

    // Load image
    let img = match DynamicImage::open(tile_path) {
        Ok(img) => img,
        Err(e) => {
            result.error = Some(format!("Load error: {}", e));
            return result;
        }
    };

    // Convert to grayscale for single-band processing
    let gray = img.to_luma8();
    let data = gray.as_raw();
    let width = gray.width() as usize;
    let height = gray.height() as usize;

    // Calculate mean
    let sum: f32 = data.iter().map(|
