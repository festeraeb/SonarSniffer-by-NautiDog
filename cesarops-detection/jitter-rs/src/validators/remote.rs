//! Remote HTTP validator — calls a validator worker on another node (e.g. the
//! Coral Edge TPU worker on the ML350e) per CORAL_TPU_WORKER_SPEC.md.
//!
//! Configured via env `JITTER_REMOTE_VALIDATORS`, a comma-separated list of
//! `device=url` pairs, e.g.:
//!   JITTER_REMOTE_VALIDATORS="coral_edgetpu=http://10.0.0.201:8190"
//!
//! Each remote is probed once at startup via GET {url}/health; only reachable
//! remotes become active validators. At vote time we POST {url}/validate with a
//! hard timeout — a slow/unreachable remote is skipped, never fatal.

use crate::types::{Candidate, JitterRequest, ValidatorVote};
use super::Validator;
use serde::Serialize;
use std::time::Duration;
use tracing::{info, warn};

const VOTE_TIMEOUT_MS: u64 = 750;
const HEALTH_TIMEOUT_MS: u64 = 1500;

pub struct RemoteValidator {
    device: String,
    base_url: String,
    fleet_key: Option<String>,
    client: reqwest::Client,
    reachable: bool,
}

#[derive(Serialize)]
struct PrimaryInfo<'a> {
    material: &'a str,
    certainty: f64,
    backend: &'a str,
}

#[derive(Serialize)]
struct ValidateBody<'a> {
    tile_id: &'a str,
    thermal_timeseries: &'a [String],
    coordinates: Coords,
    depth_estimate_m: f64,
    primary: PrimaryInfo<'a>,
}

#[derive(Serialize)]
struct Coords {
    lat: f64,
    lon: f64,
}

impl RemoteValidator {
    pub async fn new(device: String, base_url: String, fleet_key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(VOTE_TIMEOUT_MS))
            .build()
            .expect("reqwest client");
        let mut v = Self {
            device,
            base_url: base_url.trim_end_matches('/').to_string(),
            fleet_key,
            client,
            reachable: false,
        };
        v.reachable = v.probe_health().await;
        v
    }

    async fn probe_health(&self) -> bool {
        let url = format!("{}/health", self.base_url);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(HEALTH_TIMEOUT_MS))
            .build()
            .expect("reqwest client");
        let mut req = client.get(&url);
        if let Some(k) = &self.fleet_key {
            req = req.header("X-Fleet-Key", k);
        }
        match req.send().await {
            Ok(r) if r.status().is_success() => {
                info!("remote validator '{}' reachable at {}", self.device, self.base_url);
                true
            }
            Ok(r) => {
                warn!("remote validator '{}' health HTTP {}", self.device, r.status());
                false
            }
            Err(e) => {
                warn!("remote validator '{}' unreachable: {}", self.device, e);
                false
            }
        }
    }
}

#[async_trait::async_trait]
impl Validator for RemoteValidator {
    fn device(&self) -> &str {
        &self.device
    }

    fn available(&self) -> bool {
        self.reachable
    }

    async fn vote(&self, req: &JitterRequest, primary: &Candidate) -> Option<ValidatorVote> {
        if !self.reachable {
            return None;
        }
        let body = ValidateBody {
            tile_id: &req.tile_id,
            thermal_timeseries: &req.thermal_timeseries,
            coordinates: Coords {
                lat: req.coordinates.lat,
                lon: req.coordinates.lon,
            },
            depth_estimate_m: req.depth_estimate_m,
            primary: PrimaryInfo {
                material: &primary.material,
                certainty: primary.certainty,
                backend: &primary.backend,
            },
        };

        let url = format!("{}/validate", self.base_url);
        let mut rb = self.client.post(&url).json(&body);
        if let Some(k) = &self.fleet_key {
            rb = rb.header("X-Fleet-Key", k);
        }

        let resp = match rb.send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("remote '{}' vote failed for {}: {}", self.device, req.tile_id, e);
                return None;
            }
        };
        if !resp.status().is_success() {
            warn!("remote '{}' vote HTTP {} for {}", self.device, resp.status(), req.tile_id);
            return None;
        }

        let v: serde_json::Value = resp.json().await.ok()?;
        let agreement = v.get("agreement").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let agreed = v.get("agreed").and_then(|x| x.as_bool()).unwrap_or(false);
        let backend = v
            .get("backend")
            .and_then(|x| x.as_str())
            .unwrap_or("edgetpu_int8")
            .to_string();

        Some(ValidatorVote {
            device: self.device.clone(),
            agreement: (agreement * 1000.0).round() / 1000.0,
            agreed,
            backend,
        })
    }
}

/// Parse `JITTER_REMOTE_VALIDATORS` and build reachable remote validators.
pub async fn from_env() -> Vec<RemoteValidator> {
    let spec = std::env::var("JITTER_REMOTE_VALIDATORS").unwrap_or_default();
    if spec.trim().is_empty() {
        return Vec::new();
    }
    let fleet_key = std::env::var("FLEET_KEY").ok().filter(|s| !s.is_empty());

    let mut out = Vec::new();
    for pair in spec.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let (device, url) = match pair.split_once('=') {
            Some((d, u)) => (d.trim().to_string(), u.trim().to_string()),
            None => {
                warn!("bad JITTER_REMOTE_VALIDATORS entry '{}': expected device=url", pair);
                continue;
            }
        };
        out.push(RemoteValidator::new(device, url, fleet_key.clone()).await);
    }
    out
}
