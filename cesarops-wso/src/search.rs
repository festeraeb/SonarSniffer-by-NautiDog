use reqwest::Client;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use anyhow::Result;
use tracing::info;

use crate::errors::{WsoError, WsoResult};

/// Web finding structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFinding {
    pub url: String,
    pub title: String,
    pub snippet: String,
    pub content: String, // Cleaned text
    pub confidence: f32, // 0.0-1.0 based on source quality
    pub fetched_at: DateTime<Utc>,
    pub cached: bool,
}

/// Web search result container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResult {
    pub query: String,
    pub findings: Vec<WebFinding>,
    pub total_results: usize,
    pub search_time_ms: u64,
}

/// SearXNG search engine implementation
pub struct SearXNGSearch {
    base_url: String,
}

impl SearXNGSearch {
    pub fn new(base_url: String) -> Self {
        Self { base_url }
    }

    pub async fn search(&self, client: &Client, query: &str) -> Result<Vec<WebFinding>> {
        let url = format!("{}/search", self.base_url);
        let params: Vec<(&str, &str)> = vec![
            ("q", query),
            ("format", "json"),
            ("engines", "google,bing,duckduckgo"),
            ("categories", "general"),
        ];

        let resp = client.get(&url).query(&params).send().await?;
        
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("SearXNG returned status {}", resp.status()));
        }

        let json: serde_json::Value = resp.json().await?;
        
        // Parse SearXNG JSON response
        let results = parse_searxng_results(&json);
        info!("SearXNG returned {} results", results.len());
        
        Ok(results)
    }
}

/// Google Custom Search Engine implementation
pub struct GoogleCSESearch {
    api_key: String,
    engine_id: String,
}

impl GoogleCSESearch {
    pub fn new(api_key: String, engine_id: String) -> Self {
        Self { api_key, engine_id }
    }

    pub async fn search(&self, client: &Client, query: &str) -> Result<Vec<WebFinding>> {
        let url = "https://www.googleapis.com/customsearch/v1";
        let params: Vec<(&str, &str)> = vec![
            ("key", self.api_key.as_str()),
            ("cx", self.engine_id.as_str()),
            ("q", query),
            ("num", "5"),
        ];

        let resp = client.get(url).query(&params).send().await?;
        
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Google CSE returned status {}", resp.status()));
        }

        let json: serde_json::Value = resp.json().await?;
        
        // Parse Google CSE JSON response
        let results = parse_google_cse_results(&json);
        info!("Google CSE returned {} results", results.len());
        
        Ok(results)
    }
}

/// Enum to hold either SearXNG or Google CSE search backend
pub enum SearchBackend {
    SearXNG(SearXNGSearch),
    GoogleCSE(GoogleCSESearch),
}

impl SearchBackend {
    pub async fn search(&self, client: &Client, query: &str) -> Result<Vec<WebFinding>> {
        match self {
            SearchBackend::SearXNG(s) => s.search(client, query).await,
            SearchBackend::GoogleCSE(g) => g.search(client, query).await,
        }
    }
}

/// Parse SearXNG JSON response
fn parse_searxng_results(json: &serde_json::Value) -> Vec<WebFinding> {
    let mut findings = Vec::new();
    
    if let Some(results_array) = json["results"].as_array() {
        for result in results_array {
            if let (Some(url), Some(title), Some(snippet)) = (
                result["url"].as_str(),
                result["title"].as_str(),
                result["content"].as_str(),
            ) {
                findings.push(WebFinding {
                    url: url.to_string(),
                    title: title.to_string(),
                    snippet: snippet.to_string(),
                    content: String::new(), // Will be extracted later
                    confidence: 0.5,
                    fetched_at: Utc::now(),
                    cached: false,
                });
            }
        }
    }
    
    findings
}

/// Parse Google CSE JSON response
fn parse_google_cse_results(json: &serde_json::Value) -> Vec<WebFinding> {
    let mut findings = Vec::new();
    
    if let Some(items) = json["items"].as_array() {
        for item in items {
            if let (Some(url), Some(title), Some(snippet)) = (
                item["link"].as_str(),
                item["title"].as_str(),
                item["snippet"].as_str(),
            ) {
                findings.push(WebFinding {
                    url: url.to_string(),
                    title: title.to_string(),
                    snippet: snippet.to_string(),
                    content: String::new(), // Will be extracted later
                    confidence: 0.5,
                    fetched_at: Utc::now(),
                    cached: false,
                });
            }
        }
    }
    
    findings
}
