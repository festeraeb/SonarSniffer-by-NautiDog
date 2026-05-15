use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use axum::{routing::get, Router, Json};
use tracing::info;
use cesarops_lib::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("cesarops=info").init();
    info!("CESARops SAR Platform starting...");

    let state = AppState {
        cases: Arc::new(Mutex::new(Vec::new())),
        segments: Arc::new(Mutex::new(Vec::new())),
        members: Arc::new(Mutex::new(Vec::new())),
        positions: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/health", get(|| async { Json(serde_json::json!({"status":"ok","service":"cesarops"})) }))
        .merge(cesarops_lib::dispatch::routes())
        .merge(cesarops_lib::mapping::routes())
        .merge(cesarops_lib::tracking::routes())
        .merge(cesarops_lib::admin::routes())
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 9200));
    info!("CESARops listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
