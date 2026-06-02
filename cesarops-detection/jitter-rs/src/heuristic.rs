//! CPU thermal heuristic — deterministic fallback that reproduces the contract
//! of the original Python cpu_sim_workers jitter path.

use crate::types::{Candidate, JitterRequest};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Deterministic seed in [0,1) from tile id + rounded coordinates.
fn seed(tile_id: &str, lat: f64, lon: f64) -> f64 {
    let key = format!("{}:{:.4}:{:.4}", tile_id, lat, lon);
    let mut h = DefaultHasher::new();
    key.hash(&mut h);
    let v = (h.finish() & 0xFFFF_FFFF) as f64;
    (v % 1000.0) / 1000.0
}

/// Produce the heuristic candidate. This is the always-available baseline.
pub fn evaluate(req: &JitterRequest) -> Candidate {
    let lat = req.coordinates.lat;
    let lon = req.coordinates.lon;
    let n_bands = req.thermal_timeseries.len();
    let base = seed(&req.tile_id, lat, lon);

    let mut certainty = (base + 0.12 * (n_bands.min(6) as f64)).min(0.95);
    if certainty < 0.72 && n_bands >= 2 {
        certainty = 0.72;
    }

    let material = if certainty > 0.7 {
        "ferrous_composite"
    } else {
        "natural"
    };

    Candidate {
        material: material.to_string(),
        certainty: round3(certainty),
        jitter_frequency_hz: round4(0.02 + (base % 0.05)),
        thermal_delta_c: round2(0.5 + base * 2.0),
        backend: "cpu_thermal".to_string(),
    }
}

pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}
pub fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}
pub fn round4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}
pub fn round1_ft(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
