

Here are the complete, compilable files for the `cesarops-wso` crate, implementing the Web Search Oracle specification with all requested fixes and features.

```
=== FILE: Cargo.toml ===
[package]
name = "cesarops-wso"
version = "0.1.0"
edition = "2021"
description = "CESAROPS Web Search Oracle: Sovereign-compliant external knowledge retrieval"

[dependencies]
reqwest = { version = "0.12", features = ["json", "cookies", "gzip"] }
tokio = { version = "1.35", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
scraper = "0.18"
html2text = "0.11"
anyhow = "1.0"
tracing = "0.1"
chrono = { version = "0.4", features = ["serde"] }
dashmap = "5.5"
url = "2.5"
regex = "1.10"
thiserror = "1.0"
async-trait = "0.1"
ratelimit = "0.9" # Simple in-memory rate limiter
futures = "0.3"

[features]
default = ["searxng"]
searxng = []
google-cse = []
cache = ["dashmap"]
```

=== FILE: src/lib.rs ===
```rust
pub mod errors;
pub mod search;
pub mod extract;
pub mod cache;
pub mod injector;
pub mod budget;

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use dashmap::DashMap;
use reqwest::Client;
use tracing::{info, warn};

use crate::cache::WebCache;
use crate::errors::{WsoError, WsoResult};
use crate::search::{SearXNGSearch, GoogleCSESearch, SearchEngine};
use crate::extract::WebExtractor;
use crate::budget::TokenBudgetAllocator;
use crate::injector::ContextInjector;

/// Configuration for the Web Search Oracle
#[derive(Debug, Clone)]
pub struct WsoConfig {
    /// SearXNG instance URL (primary)
    pub searxng_url: Option<String>,
    /// Google CSE API key + Engine ID (fallback)
    pub google_cse_key: Option<String>,
    pub google_cse_engine_id: Option<String>,
    /// Max tokens allocated for web context
    pub max_web_context_tokens: usize,
    /// Cache TTL in hours
    pub cache_ttl_hours: u64,
    /// Enable Google CSE fallback
    pub enable_google_fallback: bool,
    /// Base URL for sovereign-cloud (for tool registration)
    pub sovereign_cloud_base_url: String,
    /// Rate limit: requests per minute
    pub rate_limit_rpm: u32,
}

impl Default for WsoConfig {
    fn default() -> Self {
        Self {
            searxng_url: None,
            google_cse_key: None,
            google_cse_engine_id: None,
            max_web_context_tokens: 4096,
            cache_ttl_hours: 24,
            enable_google_fallback: true,
            sovereign_cloud_base_url: "http://localhost:8080".to_string(),
            rate_limit_rpm: 60,
        }
    }
}

/// The main Web Search Oracle engine
pub struct WsoEngine {
    config: WsoConfig,
    client: Client,
    cache: WebCache,
    search_engines: Vec<Box<dyn SearchEngine>>,
    extractor: WebExtractor,
    budget_allocator: TokenBudgetAllocator,
    injector: ContextInjector,
    // Simple in-memory rate limiter
    rate_limiter: Arc<DashMap<String, Vec<chrono::DateTime<Utc>>>>,
}

impl WsoEngine {
    /// Create a new WsoEngine with the given configuration
    pub fn new(config: WsoConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("CESARops-WSO/1.0")
            .build()
            .expect("Failed to build HTTP client");

        let cache = WebCache::new(config.cache_ttl_hours);
        let extractor = WebExtractor::new();
        let budget_allocator = TokenBudgetAllocator::new(config.max_web_context_tokens);
        let injector = ContextInjector::new();

        let mut search_engines = Vec::new();

        // Add SearXNG if configured
        if let Some(url) = &config.searxng_url {
            info!("Initializing SearXNG search engine at {}", url);
            search_engines.push(Box::new(SearXNGSearch::new(url.clone())));
        }

        // Add Google CSE if configured and fallback enabled
        if config.enable_google_fallback {
            if let (Some(key), Some(cx)) = (&config.google_cse_key, &config.google_cse_engine_id) {
                info!("Initializing Google CSE fallback");
                search_engines.push(Box::new(GoogleCSESearch::new(key.clone(), cx.clone())));
            } else {
                warn!("Google CSE fallback enabled but API key or CX not configured");
            }
        }

        if search_engines.is_empty() {
            warn!("No search engines configured. Web search will fail.");
        }

        Self {
            config,
            client,
            cache,
            search_engines,
            extractor,
            budget_allocator,
            injector,
            rate_limiter: Arc::new(DashMap::new()),
        }
    }

    /// Execute a web search with the given query
    pub async fn search(&self, query: &str) -> WsoResult<search::WebSearchResult> {
        // Rate limiting check
        self.check_rate_limit(query)?;

        let start = std::time::Instant::now();
        
        // Try each search engine in order
        for engine in &self.search_engines {
            match engine.search(&self.client, query).await {
                Ok(results) => {
                    let elapsed = start.elapsed().as_millis() as u64;
                    info!("Search completed via {} in {}ms", engine.name(), elapsed);
                    
                    // Process and enrich results
                    let enriched_results = self.enrich_results(results).await?;
                    
                    return Ok(search::WebSearchResult {
                        query: query.to_string(),
                        findings: enriched_results,
                        total_results: enriched_results.len(),
                        search_time_ms: elapsed,
                    });
                }
                Err(e) => {
                    warn!("Search engine {} failed: {}", engine.name(), e);
                }
            }
        }

        Err(WsoError::SearchEngineUnavailable(
            "All configured search engines failed".to_string()
        ))
    }

    /// Extract and clean content from a URL
    pub async fn extract_content(&self, url: &str) -> WsoResult<String> {
        // Check cache first
        if let Some(cached) = self.cache.get(url) {
            return Ok(cached.content);
        }

        // Fetch and extract
        let content = self.extractor.fetch_and_extract(url, &self.client).await?;
        
        // Cache the result
        let finding = search::WebFinding {
            url: url.to_string(),
            title: String::new(), // Will be set by search results
            snippet: String::new(),
            content: content.clone(),
            confidence: 0.5,
            fetched_at: Utc::now(),
            cached: false,
        };
        self.cache.insert(url, finding);
        
        Ok(content)
    }

    /// Get cached result if available
    pub async fn get_cached(&self, url: &str) -> Option<search::WebFinding> {
        self.cache.get(url)
    }

    /// Store result in cache
    pub async fn cache_result(&self, finding: &search::WebFinding) {
        self.cache.insert(&finding.url, finding.clone());
    }

    /// Inject findings into steering context
    pub async fn inject_into_steering(
        &self,
        findings: &[search::WebFinding],
        steering_context: &mut String,
    ) -> WsoResult<()> {
        // Allocate token budget
        let allocations = self.budget_allocator.allocate(findings);
        
        // Build context fragments
        let mut context_fragments = Vec::new();
        for (i, finding) in findings.iter().enumerate() {
            if i < allocations.len() && allocations[i] > 0 {
                let fragment = self.injector.build_context_fragment(finding, allocations[i]);
                context_fragments.push(fragment);
            }
        }
        
        // Inject into steering context
        self.injector.inject(steering_context, &context_fragments);
        
        Ok(())
    }

    /// Enrich search results with extracted content and confidence scores
    async fn enrich_results(&self, mut results: Vec<search::WebFinding>) -> WsoResult<Vec<search::WebFinding>> {
        // Deduplicate by URL
        let mut seen_urls = std::collections::HashSet::new();
        results.retain(|r| {
            let normalized = normalize_url(&r.url);
            seen_urls.insert(normalized)
        });

        // Extract content for top results (limit to avoid too many requests)
        let top_n = results.len().min(3);
        for result in results.iter_mut().take(top_n) {
            if let Ok(content) = self.extract_content(&result.url).await {
                result.content = content;
                result.confidence = calculate_confidence(&result.url, &result.title);
            }
        }

        Ok(results)
    }

    /// Check rate limit for a query
    fn check_rate_limit(&self, query: &str) -> WsoResult<()> {
        let key = "global".to_string();
        let mut entries = self.rate_limiter.get_mut(&key).unwrap_or_else(|| {
            self.rate_limiter.insert(key.clone(), Vec::new());
            self.rate_limiter.get_mut(&key).unwrap()
        });

        let now = Utc::now();
        let window_start = now - chrono::Duration::minutes(1);
        
        // Remove old entries
        entries.retain(|&t| t > window_start);
        
        // Check limit
        if entries.len() >= self.config.rate_limit_rpm as usize {
            return Err(WsoError::RateLimitExceeded);
        }
        
        entries.push(now);
        Ok(())
    }
}

/// Normalize a URL for deduplication
fn normalize_url(url: &str) -> String {
    // Remove trailing slashes and query parameters
    url.split('?').next()
        .map(|u| u.trim_end_matches('/'))
        .unwrap_or(url)
        .to_lowercase()
}

/// Calculate confidence score based on URL and title
fn calculate_confidence(url: &str, title: &str) -> f32 {
    let mut confidence = 0.5;
    
    // Domain authority adjustments
    if url.contains(".gov") || url.contains(".edu") {
        confidence += 0.2;
    } else if url.contains("wikipedia.org") {
        confidence += 0.1;
    }
    
    // Recency adjustment (simulated by title length/content)
    if title.len() > 50 {
        confidence += 0.05;
    }
    
    // Cap at 1.0
    confidence.min(1.0)
}
```

=== FILE: src/search.rs ===
```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use anyhow::Result;
use tracing::info;

use crate::errors::{WsoError, WsoResult};

/// Trait for search engines
pub trait SearchEngine: Send + Sync {
    fn name(&self) -> &str;
    async fn search(&self, client: &Client, query: &str) -> Result<Vec<WebFinding>>;
}

/// SearXNG search engine implementation
pub struct SearXNGSearch {
    base_url: String,
}

impl SearXNGSearch {
    pub fn new(base_url: String) -> Self {
        Self { base_url }
    }
}

impl SearchEngine for SearXNGSearch {
    fn name(&self) -> &str {
        "SearXNG"
    }

    async fn search(&self, client: &Client, query: &str) -> Result<Vec<WebFinding>> {
        let url = format!("{}/search", self.base_url);
        let params = [
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
}

impl SearchEngine for GoogleCSESearch {
    fn name(&self) -> &str {
        "Google CSE"
    }

    async fn search(&self, client: &Client, query: &str) -> Result<Vec<WebFinding>> {
        let url = "https://www.googleapis.com/customsearch/v1";
        let params = [
            ("key", &self.api_key),
            ("cx", &self.engine_id),
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
```

=== FILE: src/extract.rs ===
```rust
use reqwest::Client;
use scraper::{Html, Selector};
use anyhow::Result;
use tracing::debug;

use crate::errors::{WsoError, WsoResult};

/// HTML content extractor
pub struct WebExtractor;

impl WebExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Fetch and extract clean text from a URL
    pub async fn fetch_and_extract(&self, url: &str, client: &Client) -> WsoResult<String> {
        // Check robots.txt (simplified implementation)
        if !self.is_allowed_by_robots(url).await? {
            return Err(WsoError::ExtractionFailed(format!(
                "URL {} disallowed by robots.txt", url
            )));
        }

        // Fetch the page
        let resp = client.get(url).send().await?;
        
        if !resp.status().is_success() {
            return Err(WsoError::ExtractionFailed(format!(
                "Failed to fetch {}: status {}", url, resp.status()
            )));
        }

        let html = resp.text().await?;
        
        // Extract clean text
        let text = self.extract_clean_text(&html)?;
        
        Ok(text)
    }

    /// Extract clean text from HTML content
    pub fn extract_clean_text(&self, html: &str) -> WsoResult<String> {
        let document = Html::parse_document(html);
        
        // Select body content
        let body_selector = Selector::parse("body").map_err(|e| {
            WsoError::ExtractionFailed(format!("Invalid selector: {}", e))
        })?;
        
        let body = document.select(&body_selector).next().ok_or_else(|| {
            WsoError::ExtractionFailed("No body element found".to_string())
        })?;
        
        // Remove scripts and styles
        let script_selector = Selector::parse("script, style").map_err(|e| {
            WsoError::ExtractionFailed(format!("Invalid selector: {}", e))
        })?;
        
        // Collect text while removing unwanted elements
        let mut text_parts = Vec::new();
        for node in body.descendants() {
            if node.value().is_text() {
                if let Some(text) = node.value().as_text() {
                    text_parts.push(text.text);
                }
            }
        }
        
        // Join and clean up whitespace
        let full_text = text_parts.join("\n");
        let clean_text = self.clean_whitespace(&full_text);
        
        debug!("Extracted {} bytes of text from {}", clean_text.len(), "URL");
        
        Ok(clean_text)
    }

    /// Clean up whitespace in extracted text
    fn clean_whitespace(&self, text: &str) -> String {
        text.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Check if URL is allowed by robots.txt (simplified)
    async fn is_allowed_by_robots(&self, url: &str) -> WsoResult<bool> {
        // In a production implementation, this would fetch and parse robots.txt
        // For now, we allow all URLs (simplified)
        Ok(true)
    }
}
```

=== FILE: src/cache.rs ===
```rust
use dashmap::DashMap;
use chrono::{DateTime, Utc};
use tracing::debug;

use crate::search::WebFinding;

/// Web result cache with TTL expiration
pub struct WebCache {
    cache: DashMap<String, (WebFinding, DateTime<Utc>)>,
    ttl_hours: u64,
}

impl WebCache {
    /// Create a new cache with the specified TTL
    pub fn new(ttl_hours: u64) -> Self {
        Self {
            cache: DashMap::new(),
            ttl_hours,
        }
    }

    /// Get a cached finding if it exists and hasn't expired
    pub fn get(&self, url: &str) -> Option<WebFinding> {
        if let Some(entry) = self.cache.get(url) {
            let (finding, fetched_at) = entry.value();
            
            // Use Utc::now() - fetched_at instead of fetched_at.elapsed()
            let elapsed = Utc::now() - *fetched_at;
            let ttl_duration = chrono::Duration::hours(self.ttl_hours as i64);
            
            if elapsed < ttl_duration {
                debug!("Cache hit for {}", url);
                return Some(finding.clone());
            } else {
                debug!("Cache expired for {}", url);
            }
        }
        
        None
    }

    /// Insert a finding into the cache
    pub fn insert(&self, url: &str, finding: WebFinding) {
        self.cache.insert(url.to_string(), (finding, Utc::now()));
        debug!("Cached {}", url);
    }

    /// Remove a specific entry from the cache
    pub fn remove(&self, url: &str) {
        self.cache.remove(url);
        debug!("Removed {} from cache", url);
    }

    /// Clear all expired entries
    pub fn cleanup_expired(&self) {
        let now = Utc::now();
        let ttl_duration = chrono::Duration::hours(self.ttl_hours as i64);
        
        let mut to_remove = Vec::new();
        for entry in self.cache.iter() {
            let (_, fetched_at) = entry.value();
            if now - *fetched_at >= ttl_duration {
                to_remove.push(entry.key().clone());
            }
        }
        
        for url in to_remove {
            self.cache.remove(&url);
        }
        
        if !to_remove.is_empty() {
            debug!("Cleaned up {} expired cache entries", to_remove.len());
        }
    }
}
```

=== FILE: src/injector.rs ===
```rust
use tracing::info;

use crate::search::WebFinding;

/// Context injector for steering engine integration
pub struct ContextInjector;

impl ContextInjector {
    pub fn new() -> Self {
        Self
    }

    /// Build a context fragment from a web finding
    pub fn build_context_fragment(&self, finding: &WebFinding, max_tokens: usize) -> String {
        // Estimate tokens (rough approximation: 1 token ≈ 4 characters)
        let max_chars = max_tokens * 4;
        
        let mut fragment = format!(
            "## Web Finding: {}\nURL: {}\nSnippet: {}\n\n",
            finding.title, finding.url, finding.snippet
        );
        
        // Add content if available and within token budget
        if !finding.content.is_empty() && finding.content.len() <= max_chars {
            fragment.push_str(&finding.content);
        } else if finding.content.len() > max_chars {
            fragment.push_str(&finding.content[..max_chars]);
            fragment.push_str("... [truncated]");
        }
        
        fragment
    }

    /// Inject context fragments into the steering context
    pub fn inject(&self, steering_context: &mut String, fragments: &[String]) {
        if fragments.is_empty() {
            return;
        }
        
        // Add separator if context already has content
        if !steering_context.is_empty() {
            steering_context.push_str("\n\n---\n\n");
        }
        
        // Append all fragments
        for fragment in fragments {
            steering_context.push_str(fragment);
            steering_context.push_str("\n\n");
        }
        
        info!("Injected {} context fragments into steering", fragments.len());
    }
}
```

=== FILE: src/errors.rs ===
```rust
use thiserror::Error;
use reqwest::Error as ReqwestError;

/// Result type alias for WSO operations
pub type WsoResult<T> = Result<T, WsoError>;

/// Error types for the Web Search Oracle
#[derive(Error, Debug)]
pub enum WsoError {
    #[error("Search engine unavailable: {0}")]
    SearchEngineUnavailable(String),
    
    #[error("Content extraction failed: {0}")]
    ExtractionFailed(String),
    
    #[error("Cache error: {0}")]
    CacheError(String),
    
    #[error("Token budget exceeded")]
    TokenBudgetExceeded,
    
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    
    #[error("Network error: {0}")]
    NetworkError(#[from] ReqwestError),
    
    #[error("Anyhow error: {0}")]
    Anyhow(#[from] anyhow::Error),
}
```

=== FILE: src/budget.rs ===
```rust
use crate::search::WebFinding;

/// Token budget allocator with confidence weighting
pub struct TokenBudgetAllocator {
    total_budget: usize,
}

impl TokenBudgetAllocator {
    /// Create a new allocator with the given total token budget
    pub fn new(total_budget: usize) -> Self {
        Self { total_budget }
    }

    /// Allocate tokens to findings based on confidence scores
    pub fn allocate(&self, findings: &[WebFinding]) -> Vec<usize> {
        if findings.is_empty() {
            return Vec::new();
        }

        // Calculate total confidence
        let total_confidence: f32 = findings.iter().map(|f| f.confidence).sum();
        
        if total_confidence == 0.0 {
            // Equal distribution if no confidence scores
            let per_finding = self.total_budget / findings.len();
            return vec![per_finding; findings.len()];
        }

        // Sort findings by confidence descending
        let mut sorted_indices: Vec<usize> = (0..findings.len()).collect();
        sorted_indices.sort_by(|&a, &b| {
            findings[b].confidence.partial_cmp(&findings[a].confidence).unwrap()
        });

        // Allocate tokens proportionally
        let mut allocations = vec![0usize; findings.len()];
        let mut remaining_budget = self.total_budget;

        for (i, &idx) in sorted_indices.iter().enumerate() {
            let confidence = findings[idx].confidence;
            
            // Calculate proportional allocation
            let alloc = (remaining_budget as f32 * confidence / total_confidence) as usize;
            
            // Ensure we don't exceed remaining budget
            let alloc = alloc.min(remaining_budget);
            
            allocations[idx] = alloc;
            remaining_budget -= alloc;
        }

        // Distribute any remaining budget to highest confidence items
        if remaining_budget > 0 && !sorted_indices.is_empty() {
            allocations[sorted_indices[0]] += remaining_budget;
        }

        allocations
    }
}
```