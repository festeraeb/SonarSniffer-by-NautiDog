use axum::{routing::{get, post}, extract::Json, Router};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use cesarops_wso::{WsoEngine, WsoConfig};

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    #[serde(default = "default_max")]
    max_results: usize,
}

fn default_max() -> usize { 5 }

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    service: String,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        service: "cesarops-wso".to_string(),
    })
}

async fn search(
    axum::extract::State(engine): axum::extract::State<Arc<WsoEngine>>,
    Json(req): Json<SearchRequest>,
) -> Json<serde_json::Value> {
    match engine.search(&req.query).await {
        Ok(result) => {
            let findings: Vec<serde_json::Value> = result.findings.into_iter()
                .take(req.max_results)
                .map(|f| serde_json::json!({
                    "title": f.title,
                    "url": f.url,
                    "snippet": f.snippet,
                    "confidence": f.confidence,
                }))
                .collect();
            let total = findings.len();
            Json(serde_json::json!({
                "query": result.query,
                "results": findings,
                "total": total,
            }))
        }
        Err(e) => {
            tracing::warn!("WSO search failed: {}", e);
            Json(serde_json::json!({
                "query": req.query,
                "results": [],
                "error": format!("{}", e),
            }))
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let config = WsoConfig {
        // Local Searxng instance, JSON API enabled. Override with SEARXNG_URL env.
        searxng_url: Some(
            std::env::var("SEARXNG_URL")
                .unwrap_or_else(|_| "http://localhost:8888".to_string()),
        ),
        google_cse_key: std::env::var("GOOGLE_CSE_KEY").ok(),
        google_cse_engine_id: std::env::var("GOOGLE_CSE_ENGINE_ID").ok(),
        max_web_context_tokens: 4000,
        cache_ttl_hours: 24,
        enable_google_fallback: std::env::var("GOOGLE_CSE_KEY").is_ok(),
        sovereign_cloud_base_url: "http://localhost:8765".to_string(),
        rate_limit_rpm: 30,
    };

    let engine = Arc::new(WsoEngine::new(config));

    let app = Router::new()
        .route("/health", get(health))
        .route("/search", post(search))
        .with_state(engine);

    let addr = SocketAddr::from(([0, 0, 0, 0], 5010));
    tracing::info!("cesarops-wso server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
