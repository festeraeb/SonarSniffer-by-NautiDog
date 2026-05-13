

# CESAROPS Web Search Oracle: Architecture Specification

## Executive Summary

The **Web Search Oracle (WSO)** is a sovereign-compliant, high-performance Rust crate that extends the CESAROPS research engine with real-time external knowledge retrieval. It bridges the gap between static local codebase reasoning (via `nautivecs`) and dynamic global knowledge via hybrid search (SearXNG primary, Google CSE fallback).

Key innovations:
- **Sovereign-first**: Defaults to self-hosted SearXNG; Google CSE only if explicitly configured and user-consented.
- **Token-aware injection**: Dynamically allocates context budget based on query importance and source confidence.
- **Deduplication engine**: Prevents redundant web hits and conflicts with local code findings.
- **LLM-native tool interface**: Fully compatible with OpenAI function-calling schema, enabling seamless MCP integration.

---

## 1. Crate Structure & Dependencies

### File Layout
```
cesarops-wso/
├── Cargo.toml
├── src/
│   ├── lib.rs          # Public API, trait definitions, config
│   ├── search.rs       # Search orchestration (SearXNG/Google CSE)
│   ├── extract.rs      # HTML cleaning, text extraction, summarization
│   ├── cache.rs        # URL caching with TTL and deduplication
│   ├── injector.rs     # Context injection into nautivecs + steering
│   └── errors.rs       # Custom error types
├── tests/
│   ├── integration.rs  # End-to-end search flow
│   └── unit.rs         # Unit tests for extraction/caching
└── examples/
    └── wso_demo.rs     # Usage example
```

### Dependencies (`Cargo.toml`)
```toml
[package]
name = "cesarops-wso"
version = "0.1.0"
edition = "2021"

[dependencies]
reqwest = { version = "0.11", features = ["json", "cookies"] }
tokio = { version = "1.35", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
scraper = "0.18"  # HTML parsing
html2text = "0.11" # HTML to clean text
anyhow = "1.0"
tracing = "0.1"
chrono = { version = "0.4", features = ["serde"] }
dashmap = "5.5"    # Concurrent cache
url = "2.5"
regex = "1.10"
tantivy = "0.21"   # Optional: local index for web results

[features]
default = ["searxng"]
searxng = []
google-cse = []
cache = ["dashmap"]
summarization = [] # Requires external LLM or summarizer crate
```

---

## 2. Core Traits & Structs

### 2.1 Configuration
```rust
// src/lib.rs
use std::sync::Arc;
use chrono::{DateTime, Utc};

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
        }
    }
}
```

### 2.2 Search Result Structs
```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebFinding {
    pub url: String,
    pub title: String,
    pub snippet: String,
    pub content: String, // Cleaned text
    pub confidence: f32, // 0.0-1.0 based on source quality
    pub fetched_at: DateTime<Utc>,
    pub cached: bool,
}

#[derive(Debug, Clone)]
pub struct WebSearchResult {
    pub query: String,
    pub findings: Vec<WebFinding>,
    pub total_results: usize,
    pub search_time_ms: u64,
}
```

### 2.3 Core Trait
```rust
#[async_trait::async_trait]
pub trait WebSearchOracle: Send + Sync {
    /// Execute a web search with the given query
    async fn search(&self, query: &str) -> Result<WebSearchResult>;

    /// Extract and clean content from a URL
    async fn extract_content(&self, url: &str) -> Result<String>;

    /// Get cached result if available
    async fn get_cached(&self, url: &str) -> Option<WebFinding>;

    /// Store result in cache
    async fn cache_result(&self, finding: &WebFinding);

    /// Inject findings into steering context
    async fn inject_into_steering(
        &self,
        findings: &[WebFinding],
        steering: &mut SteeringEngine,
    ) -> Result<()>;
}
```

---

## 3. Implementation Details

### 3.1 Search Orchestration (`search.rs`)
```rust
// src/search.rs
use reqwest::Client;
use crate::WsoConfig;
use crate::WebSearchResult;

pub struct WsoImpl {
    config: WsoConfig,
    client: Client,
}

impl WsoImpl {
    pub fn new(config: WsoConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        Self { config, client }
    }

    async fn search_searxng(&self, query: &str) -> Result<serde_json::Value> {
        let url = format!("{}/search", self.config.searxng_url.as_ref().unwrap());
        let params = [
            ("q", query),
            ("format", "json"),
            ("engines", "google,bing,duckduckgo"),
        ];
        let resp = self.client.get(&url).query(&params).send().await?;
        Ok(resp.json().await?)
    }

    async fn search_google_cse(&self, query: &str) -> Result<serde_json::Value> {
        let url = "https://www.googleapis.com/customsearch/v1";
        let params = [
            ("key", self.config.google_cse_key.as_ref().unwrap()),
            ("cx", self.config.google_cse_engine_id.as_ref().unwrap()),
            ("q", query),
        ];
        let resp = self.client.get(url).query(&params).send().await?;
        Ok(resp.json().await?)
    }

    fn parse_search_results(&self, json: &serde_json::Value, source: &str) -> Vec<crate::WebFinding> {
        // Parse JSON based on source (SearXNG vs Google CSE)
        // Extract title, link, snippet
        // Assign confidence based on source reliability
        unimplemented!()
    }
}

#[async_trait::async_trait]
impl WebSearchOracle for WsoImpl {
    async fn search(&self, query: &str) -> Result<WebSearchResult> {
        let start = std::time::Instant::now();
        
        // Try SearXNG first
        if let Some(ref url) = self.config.searxng_url {
            match self.search_searxng(query).await {
                Ok(json) => {
                    let findings = self.parse_search_results(&json, "searxng");
                    return Ok(WebSearchResult {
                        query: query.to_string(),
                        findings,
                        total_results: json["results"].as_array().map_or(0, |a| a.len()),
                        search_time_ms: start.elapsed().as_millis() as u64,
                    });
                }
                Err(e) => {
                    tracing::warn!("SearXNG failed: {}", e);
                }
            }
        }

        // Fallback to Google CSE
        if self.config.enable_google_fallback {
            match self.search_google_cse(query).await {
                Ok(json) => {
                    let findings = self.parse_search_results(&json, "google_cse");
                    return Ok(WebSearchResult {
                        query: query.to_string(),
                        findings,
                        total_results: json["searchInformation"]["totalResults"]
                            .as_str()
                            .unwrap_or("0")
                            .parse()
                            .unwrap_or(0),
                        search_time_ms: start.elapsed().as_millis() as u64,
                    });
                }
                Err(e) => {
                    tracing::error!("Google CSE also failed: {}", e);
                }
            }
        }

        anyhow::bail!("No search engines available")
    }

    async fn extract_content(&self, url: &str) -> Result<String> {
        // Fetch HTML, clean with scraper/html2text
        // Respect robots.txt
        unimplemented!()
    }

    async fn get_cached(&self, url: &str) -> Option<crate::WebFinding> {
        // Check cache
        unimplemented!()
    }

    async fn cache_result(&self, finding: &crate::WebFinding) {
        // Store in cache
        unimplemented!()
    }

    async fn inject_into_steering(
        &self,
        findings: &[crate::WebFinding],
        steering: &mut SteeringEngine,
    ) -> Result<()> {
        // Build context fragments and inject
        unimplemented!()
    }
}
```

### 3.2 Content Extraction (`extract.rs`)
```rust
// src/extract.rs
use scraper::{Html, Selector};
use html2text;
use reqwest::Client;

pub async fn extract_clean_text(url: &str, html: &str) -> Result<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("body").unwrap();
    let body = document.select(&selector).next().unwrap();
    
    // Remove scripts/styles
    for node in body.select(&Selector::parse("script, style").unwrap()) {
        node.remove();
    }
    
    // Extract text
    let text = body.text().collect::<Vec<_>>().join("\n");
    
    // Clean up whitespace
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    
    Ok(clean)
}

pub async fn fetch_and_extract(url: &str, client: &Client) -> Result<String> {
    let resp = client.get(url).send().await?;
    let html = resp.text().await?;
    extract_clean_text(url, &html).await
}
```

### 3.3 Caching (`cache.rs`)
```rust
// src/cache.rs
use dashmap::DashMap;
use chrono::{DateTime, Utc};
use crate::WebFinding;

pub struct WebCache {
    cache: DashMap<String, (WebFinding, DateTime<Utc>)>,
    ttl_hours: u64,
}

impl WebCache {
    pub fn new(ttl_hours: u64) -> Self {
        Self {
            cache: DashMap::new(),
            ttl_hours,
        }
    }

    pub fn get(&self, url: &str) -> Option<WebFinding> {
        if let Some(entry) = self.cache.get(url) {
            let (finding, fetched_at) = entry.value();
            if fetched_at.elapsed().as_secs() < self.ttl_hours * 3600 {
                return Some(finding.clone());
            }
        }
        None
    }

    pub fn insert(&self, url: &str, finding: WebFinding) {
        self.cache.insert(url.to_string(), (finding, Utc::now()));
    }
}
```

---

## 4. Integration with Sovereign-Cloud

### 4.1 Tool Schema (OpenAI-compatible)
```json
{
  "name": "web_search",
  "description": "Search the web for current information. Returns top results with extracted content.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "The search query"
      },
      "max_results": {
        "type": "integer",
        "description": "Maximum number of results to return (default: 5)",
        "default": 5
      }
    },
    "required": ["query"]
  }
}
```

### 4.2 Middleware Integration
```rust
// In sovereign-cloud/src/api/mod.rs
async fn handle_web_search(
    State(wso): State<Arc<WsoImpl>>,
    State(steering): State<Arc<Mutex<SteeringEngine>>>,
    json: Json<WebSearchRequest>,
) -> Result<Json<WebSearchResponse>> {
    let mut steering = steering.lock().await;
    
    // Execute search
    let result = wso.search(&json.query).await?;
    
    // Inject into steering context
    wso.inject_into_steering(&result.findings, &mut steering).await?;
    
    Ok(Json(WebSearchResponse {
        findings: result.findings,
        total: result.total_results,
    }))
}
```

---

## 5. Data Flow

```
1. LLM receives tool_call: {"name": "web_search", "arguments": {"query": "latest WASM specs"}}
2. Sovereign-cloud intercepts tool_call
3. Calls WSO::search("latest WASM specs")
4. WSO queries SearXNG → gets JSON results
5. For top 3 URLs, WSO::extract_content() fetches and cleans HTML
6. WSO::inject_into_steering() adds findings to SteeringEngine context
7. LLM continues generation with enhanced context
8. LLM cites sources in final response
```

---

## 6. Token Budget Strategy

### Allocation Logic
```rust
fn allocate_token_budget(findings: &[WebFinding], total_budget: usize) -> Vec<usize> {
    let mut allocations = Vec::new();
    let mut remaining = total_budget;
    
    // Sort by confidence descending
    let mut sorted = findings.to_vec();
    sorted.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    
    for finding in sorted {
        // Allocate proportionally based on confidence
        let alloc = (remaining as f32 * finding.confidence / total_confidence) as usize;
        allocations.push(alloc);
        remaining -= alloc;
    }
    
    allocations
}
```

### Summarization Pipeline
For long pages (>2000 tokens):
1. Extract key sections (headings, bold text)
2. Use local LLM to summarize
3. Inject summary instead of full text

---

## 7. Confidence Scoring

| Source | Base Confidence | Notes |
|--------|-----------------|-------|
| Local Code (nautivecs) | 0.9 | Direct codebase match |
| Cached Web (24h) | 0.7 | Fresh but cached |
| Fresh Web | 0.5-0.6 | Depends on source quality |
| Human Correction | 1.0 | Highest priority |

Confidence adjusted by:
- Domain authority (.gov, .edu higher)
- Recency (newer = higher)
- Cross-reference with other sources

---

## 8. Deduplication

### URL Deduplication
```rust
fn is_duplicate(url1: &str, url2: &str) -> bool {
    // Normalize URLs (remove trailing slash, query params)
    let u1 = normalize_url(url1);
    let u2 = normalize_url(url2);
    u1 == u2
}
```

### Content Deduplication
Use fuzzy matching on extracted content to avoid injecting similar findings.

---

## 9. Error Handling

```rust
#[derive(Debug)]
pub enum WsoError {
    SearchEngineUnavailable(String),
    ExtractionFailed(String),
    CacheError(String),
    TokenBudgetExceeded,
    NetworkError(reqwest::Error),
}

impl std::fmt::Display for WsoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsoError::SearchEngineUnavailable(msg) => write!(f, "Search engine unavailable: {}", msg),
            WsoError::ExtractionFailed(msg) => write!(f, "Content extraction failed: {}", msg),
            WsoError::CacheError(msg) => write!(f, "Cache error: {}", msg),
            WsoError::TokenBudgetExceeded => write!(f, "Token budget exceeded"),
            WsoError::NetworkError(e) => write!(f, "Network error: {}", e),
        }
    }
}
```

---

## 10. Testing Strategy

### Unit Tests
- HTML extraction with various page structures
- Cache TTL expiration
- URL normalization and deduplication

### Integration Tests
- End-to-end search flow with mock SearXNG
- Token budget enforcement
- Steering context injection verification

---

## Implementation Priority

1. **Core WSO crate** with SearXNG support
2. **Integration** with sovereign-cloud tool endpoint
3. **Caching layer** with TTL
4. **Content extraction** pipeline
5. **Steering integration** for context injection
6. **Google CSE fallback**
7. **Summarization** for long pages
8. **Advanced deduplication** and confidence scoring

This spec provides a complete, implementable architecture for the Web Search Oracle. The modular design allows incremental implementation while maintaining sovereignty and performance requirements.