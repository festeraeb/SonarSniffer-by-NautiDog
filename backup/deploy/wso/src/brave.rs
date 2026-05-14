//! Brave Search API client for cesarops-wso web search.
//!
//! Uses the Brave Search API (free tier: 2000 queries/month).
//! Returns structured JSON results with high reliability.
//!
//! API Key: Set via BRAVE_API_KEY environment variable or config.toml.
//! Docs: https://api.search.brave.com/res/v1/web/search

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A single search result from Brave Search API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraveResult {
    pub title: String,
    pub url: String,
    pub description: String,
    pub extra_snippets: Vec<String>,
    pub result_type: String, // "web", "news", etc.
}

/// Brave Search API response structure
#[derive(Debug, Deserialize)]
struct BraveApiResponse {
    web: Option<BraveWebResults>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResults {
    results: Vec<BraveApiResult>,
}

#[derive(Debug, Deserialize)]
struct BraveApiResult {
    title: String,
    url: String,
    description: String,
    extra_snippets: Option<Vec<String>>,
    #[serde(rename = "type")]
    result_type: Option<String>,
}

/// Brave Search API client
pub struct BraveClient {
    http: Client,
    api_key: String,
    max_results: usize,
}

impl BraveClient {
    /// Create a new Brave Search client.
    /// api_key: Brave API key (from environment or config)
    pub fn new(api_key: String, max_results: usize) -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            api_key,
            max_results,
        }
    }

    /// Search using the Brave Search API.
    pub async fn search(&self, query: &str) -> Result<Vec<BraveResult>> {
        let url = "https://api.search.brave.com/res/v1/web/search";

        let payload = serde_json::json!({
            "q": query,
            "count": self.max_results,
            "freshness": "pm", // Past month for recency
        });

        let response = self
            .http
            .post(url)
            .header("Accept", "application/json")
            .header("Accept-Encoding", "gzip")
            .header("X-Subscription-Token", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("Failed to send Brave Search request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Brave Search API returned {}: {}", status, body);
        }

        let api_response: BraveApiResponse = response
            .json()
            .await
            .context("Failed to parse Brave Search response")?;

        let results: Vec<BraveResult> = api_response
            .web
            .map(|w| {
                w.results
                    .into_iter()
                    .take(self.max_results)
                    .map(|r| BraveResult {
                        title: r.title,
                        url: r.url,
                        description: r.description,
                        extra_snippets: r.extra_snippets.unwrap_or_default(),
                        result_type: r.result_type.unwrap_or_else(|| "web".to_string()),
                    })
                    .collect()
            })
            .unwrap_or_default();

        if results.is_empty() {
            tracing::warn!("Brave Search returned no results for query: {}", query);
        }

        Ok(results)
    }
}

/// Search Brave directly (convenience function)
pub async fn search_brave(query: &str, api_key: &str, max_results: usize) -> Result<Vec<BraveResult>> {
    let client = BraveClient::new(api_key.to_string(), max_results);
    client.search(query).await
}
