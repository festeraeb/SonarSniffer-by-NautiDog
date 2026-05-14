use axum::{
    extract::Path,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

use crate::capabilities::WorkerManifest;
use crate::model_registry::ModelRegistry;
use crate::tools;

/// Request body for POST /generate endpoint.
#[derive(Debug, Deserialize)]
pub struct GenerateRequest {
    pub prompt: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

/// Response from POST /generate (forwarded from koboldcpp).
#[derive(Debug, Serialize)]
pub struct GenerateResponse {
    pub text: String,
    pub stop: bool,
    pub generated_tokens: u32,
}

/// Request body for POST /tool/{name} endpoint.
#[derive(Debug, Deserialize)]
pub struct ToolRequest {
    #[serde(flatten)]
    pub args: serde_json::Value,
}

/// Response from tool execution.
#[derive(Debug, Serialize)]
pub struct ToolResponse {
    pub name: String,
    pub result: String,
}

pub fn create_app(
    manifest: WorkerManifest,
    registry: ModelRegistry,
    project_root: PathBuf,
    koboldcpp_url: String,
) -> Router {
    let manifest_static = manifest.clone();
    
    Router::new()
        .route("/health", get(|| async { Json(manifest_static.clone()) }))
        .route("/capabilities", get(move || async { Json(manifest.clone()) }))
        .route("/generate", post(move |req| {
            generate_handler(req, koboldcpp_url.clone(), manifest.clone())
        }))
        .route("/tool/{name}", post(move |Path(name), req| {
            tool_handler(name, req, project_root.clone())
        }))
        // MCP-compatible JSON-RPC endpoint
        .route("/rpc", post(move |body: axum::Json<serde_json::Value>| {
            rpc_handler(body, project_root.clone(), manifest.clone())
        }))
}

async fn generate_handler(
    req: axum::Json<GenerateRequest>,
    koboldcpp_url: String,
    _manifest: WorkerManifest,
) -> (StatusCode, Json<GenerateResponse>) {
    info!("Forwarding generation request to koboldcpp at {}", koboldcpp_url);
    
    let url = format!("{}/v1/generate", koboldcpp_url);
    
    let max_tokens = req.max_tokens.unwrap_or(512);
    let temperature = req.temperature.unwrap_or(0.7);
    
    let body = serde_json::json!({
        "prompt": req.prompt,
        "max_length": max_tokens,
        "temperature": temperature,
    });
    
    match reqwest::Client::new().post(&url).json(&body).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.json::<serde_json::Value>().await {
                    Ok(json) => {
                        let text = json.get("text")
                            .or_else(|| json.get("generated_text"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        (
                            StatusCode::OK,
                            Json(GenerateResponse {
                                text: text.to_string(),
                                stop: false,
                                generated_tokens: 0,
                            }),
                        )
                    }
                    Err(e) => {
                        warn!("Failed to parse koboldcpp response: {}", e);
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(GenerateResponse {
                                text: format!("Error parsing response: {}", e),
                                stop: true,
                                generated_tokens: 0,
                            }),
                        )
                    }
                }
            } else {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(GenerateResponse {
                        text: format!("Koboldcpp returned status {}: {}", resp.status(), resp.text().await.unwrap_or_default()),
                        stop: true,
                        generated_tokens: 0,
                    }),
                )
            }
        }
        Err(e) => {
            warn!("Failed to reach koboldcpp at {}: {}", koboldcpp_url, e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(GenerateResponse {
                    text: format!("Cannot reach koboldcpp: {}", e),
                    stop: true,
                    generated_tokens: 0,
                }),
            )
        }
    }
}

async fn tool_handler(
    name: String,
    req: axum::Json<ToolRequest>,
    project_root: PathBuf,
) -> (StatusCode, Json<ToolResponse>) {
    info!("Executing tool: {}", name);
    
    let result = tools::execute(&name, &req.args, &project_root).await;
    
    (StatusCode::OK, Json(ToolResponse { name, result }))
}

/// Handle MCP-compatible JSON-RPC calls.
pub async fn rpc_handler(
    body: axum::Json<serde_json::Value>,
    project_root: PathBuf,
    manifest: WorkerManifest,
) -> (StatusCode, Json<serde_json::Value>) {
    // Extract method and params from JSON-RPC envelope
    let method = match body.get("method").and_then(|m| m.as_str()) {
        Some(m) => m,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing 'method' field in JSON-RPC request" })),
            );
        }
    };
    
    let params = body.get("params").unwrap_or(&serde_json::Value::Null);
    
    let result = match method {
        "tools/list" => {
            serde_json::json!({
                "tools": manifest.tools.iter().map(|t| serde_json::json!({
                    "name": t,
                    "description": format!("Tool: {}", t),
                    "inputSchema": { "type": "object" }
                })).collect::<Vec<_>>()
            })
        }
        "tools/call" => {
            let tool_name = params.get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            let tool_args = params.get("arguments")
                .unwrap_or(params);
            let result = tools::execute(tool_name, tool_args, &project_root).await;
            serde_json::json!({ "result": result })
        }
        "resources/list" => {
            serde_json::json!({
                "resources": vec![
                    serde_json::json!({
                        "uri": "manifest://worker",
                        "name": "Worker Manifest",
                        "description": "Current worker capabilities and status"
                    }),
                    serde_json::json!({
                        "uri": "registry://models",
                        "name": "Model Registry",
                        "description": "Available GGUF models on this worker"
                    })
                ]
            })
        }
        _ => {
            return (
                StatusCode::METHOD_NOT_ALLOWED,
                Json(serde_json::json!({
                    "error": format!("Unknown method: {}. Supported: tools/list, tools/call, resources/list", method)
                })),
            );
        }
    };
    
    (StatusCode::OK, Json(serde_json::json!({ "result": result })))
}
