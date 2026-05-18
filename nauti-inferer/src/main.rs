//! NautiInferer v4 — Public API server
//! Serves on :8099, proxied via api.cesarops.org
//! Routes inference requests to the best available node in the fleet.

use axum::{
    extract::State,
    response::{IntoResponse, Response},
    routing::{get, post},
    body::Body,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Clone)]
struct AppState {
    nodes: Arc<RwLock<Vec<NodeEndpoint>>>,
}

#[derive(Clone, Debug)]
struct NodeEndpoint {
    name: String,
    url: String,       // e.g. "http://127.0.0.1:5001"
    model: String,
    available: bool,
}

#[derive(Deserialize)]
struct InferenceRequest {
    #[serde(default = "default_model")]
    model: String,
    prompt: String,
    #[serde(default = "default_max_tokens")]
    max_tokens: u32,
    #[serde(default = "default_temperature")]
    temperature: f32,
    #[serde(default)]
    stream: bool,
}

fn default_model() -> String { "auto".to_string() }
fn default_max_tokens() -> u32 { 1024 }
fn default_temperature() -> f32 { 0.4 }

#[derive(Serialize)]
struct ModelInfo {
    id: String,
    name: String,
    node: String,
    ready: bool,
}

/// POST /v1/inference — main inference endpoint
async fn inference(
    State(state): State<AppState>,
    Json(req): Json<InferenceRequest>,
) -> Response {
    let nodes = state.nodes.read().await;

    // Pick best node: if model specified, match it; otherwise pick first available
    let target = if req.model == "auto" {
        nodes.iter().find(|n| n.available)
    } else {
        nodes.iter().find(|n| n.model.to_lowercase().contains(&req.model.to_lowercase()) && n.available)
            .or_else(|| nodes.iter().find(|n| n.available))
    };

    let target = match target {
        Some(t) => t.clone(),
        None => {
            return Json(serde_json::json!({"error": "no available nodes"})).into_response();
        }
    };

    info!(node = %target.name, model = %target.model, "routing inference");

    // Build koboldcpp request
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    if req.stream {
        // Streaming: proxy the SSE stream from koboldcpp
        let url = format!("{}/api/extra/generate/stream", target.url);
        let payload = serde_json::json!({
            "prompt": req.prompt,
            "max_length": req.max_tokens,
            "temperature": req.temperature,
            "top_p": 0.95,
            "rep_pen": 1.1,
        });

        match client.post(&url).json(&payload).send().await {
            Ok(resp) => {
                let body = Body::from_stream(resp.bytes_stream());
                Response::builder()
                    .header("Content-Type", "text/event-stream")
                    .header("Cache-Control", "no-cache")
                    .header("X-NautiInferer-Node", &target.name)
                    .header("X-NautiInferer-Model", &target.model)
                    .body(body)
                    .unwrap()
            }
            Err(e) => {
                Json(serde_json::json!({"error": format!("upstream: {}", e)})).into_response()
            }
        }
    } else {
        // Non-streaming: proxy and return full response
        let url = format!("{}/api/v1/generate", target.url);
        let payload = serde_json::json!({
            "prompt": req.prompt,
            "max_length": req.max_tokens,
            "temperature": req.temperature,
            "top_p": 0.95,
            "rep_pen": 1.1,
        });

        match client.post(&url).json(&payload).send().await {
            Ok(resp) => {
                let body: serde_json::Value = resp.json().await.unwrap_or_default();
                let text = body["results"][0]["text"].as_str().unwrap_or("");
                Json(serde_json::json!({
                    "id": format!("nauti-{}", std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()),
                    "model": target.model,
                    "node": target.name,
                    "choices": [{
                        "text": text,
                        "finish_reason": "stop"
                    }]
                })).into_response()
            }
            Err(e) => {
                Json(serde_json::json!({"error": format!("upstream: {}", e)})).into_response()
            }
        }
    }
}

/// GET /v1/models — list available models
async fn list_models(State(state): State<AppState>) -> Json<serde_json::Value> {
    let nodes = state.nodes.read().await;
    let models: Vec<ModelInfo> = nodes.iter().map(|n| ModelInfo {
        id: n.model.clone(),
        name: n.model.clone(),
        node: n.name.clone(),
        ready: n.available,
    }).collect();
    Json(serde_json::json!({"models": models}))
}

/// GET /v1/nodes — fleet status
async fn list_nodes(State(state): State<AppState>) -> Json<serde_json::Value> {
    let nodes = state.nodes.read().await;
    Json(serde_json::json!({"nodes": nodes.iter().map(|n| {
        serde_json::json!({"name": n.name, "model": n.model, "available": n.available, "url": n.url})
    }).collect::<Vec<_>>()}))
}

/// GET /health
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "nauti-inferer", "version": "4.0.0"}))
}

/// Background task: poll forge for node status every 30s
async fn node_poller(state: AppState) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    loop {
        // Poll the forge's cluster/nodes endpoint for live fleet data
        if let Ok(resp) = client.get("http://127.0.0.1:9100/cluster/nodes").send().await {
            if let Ok(nodes_json) = resp.json::<Vec<serde_json::Value>>().await {
                let mut endpoints = Vec::new();
                for node in &nodes_json {
                    let node_id = node["node_id"].as_str().unwrap_or("?");
                    let online = node["online"].as_bool().unwrap_or(false);
                    let hb = &node["last_heartbeat"];
                    let model = hb["model"].as_str().unwrap_or("idle");
                    let port = hb["port"].as_u64();

                    // Also check known koboldcpp endpoints directly
                    // These are the always-on fleet endpoints
                    if node_id == "t440-local" || node_id.contains("t440") {
                        // P100s — check if they're serving
                        for (name, url) in [("P100-Gemma", "http://127.0.0.1:5001"), ("P100-Qwen", "http://127.0.0.1:5002")] {
                            let model_name = probe_model(&client, url).await;
                            endpoints.push(NodeEndpoint {
                                name: name.to_string(),
                                url: url.to_string(),
                                model: model_name.clone(),
                                available: !model_name.is_empty(),
                            });
                        }
                    }
                    if online && port.is_some() {
                        // Remote node with active model
                        let p = port.unwrap();
                        let ip = match node_id {
                            "cesarops2" => "10.0.0.129",
                            "cesarops3" => "10.0.0.41",
                            _ => continue,
                        };
                        let url = format!("http://{}:{}", ip, p);
                        let model_name = probe_model(&client, &url).await;
                        if !model_name.is_empty() {
                            endpoints.push(NodeEndpoint {
                                name: node_id.to_string(),
                                url,
                                model: model_name,
                                available: true,
                            });
                        }
                    }
                }

                // Also always probe the 2060 Super on cesarops3
                let url_2060 = "http://10.0.0.41:5100";
                let model_2060 = probe_model(&client, url_2060).await;
                if !model_2060.is_empty() {
                    // Only add if not already present
                    if !endpoints.iter().any(|e| e.url == url_2060) {
                        endpoints.push(NodeEndpoint {
                            name: "cesarops3-2060S".to_string(),
                            url: url_2060.to_string(),
                            model: model_2060,
                            available: true,
                        });
                    }
                }

                *state.nodes.write().await = endpoints;
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

async fn probe_model(client: &reqwest::Client, url: &str) -> String {
    let probe_url = format!("{}/api/v1/model", url);
    match client.get(&probe_url).send().await {
        Ok(r) if r.status().is_success() => {
            if let Ok(body) = r.json::<serde_json::Value>().await {
                body["result"].as_str().unwrap_or("").replace("koboldcpp/", "").to_string()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let state = AppState {
        nodes: Arc::new(RwLock::new(Vec::new())),
    };

    // Start background node poller
    let poller_state = state.clone();
    tokio::spawn(async move {
        node_poller(poller_state).await;
    });

    let app = Router::new()
        .route("/v1/inference", post(inference))
        .route("/v1/models", get(list_models))
        .route("/v1/nodes", get(list_nodes))
        .route("/health", get(health))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state);

    let addr = "0.0.0.0:8099";
    info!("NautiInferer v4 API on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
