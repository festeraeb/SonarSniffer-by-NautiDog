//! DuckDuckGo HTML scraper for cesarops-wso web search.
//!
//! No API key required. Scrapes the HTML results page directly.
//! Used as a fallback when SearXNG is unavailable or for specific queries.
//!
//! ⚠️ DuckDuckGo may block aggressive scraping. Use responsibly.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A single search result from DuckDuckGo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuckDuckGoResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source: String, // e.g., "Wikipedia", "Stack Overflow"
}

/// DuckDuckGo search client
pub struct DuckDuckGoClient {
    http: Client,
    max_results: usize,
}

impl DuckDuckGoClient {
    pub fn new(max_results: usize) -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent("CESAROPS/1.0 (research agent; +https://cesarops.io)")
                .build()
                .unwrap_or_default(),
            max_results,
        }
    }

    /// Search DuckDuckGo and return structured results.
    pub async fn search(&self, query: &str) -> Result<Vec<DuckDuckGoResult>> {
        // DuckDuckGo HTML search URL (no API needed)
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let response = self.http.get(&url).send().await?;
        let html = response.text().await?;

        // Parse results from HTML using simple DOM-like extraction
        let results = Self::parse_html_results(&html, self.max_results);

        if results.is_empty() {
            tracing::warn!("DuckDuckGo returned no results for query: {}", query);
        }

        Ok(results)
    }

    /// Parse DuckDuckGo HTML results page.
    /// DuckDuckGo's HTML structure uses specific classes for results.
    fn parse_html_results(html: &str, max_results: usize) -> Vec<DuckDuckGoResult> {
        let mut results = Vec::new();

        // Extract result blocks using regex patterns matching DuckDuckGo's HTML structure
        // Pattern: <a class="result__a" href="..." ...>title</a>
        // Pattern: <a class="result__snippet" href="..." ...>snippet</a>

        // Find all result links
        let title_pattern = regex::Regex::new(
            r#"<a[^>]*class="result__a"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#,
        )
        .unwrap();

        let snippet_pattern = regex::Regex::new(
            r#"<a[^>]*class="result__snippet"[^>]*href="[^"]*"[^>]*>(.*?)</a>"#,
        )
        .unwrap();

        let entity_pattern = regex::Regex::new(r#"&([a-z]+);"#).unwrap();

        // Decode HTML entities
        let decode_entities = |s: &str| -> String {
            entity_pattern
                .replace_all(s, |caps: &regex::Captures| {
                    let entity = caps[1].to_string();
                    match entity.as_str() {
                        "amp" => "&".to_string(),
                        "lt" => "<".to_string(),
                        "gt" => ">".to_string(),
                        "quot" => "\"".to_string(),
                        "apos" => "'".to_string(),
                        _ => caps[0].to_string(),
                    }
                })
                .to_string()
        };

        // Extract titles and URLs
        for cap in title_pattern.captures_iter(html) {
            let url = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let raw_title = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            let title = decode_entities(raw_title);

            if !url.is_empty() && !title.is_empty() {
                results.push(DuckDuckGoResult {
                    title,
                    url: url.to_string(),
                    snippet: String::new(), // Will be filled below
                    source: String::new(),
                });
            }
        }

        // Extract snippets (they appear right after titles in the HTML)
        for cap in snippet_pattern.captures_iter(html) {
            if let Some(idx) = cap.get(1) {
                let snippet = decode_entities(idx.as_str());
                if let Some(result) = results.last_mut() {
                    result.snippet = snippet;
                }
            }
        }

        // Limit to max_results
        results.truncate(max_results);

        results
    }
}

/// Search DuckDuckGo directly (convenience function)
pub async fn search_ddg(query: &str, max_results: usize) -> Result<Vec<DuckDuckGoResult>> {
    let client = DuckDuckGoClient::new(max_results);
    client.search(query).await
}
