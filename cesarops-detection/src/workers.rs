//! HTTP clients for the vision worker nodes.
//! Each worker is a Python service on a different GPU.

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::json;

use crate::types::{GeoTile, ScoutReport, ValidationReport, JitterSignature};

/// Client for the Scout (GTX 1060, Florence-2)
pub struct ScoutClient {
    client: Client,
    base_url: String,
}

impl ScoutClient {
    pub fn new() -> Self {
        let base_url = std::env::var("SCOUT_URL")
            .unwrap_or_else(|_| "http://100.105.77.74:5570".to_string());
        Self {
            client: Client::builder().timeout(std::time::Duration::from_secs(30)).build().unwrap(),
            base_url,
        }
    }

    pub async fn analyze(&self, tile: &GeoTile) -> Result<ScoutReport> {
        let payload = json!({
            "tile_id": tile.id,
            "lat": tile.lat,
            "lon": tile.lon,
            "image_b64": tile.image_b64,
            "task": "anomaly_detection"
        });

        let resp = self.client
            .post(format!("{}/analyze", self.base_url))
            .json(&payload)
            .send()
            .await
            .context("Scout (1060) unreachable")?;

        resp.json::<ScoutReport>().await.context("Failed to parse scout report")
    }

    pub async fn health(&self) -> bool {
        self.client.get(format!("{}/health", self.base_url))
            .send().await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

/// Client for the Validator (P1000, Moondream2)
pub struct ValidatorClient {
    client: Client,
    base_url: String,
}

impl ValidatorClient {
    pub fn new() -> Self {
        let base_url = std::env::var("VALIDATOR_URL")
            .unwrap_or_else(|_| "http://100.102.158.111:5571".to_string());
        Self {
            client: Client::builder().timeout(std::time::Duration::from_secs(60)).build().unwrap(),
            base_url,
        }
    }

    pub async fn validate(&self, tile: &GeoTile) -> Result<ValidationReport> {
        let payload = json!({
            "tile_id": tile.id,
            "lat": tile.lat,
            "lon": tile.lon,
            "image_b64": tile.image_b64,
        });

        let resp = self.client
            .post(format!("{}/validate", self.base_url))
            .json(&payload)
            .send()
            .await
            .context("Validator (P1000) unreachable")?;

        resp.json::<ValidationReport>().await.context("Failed to parse validation report")
    }

    pub async fn health(&self) -> bool {
        self.client.get(format!("{}/health", self.base_url))
            .send().await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

/// Client for the Jitter Analyst (Coral TPU in VM)
pub struct JitterClient {
    client: Client,
    base_url: String,
}

impl JitterClient {
    pub fn new() -> Self {
        let base_url = std::env::var("JITTER_URL")
            .unwrap_or_else(|_| "http://192.168.122.10:8080".to_string()); // VM internal IP
        Self {
            client: Client::builder().timeout(std::time::Duration::from_secs(10)).build().unwrap(),
            base_url,
        }
    }

    pub async fn check(&self, tile: &GeoTile) -> Result<JitterSignature> {
        let payload = json!({
            "tile_id": tile.id,
            "thermal_timeseries": tile.bands, // thermal band data as time series
            "coordinates": {"lat": tile.lat, "lon": tile.lon},
            "depth_estimate_m": 150.0
        });

        let resp = self.client
            .post(format!("{}/jitter", self.base_url))
            .json(&payload)
            .send()
            .await
            .context("Jitter Analyst (TPU VM) unreachable")?;

        resp.json::<JitterSignature>().await.context("Failed to parse jitter signature")
    }

    pub async fn health(&self) -> bool {
        self.client.get(format!("{}/health", self.base_url))
            .send().await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}
