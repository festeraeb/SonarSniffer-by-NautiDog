#!/bin/bash
# Deploy cesarops-wso as an HTTP server on T440
# Creates a thin axum wrapper that exposes WSO as POST /search and GET /health

export PATH=/home/cesarops/.cargo/bin:/usr/bin:/usr/local/bin:/bin:$PATH
cd /home/cesarops/wreckhunter2000-1

# Create the server binary crate
mkdir -p cesarops-wso-server/src

cat > cesarops-wso-server/Cargo.toml << 'TOML'
[package]
name = "cesarops-wso-server"
version = "0.1.0"
edition = "2021"

[dependencies]
cesarops-wso = { path = "../cesarops-wso" }
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
TOML

cat > cesarops-wso-server/src/main.rs << 'RUST'
use axum::{routing::{get, post}, extract::Json, Router};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    #[serde(default = "default_max")]
    max_results: usize,
}

fn default_max() -> usize { 5 }

#[derive(Serialize)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

#[derive(Serialize)]
struct SearchResponse {
    results: Vec<SearchResult>,
    query: String,
}

async fn health() -> &'static str {
    "{\"status\":\"ok\",\"service\":\"cesarops-wso\"}"
}

async fn search(Json(req): Json<SearchRequest>) -> Json<SearchResponse> {
    // Use the WSO library to perform web search
    let results = match cesarops_wso::search_web(&req.query, req.max_results).await {
        Ok(r) => r.into_iter().map(|item| SearchResult {
            title: item.title,
            url: item.url,
            snippet: item.snippet,
        }).collect(),
        Err(e) => {
            tracing::warn!("WSO search failed: {}", e);
            vec![]
        }
    };

    Json(SearchResponse {
        results,
        query: req.query,
    })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let app = Router::new()
        .route("/health", get(health))
        .route("/search", post(search));

    let addr = SocketAddr::from(([0, 0, 0, 0], 5010));
    tracing::info!("cesarops-wso server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
RUST

# Build it
cd cesarops-wso-server
cargo build --release 2>&1 | tail -5
echo "Build exit: $?"
