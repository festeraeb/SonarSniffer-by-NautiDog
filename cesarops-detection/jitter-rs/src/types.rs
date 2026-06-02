//! Shared request/response contract — mirrors the Python jitter_movidius.py API
//! so existing fleet clients (POST /jitter, GET /health) work unchanged.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct JitterRequest {
    pub tile_id: String,
    #[serde(default)]
    pub thermal_timeseries: Vec<String>,
    #[serde(default)]
    pub coordinates: Coordinates,
    #[serde(default = "default_depth")]
    pub depth_estimate_m: f64,
}

fn default_depth() -> f64 {
    150.0
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Coordinates {
    #[serde(default)]
    pub lat: f64,
    #[serde(default)]
    pub lon: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct JitterSignature {
    pub material: String,
    pub certainty: f64,
    pub depth_estimate_ft: f64,
    pub jitter_frequency_hz: f64,
    pub thermal_delta_c: f64,
    pub classification: String,
    /// Per-accelerator votes recorded during cross-validation.
    #[serde(default)]
    pub validation: Vec<ValidatorVote>,
    /// Which backend produced the primary candidate.
    pub primary_backend: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidatorVote {
    pub device: String,
    /// agreement in [0,1] with the primary candidate material.
    pub agreement: f64,
    pub agreed: bool,
    pub backend: String,
}

/// A backend's raw read on a tile before consensus is applied.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub material: String,
    pub certainty: f64,
    pub jitter_frequency_hz: f64,
    pub thermal_delta_c: f64,
    pub backend: String,
}
