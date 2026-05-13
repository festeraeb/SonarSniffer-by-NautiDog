#![allow(dead_code)]

mod translator;
mod diagnostics;
mod tools;
mod memory;
mod hardware;
mod prompts;
mod loop_engine;

use axum::{extract::{Json, State}, response::Html, routing::{get, post}, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    pub conversation: Arc<Mutex<Vec<translator::Message>>>,
    pub config: Arc<ForgeConfig>,
}

#[derive(Clone)]
pub struct ForgeConfig {
    pub coder_url: String,      // 35B on P100s
    pub thinker_url: String,    // R1 on Xeon DDR4
    pub corrector_url: String,  // 14B Coder on 1070 (Marvin)
    pub nautivecs_url: String,
    pub wso_url: String,
    pub project_root: String,
}

#[derive(Deserialize)]
struct SendRequest { message: String }

#[derive(Serialize)]
struct SendResponse {
    response: String,
    tool_actions: Vec<String>,
    diagnosis: Option<String>,
}

async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

async fn health() -> &'static str {
    r#"{"status":"ok","service":"cesarops-forge-v2","mode":"self-healing-translator"}"#
}

async fn send_message(
    State(state): State<AppState>,
    Json(req): Json<SendRequest>,
) -> Json<SendResponse> {
    info!("User: {}", &req.message[..req.message.len().min(100)]);
    
    let result = loop_engine::run(&state, &req.message).await;
    
    info!("Response: {}... (tools: {}, diagnosed: {})",
        &result.response[..result.response.len().min(80)],
        result.tool_actions.len(),
        result.diagnosis.is_some()
    );
    
    Json(result)
}

async fn clear(State(state): State<AppState>) -> &'static str {
    let mut conv = state.conversation.lock().await;
    conv.clear();
    tools::reset_think_counter();
    "Conversation cleared"
}

async fn monitor() -> Json<serde_json::Value> {
    Json(hardware::cluster_summary().await)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("cesarops_forge_v2=info")
        .init();

    let config = ForgeConfig {
        coder_url: "http://127.0.0.1:5001".to_string(),
        thinker_url: "http://127.0.0.1:5557".to_string(),       // R1 on Xeon DDR4 Socket 1
        corrector_url: "http://100.102.158.111:5555".to_string(), // 14B Marvin on 1070
        nautivecs_url: "http://127.0.0.1:5003/query".to_string(),
        wso_url: "http://127.0.0.1:5010/search".to_string(),
        project_root: "/codebase/wreckhunter2000-1".to_string(),
    };

    let state = AppState {
        conversation: Arc::new(Mutex::new(Vec::new())),
        config: Arc::new(config),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/send", post(send_message))
        .route("/clear", post(clear))
        .route("/monitor", get(monitor))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 9100));
    info!("cesarops-forge-v2 (Self-Healing Knowledge Translator) on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
