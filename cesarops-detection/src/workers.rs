//! HTTP clients for vision workers with failover across the cluster.
//!
//! Priority: cesarops2 (1070 / 2060 / P106) → T440 local CPU sim fallback.

use anyhow::{Context, Result};
use serde_json::json;

use crate::endpoint_pool::EndpointPool;
use crate::types::{GeoTile, JitterSignature, ScoutReport, ValidationReport};

/// Client for Scout (Florence-2 or CPU sim)
pub struct ScoutClient {
    pool: EndpointPool,
    client: reqwest::Client,
}

impl ScoutClient {
    pub fn new() -> Self {
        let pool = EndpointPool::scout();
        Self {
            client: pool.http_client(),
            pool,
        }
    }

    pub async fn analyze(&self, tile: &GeoTile) -> Result<ScoutReport> {
        let base = self
            .pool
            .active_base()
            .await
            .context("Scout pool offline (cesarops2 + T440 fallback)")?;

        let payload = json!({
            "tile_id": tile.id,
            "lat": tile.lat,
            "lon": tile.lon,
            "image_b64": tile.image_b64,
            "task": "anomaly_detection"
        });

        let resp = self
            .client
            .post(format!("{}/analyze", base))
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .with_context(|| format!("Scout unreachable at {}", base))?;

        if !resp.status().is_success() {
            let _ = self.pool.refresh().await;
            anyhow::bail!("Scout returned HTTP {}", resp.status());
        }

        resp.json::<ScoutReport>()
            .await
            .context("Failed to parse scout report")
    }

    pub async fn health(&self) -> bool {
        self.pool.health().await
    }

    pub async fn endpoint_label(&self) -> String {
        self.pool.active_label().await
    }
}

/// Client for Validator (Moondream2 or CPU sim)
pub struct ValidatorClient {
    pool: EndpointPool,
    client: reqwest::Client,
}

impl ValidatorClient {
    pub fn new() -> Self {
        let pool = EndpointPool::validator();
        Self {
            client: pool.http_client(),
            pool,
        }
    }

    pub async fn validate(&self, tile: &GeoTile) -> Result<ValidationReport> {
        let base = self
            .pool
            .active_base()
            .await
            .context("Validator pool offline (cesarops2 + T440 fallback)")?;

        let payload = json!({
            "tile_id": tile.id,
            "lat": tile.lat,
            "lon": tile.lon,
            "image_b64": tile.image_b64,
        });

        let resp = self
            .client
            .post(format!("{}/validate", base))
            .json(&payload)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .with_context(|| format!("Validator unreachable at {}", base))?;

        if !resp.status().is_success() {
            let _ = self.pool.refresh().await;
            anyhow::bail!("Validator returned HTTP {}", resp.status());
        }

        resp.json::<ValidationReport>()
            .await
            .context("Failed to parse validation report")
    }

    pub async fn health(&self) -> bool {
        self.pool.health().await
    }

    pub async fn endpoint_label(&self) -> String {
        self.pool.active_label().await
    }
}

/// Client for Jitter Analyst (TPU VM or CPU sim)
pub struct JitterClient {
    pool: EndpointPool,
    client: reqwest::Client,
}

impl JitterClient {
    pub fn new() -> Self {
        let pool = EndpointPool::jitter();
        Self {
            client: pool.http_client(),
            pool,
        }
    }

    pub async fn check(&self, tile: &GeoTile) -> Result<JitterSignature> {
        let base = self
            .pool
            .active_base()
            .await
            .context("Jitter pool offline (cesarops2 + T440 fallback)")?;

        let payload = json!({
            "tile_id": tile.id,
            "thermal_timeseries": tile.bands,
            "coordinates": {"lat": tile.lat, "lon": tile.lon},
            "depth_estimate_m": 150.0
        });

        let resp = self
            .client
            .post(format!("{}/jitter", base))
            .json(&payload)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .with_context(|| format!("Jitter unreachable at {}", base))?;

        if !resp.status().is_success() {
            let _ = self.pool.refresh().await;
            anyhow::bail!("Jitter returned HTTP {}", resp.status());
        }

        resp.json::<JitterSignature>()
            .await
            .context("Failed to parse jitter signature")
    }

    pub async fn health(&self) -> bool {
        self.pool.health().await
    }

    pub async fn endpoint_label(&self) -> String {
        self.pool.active_label().await
    }
}
