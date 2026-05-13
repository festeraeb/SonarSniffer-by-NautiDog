use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

#[derive(Debug, Serialize)]
struct NautivecsRequest {
    query: String,
    top_k: usize,
}

#[derive(Debug, Serialize)]
struct WsoRequest {
    query: String,
    max_results: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub score: f32,
}

pub struct SearchClient {
    client: Client,
    nautivecs_url: String,
    wso_url: String,
}

impl SearchClient {
    pub fn new(nautivecs_base: &str, wso_base: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build search client");

        Self {
            client,
            nautivecs_url: format!("{}/query", nautivecs_base),
            wso_url: format!("{}/search", wso_base),
        }
    }

    pub async fn search_nautivecs(&self, query: &str, top_k: usize) -> Result<Vec<SearchResult>> {
        let request = NautivecsRequest {
            query: query.to_string(),
            top_k,
        };

        let response = self.client
            .post(&self.nautivecs_url)
            .json(&request)
            .send()
            .await
            .context("Failed to query nautivecs")?;

        if !response.status().is_success() {
            return Ok(Vec::new());
        }

        let results: Vec<SearchResult> = response.json().await.unwrap_or_default();
        info!("nautivecs returned {} results for '{}'", results.len(), query);
        Ok(results)
    }

    pub async fn search_web(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let request = WsoRequest {
            query: query.to_string(),
            max_results,
        };

        let response = self.client
            .post(&self.wso_url)
            .json(&request)
            .send()
            .await
            .context("Failed to query WSO")?;

        if !response.status().is_success() {
            return Ok(Vec::new());
        }

        let results: Vec<SearchResult> = response.json().await.unwrap_or_default();
        info!("WSO returned {} results for '{}'", results.len(), query);
        Ok(results)
    }
}
