//! HTTP server for nautivecs — exposes search as an OpenAI-compatible API.
//!
//! Endpoints:
//!   GET  /health              — liveness check
//!   POST /v1/search           — OpenAI-style search (returns context fragments)
//!   POST /query               — Simple query (for frontend/curl)
//!   GET  /stats               — Index statistics

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use crate::{Config, ContextFragment, InjectedContextBuilder, NautivecsEngine};

pub struct AppState {
    pub engine: RwLock<NautivecsEngine>,
    pub config: Config,
}

#[derive(Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default)]
    pub include_context: bool,
}

fn default_top_k() -> usize {
    5
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultItem>,
    pub context_block: Option<String>,
    pub total_chunks: usize,
}

#[derive(Serialize)]
pub struct SearchResultItem {
    pub score: f32,
    pub file_path: String,
    pub function_name: String,
    pub symbol_type: String,
    pub line_start: usize,
    pub line_end: usize,
    pub text: String,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub service: &'static str,
    pub status: &'static str,
    pub chunks: usize,
}

async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let engine = state.engine.read().await;
    Json(HealthResponse {
        service: "nautivecs",
        status: "ok",
        chunks: engine.chunk_count(),
    })
}

async fn stats(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let engine = state.engine.read().await;
    Json(serde_json::json!({
        "chunks_indexed": engine.chunk_count(),
        "store_path": state.config.db_path.display().to_string(),
        "embedding_endpoint": state.config.embedding_endpoint,
    }))
}

async fn search(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    let engine = state.engine.read().await;

    let results = engine
        .query(&req.query, req.top_k)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Search failed: {}", e)))?;

    let items: Vec<SearchResultItem> = results
        .iter()
        .map(|r| SearchResultItem {
            score: r.score,
            file_path: r.file_path.clone(),
            function_name: r.function_name.clone(),
            symbol_type: r.symbol_type.clone(),
            line_start: r.line_start,
            line_end: r.line_end,
            text: r.text.clone(),
        })
        .collect();

    let context_block = if req.include_context {
        let fragments: Vec<ContextFragment> = results.iter().map(ContextFragment::from).collect();
        let builder = InjectedContextBuilder::new(4096, true);
        Some(builder.build_system_context(&fragments))
    } else {
        None
    };

    Ok(Json(SearchResponse {
        results: items,
        context_block,
        total_chunks: engine.chunk_count(),
    }))
}

pub async fn run_server(engine: NautivecsEngine, config: Config, port: u16) -> anyhow::Result<()> {
    let state = Arc::new(AppState {
        engine: RwLock::new(engine),
        config,
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/stats", get(stats))
        .route("/query", post(search))
        .route("/v1/search", post(search))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    println!("nautivecs server listening on {}", addr);
    println!("  POST /query       - search for frontend/curl");
    println!("  POST /v1/search   - OpenAI-compatible search");
    println!("  GET  /health      - liveness");
    println!("  GET  /stats       - index stats");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
