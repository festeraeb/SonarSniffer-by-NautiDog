//! CESAROPS Detection Pipeline — Rust workflow orchestrator
//!
//! Coordinates the Triple-Lock detection across vision workers:
//!   Scout → Validator → Jitter (cesarops2 GPUs or T440 CPU fallback)
//!
//! Also serves as the n8n-style task dispatcher for scan jobs.

mod types;
mod endpoint_pool;
mod workers;
mod pipeline;
mod dispatcher;
pub mod bag_scanner;

use std::sync::Arc;
use axum::{routing::{get, post}, Router};
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

use crate::dispatcher::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    tracing::info!("CESAROPS Detection Pipeline starting...");

    let state = Arc::new(AppState::new());

    let app = Router::new()
        .route("/health", get(dispatcher::health))
        .route("/scan", post(dispatcher::submit_scan))
        .route("/scan/{id}", get(dispatcher::get_scan_status))
        .route("/workers", get(dispatcher::list_workers))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = std::env::var("DETECTION_PORT").unwrap_or_else(|_| "5580".to_string());
    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
