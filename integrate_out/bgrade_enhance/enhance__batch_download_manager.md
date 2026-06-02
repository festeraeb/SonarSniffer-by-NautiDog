# enhance wreckhunter/batch_download_manager.py

## Verdict
KEEP_AND_ENHANCE
## Changes
*   Refactored `LakeDownloadSpec` to be the primary data structure for the catalog.
*   Replaced `lake_catalog` with a function that returns a `HashMap<String, LakeDownloadSpec>` for efficient key-based access, mirroring the Python dictionary structure.
*   Improved `build_download_tasks` to accept a slice of lake keys (`&[String]`) and return a structured task list.
*   Added comprehensive unit tests for all public functions, including edge cases for filtering and date generation.
*   Ensured all types adhere to `serde` conventions for integration compatibility.
## Rust path
cesarops-inference/src/integrate/batch_download_manager.rs
## Rust source
```rust
//! Batch download swarm — port of `wreckhunter/batch_download_manager.py`.
//!
//! This module defines the structure and logic for generating and distributing
//! data download tasks across multiple nodes (swarm mode).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Defines the metadata for a specific lake region.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LakeDownloadSpec {
    /// Unique key identifier (e.g., "superior").
    pub key: String,
    /// Human-readable label (e.g., "Lake Superior").
    pub label: String,
    /// Bounding box: [min_lat, min_lon, max_lat, max_lon].
    pub bbox: [f64; 4],
}

/// The complete catalog of available lakes, keyed by their unique identifier.
/// This mirrors the LAKES dictionary in the Python script.
pub fn lake_catalog() -> HashMap<String, LakeDownloadSpec> {
    let specs = vec![
        LakeDownloadSpec {
            key: "superior".into(),
            label: "Lake Superior".into(),
            bbox: [46.5, -92.0, 48.0, -84.5],
        },
        LakeDownloadSpec {
            key: "michigan".into(),
            label: "Lake Michigan".into(),
            bbox: [41.5, -88.0, 46.0, -85.5],
        },
        LakeDownloadSpec {
            key: "straits".into(),
            label: "Straits of Mackinac".into(),
            bbox: [45.65, -85.0, 46.10, -84.10],
        },
        LakeDownloadSpec {
            key: "huron".into(),
            label: "Lake Huron".into(),
            bbox: [42.5, -84.0, 46.0, -81.0],
        },
        LakeDownloadSpec {
            key: "erie".into(),
            label: "Lake Erie".into(),
            bbox: [41.3, -83.5, 42.5, -78.8],
        },
        LakeDownloadSpec {
            key: "ontario".into(),
            label: "Lake Ontario".into(),
            bbox: [43.2, -79.5, 44.2, -76.0],
        },
    ];

    specs.into_iter().map(|s| (s.key.clone(), s)).collect()
}

/// Generates the standard summer/fall date range for a given year.
/// Dates are inclusive: June 1st to October 31st.
pub fn summer_fall_dates(year: i32) -> (String, String) {
    (format!("{year}-06-01"), format!("{year}-10-31"))
}

/// Represents a single download task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadTask {
    /// The key of the lake (e.g., "superior").
    pub lake_key: String,
    /// The year of the data (e.g., 2023).
    pub year: i32,
    /// Unique identifier for the task (e.g., "superior-2023").
    pub task_id: String,
}

/// Builds a master list of all required download tasks for a given range of lakes and years.
///
/// # Arguments
/// * `lake_keys` - A slice of lake keys to include in the tasks.
/// * `start_year` - The starting year (inclusive).
/// * `end_year` - The ending year (inclusive).
///
/// # Returns
/// A vector of `DownloadTask` structs.
pub fn build_download_tasks(
    lake_keys: &[String],
    start_
