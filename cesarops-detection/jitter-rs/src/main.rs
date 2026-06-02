//! CESAROPS Lock-3 jitter analyst (Rust).
//!
//! Pure-Rust primary inference (tract CPU, GPU-extensible) with heterogeneous
//! accelerator cross-validation: Coral Edge TPU and Movidius NCS2 vote on the
//! primary candidate in the same flow, and consensus folds the votes into the
//! final certainty.
//!
//! HTTP contract matches the legacy Python worker:
//!   POST /jitter  -> JitterSignature
//!   GET  /health  -> backend/device status
//!
//! Env: JITTER_PORT (8180), JITTER_MODEL (optional ONNX path)

mod consensus;
mod heuristic;
mod inference;
mod types;
mod validators;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use tracing::info;
use tracing_subscriber::EnvFilter;

use inference::Engine;
use types::{JitterRequest, JitterSignature};
use validators::Validator;

#[derive(Clone)]
struct AppState {
    engine: Engine,
    validators: Arc<Vec<Box<dyn Validator>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let engine = Engine::init();
    let validators = validators::discover().await;
    for v in &validators {
        info!("validator online: {}", v.device());
    }
    if validators.is_empty() {
        info!("no accelerator validators on this node — primary-only mode");
    }

    let state = AppState {
        engine,
        validators: Arc::new(validators),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/jitter", post(jitter))
        .with_state(state);

    let port: u16 = std::env::var("JITTER_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8180);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("jitter-rs listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(st): State<AppState>) -> Json<serde_json::Value> {
    let devices: Vec<&str> = st.validators.iter().map(|v| v.device()).collect();
    Json(serde_json::json!({
        "service": "jitter-rs",
        "primary_backend": st.engine.backend(),
        "validators": devices,
        "status": "ok",
    }))
}

async fn jitter(
    State(st): State<AppState>,
    Json(req): Json<JitterRequest>,
) -> Json<JitterSignature> {
    let primary = st.engine.infer(&req);

    let mut votes = Vec::new();
    for v in st.validators.iter() {
        if let Some(vote) = v.vote(&req, &primary).await {
            votes.push(vote);
        }
    }

    let sig = consensus::combine(&req, primary, votes);
    Json(sig)
}
