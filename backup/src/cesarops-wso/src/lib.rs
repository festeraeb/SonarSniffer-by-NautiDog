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
        let total = enriched_results.len();
        
        Ok(WebSearchResult {
            query: query.to_string(),
            findings: enriched_results,
            total_results: total,
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
pub mod duckduckgo;
pub mod brave;
