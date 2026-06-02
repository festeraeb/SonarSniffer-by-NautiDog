# integrate/unmapped/laptopdump_wreckhunter_build/detection_sorter.py

## Verdict
MERGE_INTO_LIVE

## Rust path
cesarops-inference/src/integrate/detection_sorter.rs

## Rust source
```rust
use rusqlite::{Connection, Result, Row, params};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::f64::consts::PI;
use std::path::Path;

// ============================================================================
// CONFIGURATION
// ============================================================================

const BBOX_PRESETS: &[(&str, f64, f64, f64, f64)] = &[
    ("MICHIGAN_SOUTH", 42.4, -87.2, 43.0, -86.5),
    ("MICHIGAN_NORTH", 44.0, -86.5, 45.5, -85.5),
    ("ERIE", 41.5, -82.5, 42.5, -80.5),
    ("HURON", 43.5, -82.5, 45.5, -81.5),
    ("SUPERIOR", 46.5, -91.0, 48.0, -84.0),
];

const API_KEYS: &[(&str, &[&str])] = &[
    ("cesarops_admin_key_2026", &["view", "filter", "release", "export", "admin"]),
    ("cesarops_collab_key_2026", &["view", "filter", "export"]),
    ("cesarops_family_key_2026", &["view"]),
];

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

pub fn is_nan_or_ud(value: Option<&str>) -> bool {
    match value {
        Some(v) if v.is_empty() => true,
        Some(v) if v == "UD" => true,
        Some(v) if v.to_uppercase() == "NAN" => true,
        _ => false,
    }
}

pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6_371_000.0;
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();

    let a = (dlat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}

pub fn population_stddev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
    variance.sqrt()
}

pub fn calculate_confidence(total_detections: i64, spatial_stddev_m: f64, tool_count: usize, temporal_span: bool) -> (f64, &'static str) {
    let count_score = (0.2 * (total_detections as f64 + 1.0).log2()).min(0.9);
    let spatial_score = match spatial_stddev_m {
        v if v < 5.0 => 1.0,
        v if v < 10.0 => 0.9,
        v if v < 25.0 => 0.7,
        v if v <
