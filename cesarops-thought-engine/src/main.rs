use std::sync::Arc;

use axum::{Router};
use tower_http::cors::CorsLayer;

mod clients;
mod engine;
mod handlers;
mod models;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into())
        )
        .init();

    // Initialize clients
    let kobold_client = clients::KoboldClient::new();
    let nautivecs_client = clients::NautivecsClient::new();
    let cesarops_client = clients::CesaropsClient::new();

    // Initialize engine
    let thought_engine = Arc::new(engine::ThoughtEngine::new(
        kobold_client,
        nautivecs_client,
        cesarops_client,
    ));

    // Build router
    let app = Router::new()
        .route("/tasks", axum::routing::post(handlers::create_task))
        .route("/tasks/{task_id}", axum::routing::get(handlers::get_task))
        .route("/health", axum::routing::get(handlers::health))
        .with_state(thought_engine)
        .layer(CorsLayer::permissive());

    // Start server
    let addr = "0.0.0.0:5556";
    tracing::info!("Starting Thought Engine on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
