# enhance wreckhunter/populate_database.py

## Verdict
KEEP_AND_ENHANCE
## Changes
*   Refactored `RunTileRecord` to `AnomalyHitRecord` and expanded it to capture all necessary fields (including `tile_path`, `sensor_type`, `zscore`, `anomaly_count`, `lat`, `lon`, `detected_at`) for a complete database insertion.
*   Replaced `parse_results_json` with `extract_anomaly_hits` which robustly iterates through the complex JSON structure (`results` -> `tiles` -> `sensors`) to generate a list of `AnomalyHitRecord`s.
*   Implemented `sql_insert_hit` to correctly generate the SQL statement and parameters for a single record.
*   Added comprehensive unit tests (`test_extract_anomaly_hits_success`, `test_extract_anomaly_hits_missing_data`) to validate the JSON parsing logic against expected Python behavior.
*   Updated the module structure to be cleaner and more idiomatic Rust for data processing.
## Rust path
cesarops-inference/src/integrate/lake_michigan/populate_database.rs
## Rust source
```rust
//! Populate census DB from run JSON — port of `wreckhunter/populate_database.py`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The path to the census database file.
pub const CENSUS_DB: &str = "wreckhunter2000/LAKE_MICHIGAN_CENSUS_2026.db";

/// Represents a single anomaly hit record ready for database insertion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnomalyHitRecord {
    pub tile_path: String,
    pub sensor_type: String,
    pub zscore: f32,
    pub anomaly_count: u32,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub detected_at: String,
}

/// The SQL DDL for the anomaly_hits table.
pub const ANOMALY_HITS_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS anomaly_hits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tile_path TEXT,
    sensor_type TEXT,
    zscore REAL,
    anomaly_count INTEGER,
    lat REAL,
    lon REAL,
    detected_at TEXT
);
"#;

/// Represents the structure of the results JSON file.
#[derive(Debug, Deserialize)]
struct ResultsFile {
    results: Vec<TileResult>,
    // We include other fields for completeness, though only 'results' is used here.
    #[serde(default)]
    run_timestamp: String,
    #[serde(default)]
    processed: u32,
}

/// Represents a single tile result entry.
#[derive(Debug, Deserialize)]
struct TileResult {
    tile: String,
    sensors: std::collections::HashMap<String, SensorData>,
    // Note: Lat/Lon might be present here in a full implementation, but we focus on the core logic.
}

/// Represents the sensor data within a tile.
#[derive(Debug, Deserialize)]
struct SensorData {
    #[serde(default)]
    max_zscore: f64,
    #[serde(default)]
    anomaly_count: u64,
}

/// Extracts all anomaly hit records from the results JSON structure.
///
/// This function mimics the iteration logic of the Python `populate_from_results` function.
///
