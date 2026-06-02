//! HTTP server for nautivecs — exposes search as an OpenAI-compatible API.
//!
//! Endpoints:
//!   GET  /health              — liveness check
//!   POST /v1/search           — OpenAI-style search (returns context fragments)
//!   POST /query               — Simple query (for frontend/curl)
//!   GET  /stats               — Index statistics
//!   POST /add                 — Ingest a lesson or document fragment

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

#[derive(Deserialize)]
pub struct AddRequest {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tags: String,
    #[serde(default = "default_lesson_source")]
    pub source: String,
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub metadata: Option<LegacyAddMetadata>,
}

#[derive(Deserialize)]
pub struct LegacyAddMetadata {
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub source: String,
}

fn default_lesson_source() -> String {
    "lessons_learned".to_string()
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

async fn add_document(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let text = if req.text.trim().is_empty() {
        req.content.trim().to_string()
    } else {
        req.text.trim().to_string()
    };

    if text.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "text is required".to_string()));
    }

    let tags = if req.tags.trim().is_empty() {
        req.metadata
            .as_ref()
            .map(|m| m.tags.trim().to_string())
            .unwrap_or_default()
    } else {
        req.tags.trim().to_string()
    };

    let source = if req.source.trim().is_empty() || req.source == default_lesson_source() {
        req.metadata
            .as_ref()
            .map(|m| m.source.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(default_lesson_source)
    } else {
        req.source.trim().to_string()
    };

    let file_path = if req.file_path.is_empty() {
        format!("lessons/{}.md", source)
    } else {
        req.file_path.clone()
    };
    let symbol = if tags.is_empty() {
        source.clone()
    } else {
        tags.clone()
    };

    let mut engine = state.engine.write().await;
    let inserted = engine
        .ingest_document(&text, &file_path, &symbol, &tags)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Ingest failed: {}", e)))?;

    Ok(Json(serde_json::json!({
        "status": "ok",
        "chunks_inserted": inserted,
        "file_path": file_path,
        "tags": tags,
    })))
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
        .route("/add", post(add_document))
        .route("/ingest", post(add_document))
        .route("/query", post(search))
        .route("/v1/search", post(search))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    println!("nautivecs server listening on {}", addr);
    println!("  POST /add         - ingest lesson/document");
    println!("  POST /query       - search for frontend/curl");
    println!("  POST /v1/search   - OpenAI-compatible search");
    println!("  GET  /health      - liveness");
    println!("  GET  /stats       - index stats");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
