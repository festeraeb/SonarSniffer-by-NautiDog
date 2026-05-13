

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
use crate::search::{SearchBackend, WebFinding, WebSearchResult};
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
    search_backend: Option<SearchBackend>,
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

        let search_backend = {
            // Add SearXNG if configured
            if let Some(url) = &config.searxng_url {
                info!("Initializing SearXNG search engine at {}", url);
                Some(SearchBackend::SearXNG(crate::search::SearXNGSearch::new(url.clone())))
            } else if config.enable_google_fallback {
                // Add Google CSE if configured and fallback enabled
                if let (Some(key), Some(cx)) = (&config.google_cse_key, &config.google_cse_engine_id) {
                    info!("Initializing Google CSE fallback");
                    Some(SearchBackend::GoogleCSE(crate::search::GoogleCSESearch::new(key.clone(), cx.clone())))
                } else {
                    warn!("Google CSE fallback enabled but API key or CX not configured");
                    None
                }
            } else {
                None
            }
        };

        if search_backend.is_none() {
            warn!("No search engines configured. Web search will fail.");
        }

        Self {
            config,
            client,
            cache,
            search_backend,
            extractor,
            budget_allocator,
            injector,
            rate_limiter: Arc::new(DashMap::new()),
        }
    }

    /// Execute a web search with the given query
    pub async fn search(&self, query: &str) -> WsoResult<WebSearchResult> {
        // Rate limiting check
        self.check_rate_limit(query)?;

        let start = std::time::Instant::now();
        
        // Try the search backend
        let results = match &self.search_backend {
            Some(backend) => backend.search(&self.client, query).await?,
            None => {
                return Err(WsoError::SearchEngineUnavailable(
                    "No search engines configured".to_string()
                ));
            }
        };

        let elapsed = start.elapsed().as_millis() as u64;
        info!("Search completed via {} in {}ms", backend_name(&self.search_backend), elapsed);
        
        // Process and enrich results
        let enriched_results = self.enrich_results(results).await?;
        
        Ok(WebSearchResult {
            query: query.to_string(),
            findings: enriched_results,
            total_results: enriched_results.len(),
            search_time_ms: elapsed,
        })
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
        let finding = WebFinding {
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
    pub async fn get_cached(&self, url: &str) -> Option<WebFinding> {
        self.cache.get(url)
    }

    /// Store result in cache
    pub async fn cache_result(&self, finding: &WebFinding) {
        self.cache.insert(&finding.url, finding.clone());
    }

    /// Inject findings into steering context
    pub async fn inject_into_steering(
        &self,
        findings: &[WebFinding],
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
    async fn enrich_results(&self, mut results: Vec<WebFinding>) -> WsoResult<Vec<WebFinding>> {
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

fn backend_name(backend: &Option<SearchBackend>) -> &'static str {
    match backend {
        Some(SearchBackend::SearXNG(_)) => "SearXNG",
        Some(SearchBackend::GoogleCSE(_)) => "Google CSE",
        None => "None",
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
    let mut confidence: f32 = 0.5;
    
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

    pub async fn search(&self, client: &Client, query: &str) -> Result<Vec<WebFinding>> {
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
                    text_parts.push(text.to_string());
                }
            }
        }
        
        // Join and clean up whitespace
        let full_text = text_parts.join(" ");
        let clean_text = self.clean_whitespace(&full_text);
        
        debug!("Extracted {} bytes of text from URL", clean_text.len());
        
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
        
        for url in &to_remove {
            self.cache.remove(url);
        }
        
        if !to_remove.is_empty() {
            debug!("Cleaned up {} expired cache entries", to_remove.len());
        }
    }
}
```