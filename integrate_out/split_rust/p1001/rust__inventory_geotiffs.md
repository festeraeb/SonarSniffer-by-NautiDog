# integrate/unmapped/laptopdump_wreckhunter_build/inventory_geotiffs.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/geotiff_inventory.rs

## Rust source
```rust
//! Inventory GeoTIFF files by location, satellite, and band.
//!
//! This module scans designated directories for .tif files, extracts metadata
//! from filenames, and produces a JSON inventory report.

use std::path::{Path, PathBuf};
use std::fs;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use chrono::Utc;

/// Inventory data structure for GeoTIFF files.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeoTiffInventory {
    /// ISO timestamp of the inventory run.
    pub timestamp: String,
    /// Total number of tiles processed.
    pub total_tiles: usize,
    /// Count of tiles by location directory.
    pub by_location: HashMap<String, usize>,
    /// Count of tiles by satellite.
    pub by_satellite: HashMap<String, usize>,
    /// Count of tiles by band.
    pub by_band: HashMap<String, usize>,
    /// Detailed tile information.
    pub tiles: Vec<TileInfo>,
}

/// Individual tile information.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TileInfo {
    /// Full filesystem path.
    pub path: String,
    /// Original filename.
    pub filename: String,
    /// Satellite identifier (e.g., "Landsat-8", "Sentinel-2").
    pub satellite: String,
    /// Date in YYYY-DOYNNN format.
    pub date: String,
    /// Band identifier (e.g., "B10").
    pub band: String,
    /// File size in megabytes.
    pub size_mb: f64,
}

/// Configuration for the inventory scan.
#[derive(Debug, Clone)]
pub struct InventoryConfig {
    /// Directories to search for .tif files.
    pub search_dirs: Vec<PathBuf>,
    /// Output JSON file path.
    pub output_file: PathBuf,
    /// Minimum file size in bytes to consider (default 100KB).
    pub min_file_size_bytes: usize,
}

impl Default for InventoryConfig {
    fn default() -> Self {
        Self {
            search_dirs: vec![
                PathBuf::from("wreckhunter2000/data/cache/census_raw/2021_low_water"),
                PathBuf::from("wreckhunter2000/data/cache/census_raw/2025_rossa"),
                PathBuf::from("wreckhunter2000/data/cache"),
            ],
            output_file: PathBuf::from("outputs/geotiff_inventory.json"),
            min_file_size_bytes: 100_000,
        }
    }
}

/// Parse satellite code from filename part.
fn parse_satellite(satellite_code: &str) -> String {
    match satellite_code {
        "L30" => "Landsat-8".to_string(),
        "S30" => "Sentinel-2".to_string(),
        _ => satellite_code.to_string(),
    }
}

/// Parse date from filename part.
fn parse_date(date_code: &str) -> String {
    if date_code.len() >= 8 {
        let year = &date_code[..4];
        let day_of_year = &date_code[4..];
        format!("{}-DOY{}", year, day_of_year)
    } else {
        "Unknown".to_string()
    }
}

/// Parse band from filename part.
fn parse_band(band: &str) -> String {
    band.to_string()
}

/// Main inventory function.
///
/// Scans all configured directories, extracts metadata from GeoTIFF filenames,
/// and writes a JSON inventory report.
pub fn inventory_geotiffs(config: &InventoryConfig) -> Result<GeoTiffInventory, Box<dyn std::error::Error>> {
    let mut inventory = GeoTiffInventory {
        timestamp: Utc::now().to_rfc3339(),
        total_tiles: 0,
        by_location: HashMap::new(),
        by_satellite: HashMap::new(),
        by_band: HashMap::new(),
        tiles: Vec::new(),
    };

    for search_dir in &config.search_dirs {
        if !search_dir.exists() {
            continue;
        }

        let location_name = search_dir.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        inventory.by_location.entry(location_name.clone()).or_insert(0);

        for tif in search_dir.glob("*.tif") {
            let tif_path = tif.as_path();

            // Skip small files (likely metadata or corrupted)
            let metadata = fs::metadata(tif_path)?;
            if metadata.len() < config.min_file_size_bytes {
                continue;
            }

            // Parse filename
            let filename = tif_path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let parts: Vec<&str> = filename.split('.').collect();

            let mut satellite = "Unknown".to_string();
            let mut date = "Unknown".to_string();
            let mut band = "Unknown".to_string();

            if parts.len() >= 5 {
                // Expected format: HLS.L30.T16TDN.2021182T162824.v2.0.B10.tif
                let satellite_code = parts[1];
                let date_code = parts[3];
                let band = parts[5];

                satellite = parse_satellite(satellite_code);
                date = parse_date(date_code);
                band = parse_band(band);
            }

            // Create tile info
            let tile_info = TileInfo {
                path: tif_path.to_string_lossy().to_string(),
                filename,
                satellite,
                date,
                band,
                size_mb: (metadata.len() as f64) / (1024.0 * 1024.0),
            };

            inventory.tiles.push(tile_info);
            inventory.total_tiles += 1;
            *inventory.by_location.get_mut(&location_name).unwrap() += 1;

            // Count by satellite
            *inventory.by_satellite.entry(satellite.clone()).or_insert(0) += 1;

            // Count by band
            *inventory.by_band.entry(band.clone()).or_insert(0) += 1;
        }
    }

    // Create output directory if needed
    let output_dir = config.output_file.parent()
        .ok_or_else(|| "Cannot determine output directory")?;
    fs::create_dir_all(output_dir)?;

    // Save inventory
    let json_output = serde_json::to_string_pretty(&inventory)?;
    fs::write(&config.output_file, json_output)?;

    Ok(inventory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_satellite() {
        assert_eq!(parse_satellite("L30"), "Landsat-8");
        assert_eq!(parse_satellite("S30"), "Sentinel-2");
        assert_eq!(parse_satellite("Unknown"), "Unknown");
    }

    #[test]
    fn test_parse_date() {
        assert_eq!(parse_date("2021182"), "2021-DOY182");
        assert_eq!(parse_date("2025001"), "2025-DOY001");
        assert_eq!(parse_date("short"), "Unknown");
    }

    #[test]
    fn test_parse_band() {
        assert_eq!(parse_band("B10"), "B10");
    }
}
```

## Forge wire
- **Pipeline trigger**: Called on schedule (e.g., nightly) or on-demand via CLI `cargo run --bin geotiff-inventory`
- **Input**: Reads from `wreckhunter2000/data/cache/` directories (same as Python)
- **Output**: Writes `outputs/geotiff_inventory.json` for downstream analytics and dashboard ingestion

## Risks
- **Filename parsing fragility**: Assumes strict filename format (HLS.L30.T16TDN.2021182T162824.v2.0.B10.tif); malformed files will default to "Unknown"
- **No metadata validation**: Does not verify GeoTIFF headers; relies solely on filename patterns
- **Path handling**: Uses `PathBuf` but doesn't handle Windows path separators explicitly (std::path handles this)
- **Error propagation**: Uses `Result<T, Box<dyn Error>>` which may require careful unwrapping in calling code
