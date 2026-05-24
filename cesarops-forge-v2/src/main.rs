#![allow(dead_code)]

mod translator;
mod diagnostics;
mod tools;
mod memory;
mod hardware;
mod prompts;
mod loop_engine;
mod agent_dispatch;
mod validator;
mod corrector_preset;
mod model_scorecard;
mod fleet_registry;
mod orchestrator;
mod routing;
mod inference_client;
mod paths;
mod mcp_delegate;

use axum::{extract::{Json, State}, response::Html, routing::{get, post}, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    pub conversation: Arc<Mutex<Vec<translator::Message>>>,
    pub config: Arc<RwLock<ForgeConfig>>,
    pub interrupt: Arc<std::sync::atomic::AtomicBool>,
    pub steering: Arc<Mutex<Vec<String>>>,
    /// DII node registry — populated by cesarops-node heartbeats.
    pub node_registry: Arc<Mutex<std::collections::HashMap<String, NodeRegistration>>>,
    /// Mission history — populated by webhook intake + orchestrator execute.
    pub missions: Arc<Mutex<Vec<MissionRecord>>>,
}

/// A mission submitted via webhook or direct execute.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct MissionRecord {
    pub id: String,
    pub source: String,
    pub scenario_text: String,
    pub status: String,
    pub submitted_at: u64,
    pub completed_at: Option<u64>,
    pub report: Option<orchestrator::MissionReport>,
}

/// A registered node in the DII cluster.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct NodeRegistration {
    pub node_id: String,
    pub hardware: serde_json::Value,
    pub available_models: Vec<String>,
    pub listen_port: u16,
    /// Last heartbeat state snapshot.
    pub last_heartbeat: Option<NodeHeartbeat>,
    /// Unix timestamp of last heartbeat.
    pub last_seen: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct NodeHeartbeat {
    pub state: String,
    pub model: Option<String>,
    pub port: Option<u16>,
    pub gpu: serde_json::Value,
    pub queue_depth: u32,
    #[serde(default)]
    pub all_gpus: serde_json::Value,
}

#[derive(Clone)]
pub struct ForgeConfig {
    pub coder_url: String,
    pub thinker_url: String,
    pub corrector_url: String,
    pub nautivecs_url: String,
    pub wso_url: String,
    pub project_root: String,
    pub validator_url: String,
    /// Active [[agent]] name (e.g. qwen-moe, moe)
    pub chat_agent: String,
    /// Prompt template: qwen2.5 | gemma | deepseek-r1 | llama3
    pub chat_template: String,
    pub chat_model: String,
}

impl AppState {
    /// Reload endpoints from routing_state.json + cluster_config.toml before each chat turn.
    pub async fn refresh_routing(&self) {
        let resolved = routing::resolve_endpoints();
        let mut cfg = self.config.write().await;
        cfg.coder_url = resolved.coder_url;
        cfg.thinker_url = resolved.thinker_url;
        cfg.corrector_url = resolved.corrector_url;
        cfg.validator_url = resolved.validator_url;
        cfg.chat_agent = resolved.chat_agent;
        cfg.chat_template = resolved.chat_template;
        cfg.chat_model = resolved.chat_model;
    }
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

async fn cluster_panel() -> Html<&'static str> {
    Html(include_str!("cluster_panel.html"))
}

async fn health() -> &'static str {
    r#"{"status":"ok","service":"cesarops-forge-v2","mode":"self-healing-translator"}"#
}

async fn send_message(
    State(state): State<AppState>,
    Json(req): Json<SendRequest>,
) -> Json<SendResponse> {
    info!("User: {}", &req.message[..req.message.len().min(100)]);
    state.refresh_routing().await;

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
    state.interrupt.store(false, std::sync::atomic::Ordering::Relaxed);
    "Conversation cleared"
}

async fn interrupt(State(state): State<AppState>) -> &'static str {
    state.interrupt.store(true, std::sync::atomic::Ordering::Relaxed);
    info!("INTERRUPT signal received — will stop after current tool call");
    "Interrupt signal sent. Generation will stop after current round."
}

async fn steer(State(state): State<AppState>, Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    let msg = body.get("message").and_then(|v| v.as_str()).unwrap_or("");
    if msg.is_empty() {
        return Json(serde_json::json!({"error": "message required"}));
    }
    let mut steering = state.steering.lock().await;
    steering.push(msg.to_string());
    info!("STEERING injected: {}", &msg[..msg.len().min(80)]);
    Json(serde_json::json!({"message": format!("Steering queued: {}", msg)}))
}

async fn monitor() -> Json<serde_json::Value> {
    Json(hardware::cluster_summary().await)
}

// --- Preset API ---

async fn get_preset_config() -> Json<serde_json::Value> {
    let preset_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/preset.json";
    match std::fs::read_to_string(preset_path) {
        Ok(content) => {
            let val: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
            Json(val)
        }
        Err(_) => {
            // Default preset
            Json(serde_json::json!({
                "p100_mode": "unified",
                "gpu0_role": "coder",
                "gpu0_model": "",
                "gpu1_role": "kv-cache",
                "gpu1_model": "",
                "node_1070": "corrector",
                "node_1060": "thinker",
                "node_laptop": "idle",
                "active": true
            }))
        }
    }
}

async fn save_preset_config(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    let preset_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/preset.json";
    match std::fs::write(preset_path, serde_json::to_string_pretty(&body).unwrap_or_default()) {
        Ok(_) => Json(serde_json::json!({"message": "CESAROPS configuration saved."})),
        Err(e) => Json(serde_json::json!({"error": format!("Failed to save preset: {}", e)})),
    }
}

async fn activate_preset() -> Json<serde_json::Value> {
    let preset_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/preset.json";
    let mut preset: serde_json::Value = match std::fs::read_to_string(preset_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => serde_json::json!({"active": true}),
    };
    preset["active"] = serde_json::json!(true);
    let _ = std::fs::write(preset_path, serde_json::to_string_pretty(&preset).unwrap_or_default());
    info!("CESAROPS ACTIVATED - locking resources for SAR scanning");
    Json(serde_json::json!({"message": "CESAROPS activated. Resources locked for scanning."}))
}

async fn launch_preset() -> Json<serde_json::Value> {
    let preset_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/preset.json";
    let preset: serde_json::Value = match std::fs::read_to_string(preset_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => return Json(serde_json::json!({"error": "No preset config saved. Save first."})),
    };

    // Stop all existing workers
    let _ = std::process::Command::new("bash")
        .arg("-c")
        .arg("pkill -f 'cesarops-inference.*--backend' ; pkill -f 'llama-server.*--port' ; pkill -f 'koboldcpp.*--model'")
        .output();

    info!("All workers stopped. Launching preset config...");

    let gpu0_model = preset.get("gpu0_model").and_then(|v| v.as_str()).unwrap_or("");
    let gpu0_role = preset.get("gpu0_role").and_then(|v| v.as_str()).unwrap_or("coder");
    let p100_mode = preset.get("p100_mode").and_then(|v| v.as_str()).unwrap_or("unified");

    let mut launched = Vec::new();

    // Launch GPU0 worker
    if !gpu0_model.is_empty() {
        let port = 5001;
        let cmd = format!(
            "nohup {} -m {} --host 0.0.0.0 --port {} -dev CUDA0,CUDA1 -sm layer -ts 50,50 -ngl 99 -c 8192 -t 8 \
             --spec-type draft-mtp --spec-draft-n-max 2 > /tmp/preset_gpu0.log 2>&1 &",
            inference_client::LLAMA_SERVER_BIN, gpu0_model, port
        );
        let _ = std::process::Command::new("bash").arg("-c").arg(&cmd).output();
        launched.push(format!("GPU0 ({}): {} on port {}", gpu0_role, gpu0_model.split('/').last().unwrap_or_default(), port));
    }

    // Mark preset as active
    let mut active_preset = preset.clone();
    active_preset["active"] = serde_json::json!(true);
    let _ = std::fs::write(preset_path, serde_json::to_string_pretty(&active_preset).unwrap_or_default());

    let msg = if launched.is_empty() {
        "Preset launched (no models configured - configure models first).".to_string()
    } else {
        format!("Preset launched: {}", launched.join(", "))
    };

    info!("{}", msg);
    Json(serde_json::json!({"message": msg}))
}

async fn deactivate_preset() -> Json<serde_json::Value> {
    let preset_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/preset.json";
    let mut preset: serde_json::Value = match std::fs::read_to_string(preset_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => serde_json::json!({"active": false}),
    };
    preset["active"] = serde_json::json!(false);
    let _ = std::fs::write(preset_path, serde_json::to_string_pretty(&preset).unwrap_or_default());
    info!("CESAROPS DEACTIVATED — freeform mode for coding/testing");
    Json(serde_json::json!({"message": "CESAROPS deactivated. Freeform mode."}))
}

/// POST /tool/{name}  — direct tool invocation, bypassing AI orchestration.
/// Used by the cluster panel for human-driven tool calls without going
/// through chat.
///
/// Body: { "arguments": { ...tool-specific args... } }
/// Returns: { "result": "<tool-output-string>", "tool": "<name>" }
async fn invoke_tool(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let args = body.get("arguments").cloned().unwrap_or(serde_json::json!({}));
    info!("Direct tool invocation: {} args={}", name,
        args.to_string().chars().take(120).collect::<String>());
    let result = tools::execute(&name, &args, &state).await;
    Json(serde_json::json!({"result": result, "tool": name}))
}

async fn apply_freeform(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    let freeform_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/freeform_state.json";
    match std::fs::write(freeform_path, serde_json::to_string_pretty(&body).unwrap_or_default()) {
        Ok(_) => {
            info!("Freeform config applied: coder={}, thinker={}, corrector={}, polisher={}",
                body.get("coder_model").and_then(|v| v.as_str()).unwrap_or("none"),
                body.get("thinker_model").and_then(|v| v.as_str()).unwrap_or("none"),
                body.get("corrector_model").and_then(|v| v.as_str()).unwrap_or("none"),
                body.get("polisher_model").and_then(|v| v.as_str()).unwrap_or("none"),
            );
            Json(serde_json::json!({"message": "Freeform configuration applied. Models loading."}))
        }
        Err(e) => Json(serde_json::json!({"error": format!("Failed to save freeform state: {}", e)})),
    }
}

// ── Mode swap: CESAROPS (SAR / wreck-hunting) vs CODING ──────────────────
// CESAROPS mode: workers run their wreck-detection roles (Qwen MoE for
//                analysis, Gemma for vision-validation, etc).
// CODING mode:   workers swap to coding configuration. Default coding cluster
//                is Qwen3.6-MoE on P100s as the main coder, Gemma-4-MoE
//                layered across 1070 + P1000 as reviewer/draft pair.
//                The pipeline is: P100 writes -> Gemma reviews -> P100 compiles.
//
// State persisted at mode_state.json so a forge restart keeps the active mode.

const MODE_STATE_PATH: &str = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/mode_state.json";

fn read_active_mode() -> String {
    std::fs::read_to_string(MODE_STATE_PATH)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("mode").and_then(|m| m.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| "cesarops".to_string())
}

fn write_active_mode(mode: &str, extras: serde_json::Value) {
    let payload = serde_json::json!({
        "mode": mode,
        "activated_at": chrono_now_unix(),
        "extras": extras,
    });
    let _ = std::fs::write(MODE_STATE_PATH, serde_json::to_string_pretty(&payload).unwrap_or_default());
}

fn chrono_now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// GET /mode  — returns current active mode (cesarops or coding)
async fn get_mode() -> Json<serde_json::Value> {
    let mode = read_active_mode();
    let extras: serde_json::Value = std::fs::read_to_string(MODE_STATE_PATH)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    Json(serde_json::json!({
        "mode": mode,
        "state": extras,
    }))
}

/// POST /mode/cesarops  — activate CESAROPS (SAR / wreck-hunting) cluster.
///
/// In this mode the **P100s are kept clear** so they can take GeoTIFF tile
/// stacks or magnetic grids the moment a mission lands. The intake / health /
/// n8n brain runs on **Picasso (P1000 on cesarops2:5571)** — same TinyLlama
/// instance that's always up for validator + speculative decode duty.
///
/// What we actually do:
///   1. Kill anything on the P100 ports (5001/5002). They MUST be empty.
///   2. Verify Picasso :5571 is responding; if not, log a warning (we don't
///      auto-spawn it — that's a remote node).
///   3. Persist the active mode + role map so the loop_engine knows where to
///      send intake messages.
async fn mode_cesarops() -> Json<serde_json::Value> {
    info!("MODE SWAP -> CESAROPS (SAR / wreck-hunting). P100s will be cleared.");

    // 1. Hard-clear the P100 ports. setsid + pkill catches detached koboldcpp
    //    processes that fuser alone would miss.
    let stop_cmd = inference_client::STOP_P100_INFERENCE;
    let _ = std::process::Command::new("bash").arg("-c").arg(stop_cmd).output();

    // 2. Probe intake brain on cesarops2 (1070 draft). Best-effort — we don't auto-start
    //    remote nodes; T440 P100s remain fallback.
    let intake_url = "http://10.0.0.201:5571/v1/models";
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap();
    let intake_online = client.get(intake_url).send().await
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    if !intake_online {
        warn!("CESAROPS mode: intake at {} not responding — T440 P100 fallback available.", intake_url);
    }

    // 3. Persist mode. Intake prefers cesarops2; P100s absorb load if remote down.
    write_active_mode("cesarops", serde_json::json!({
        "primary_role": "wreck_detection",
        "intake_endpoint": "http://10.0.0.201:5571",
        "intake_model": "Phi-3-mini-4k-instruct-Q4_K_M",
        "intake_online": intake_online,
        "thinker_endpoint": "http://10.0.0.201:5200",
        "fallback_intake": "http://127.0.0.1:5002",
        "p100_status": "cleared_for_tile_compute",
    }));

    info!("CESAROPS mode active. P100s cleared. Intake brain ({}).",
          if intake_online { "cesarops2 online" } else { "OFFLINE — T440 fallback" });

    Json(serde_json::json!({
        "message": format!(
            "Mode -> CESAROPS. P100s cleared for tile/mag compute. Intake: {}.",
            if intake_online { "cesarops2 online" } else { "offline — T440 fallback" }
        ),
        "mode": "cesarops",
        "intake_online": intake_online,
        "p100_status": "clear",
    }))
}

/// POST /mode/coding  — activate CODING cluster.
/// Default layout: Qwen3.6-MoE on P100s (main coder), Gemma-4-MoE layered
/// across 1070 + P1000 (reviewer/draft pair).
///
/// Pipeline:
///   1. User issues a /code command in chat
///   2. Coder (Qwen MoE on P100s) writes the code
///   3. Reviewer (Gemma layered 1070+P1000) reviews + suggests fixes
///   4. Coder applies fixes + compiles
///   5. Result returned via the standard /send loop
///
/// Body (optional):
///   { "coder_model": "...", "reviewer_model": "...", "free_p100_for_other_work": false }
async fn mode_coding(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    info!("MODE SWAP -> CODING");

    // Whether to keep one P100 free for tile/mag work even in coding mode.
    // Defaults false — coding mode wants both heavy hitters by default.
    let free_one_p100 = body
        .get("free_p100_for_other_work")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Use our own start_worker route. The cluster_config.toml [[worker]]
    // entries already pin vulkan_device + gpulayers + maingpu correctly per
    // P100 (fix from this morning), so the simplest correct path is:
    // hit /cluster/worker/{name}/start for each card and let that route do
    // the launching with the right flags. No bespoke koboldcpp shell-out here.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .unwrap();

    let workers_to_start: Vec<&str> = if free_one_p100 {
        vec!["GemmaBig"]
    } else {
        vec!["GemmaBig", "QwenBig"]
    };

    let mut started = Vec::new();
    let mut failed = Vec::new();
    for w in &workers_to_start {
        // POST to ourselves on localhost. The forge is on 9100.
        let url = format!("http://127.0.0.1:9100/cluster/worker/{}/start", w);
        match client.post(&url).send().await {
            Ok(r) if r.status().is_success() => {
                started.push(w.to_string());
                info!("mode_coding: launched {}", w);
            }
            Ok(r) => {
                failed.push(format!("{} (HTTP {})", w, r.status()));
                warn!("mode_coding: {} returned HTTP {}", w, r.status());
            }
            Err(e) => {
                failed.push(format!("{} ({})", w, e));
                warn!("mode_coding: {} failed: {}", w, e);
            }
        }
    }

    write_active_mode("coding", serde_json::json!({
        "primary_role": "coding",
        "coder_endpoint": "http://127.0.0.1:5001",
        "coder_model": "/codebase/models/Gemma-4-26B-MoE-IQ4_XS.gguf",
        "reviewer_endpoint": "http://127.0.0.1:5002",
        "reviewer_model": "/codebase/models/Qwen3.6-35B-A3B-Q4_K_M.gguf",
        "draft_endpoint": "http://10.0.0.201:5571",
        "free_one_p100": free_one_p100,
        "pipeline": "coder (Gemma) -> reviewer (Qwen) -> draft (Picasso)",
        "started_workers": started.clone(),
        "failed_workers": failed.clone(),
    }));

    info!(
        "CODING mode active. Started: {:?}. Failed: {:?}.",
        started, failed
    );

    Json(serde_json::json!({
        "message": format!(
            "Mode -> CODING. Started {} P100 worker(s). Reviewer pair = Gemma (P100#0) + Qwen (P100#1).",
            started.len()
        ),
        "mode": "coding",
        "started": started,
        "failed": failed,
        "coder_endpoint": "http://127.0.0.1:5001",
        "reviewer_endpoint": "http://127.0.0.1:5002",
        "draft_endpoint": "http://10.0.0.201:5571",
    }))
}

/// POST /code  — coding-mode pipeline entry point.
/// Routes a coding request through coder -> reviewer -> coder_compile.
/// Only valid when /mode/coding is active. Falls back to /send in cesarops mode.
async fn route_code_request(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let active_mode = read_active_mode();
    if active_mode != "coding" {
        return Json(serde_json::json!({
            "error": format!("/code requires CODING mode. Active mode: {}. POST /mode/coding first.", active_mode),
        }));
    }

    let task = body.get("task").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if task.is_empty() {
        return Json(serde_json::json!({"error": "Field 'task' required."}));
    }

    info!("CODING pipeline: task='{}'", &task[..task.len().min(120)]);

    // Stage 1: coder writes
    let coder_resp = call_coder_for_code(&state, &task).await;
    let initial_code = coder_resp.unwrap_or_else(|e| format!("[CODER ERROR]: {}", e));

    // Stage 2: reviewer reviews
    let review = call_reviewer_for_review(&state, &task, &initial_code).await
        .unwrap_or_else(|e| format!("[REVIEWER ERROR]: {}", e));

    // Stage 3: coder applies review + compiles (just returns the integrated answer)
    let final_resp = call_coder_for_integration(&state, &task, &initial_code, &review).await
        .unwrap_or_else(|e| format!("[INTEGRATION ERROR]: {}", e));

    Json(serde_json::json!({
        "task": task,
        "initial_code": initial_code,
        "review": review,
        "final": final_resp,
        "pipeline": "coder -> reviewer -> coder_integrate",
    }))
}

async fn call_coder_for_code(state: &AppState, task: &str) -> Result<String, String> {
    let prompt = format!(
        "<|im_start|>system\nYou are a Rust+wgpu specialist. Write production-ready code. \
        No placeholders, no `todo!()`. Output only the code.\n<|im_end|>\n\
        <|im_start|>user\n{}\n<|im_end|>\n<|im_start|>assistant\n",
        task
    );
    call_endpoint("http://127.0.0.1:5001", &prompt, 8192, 0.3).await
}

async fn call_reviewer_for_review(_state: &AppState, task: &str, code: &str) -> Result<String, String> {
    let prompt = format!(
        "<|im_start|>system\nYou are a code reviewer. Find bugs, suggest improvements. \
        Be terse. Bullet points only.\n<|im_end|>\n\
        <|im_start|>user\nTask: {}\n\nCode:\n{}\n<|im_end|>\n<|im_start|>assistant\n",
        task,
        if code.len() > 6000 { &code[..6000] } else { code }
    );
    let endpoint = routing::resolve_reviewer_endpoint().await;
    call_endpoint(&endpoint, &prompt, 2048, 0.2).await
}

async fn call_coder_for_integration(state: &AppState, task: &str, code: &str, review: &str) -> Result<String, String> {
    let prompt = format!(
        "<|im_start|>system\nApply the review feedback to the code. Output the final corrected code only.\n<|im_end|>\n\
        <|im_start|>user\nTask: {}\n\nCode:\n{}\n\nReview:\n{}\n<|im_end|>\n<|im_start|>assistant\n",
        task,
        if code.len() > 6000 { &code[..6000] } else { code },
        if review.len() > 2000 { &review[..2000] } else { review }
    );
    call_endpoint("http://127.0.0.1:5001", &prompt, 8192, 0.3).await
}

async fn call_endpoint(url: &str, prompt: &str, max_length: u32, temperature: f32) -> Result<String, String> {
    let client = reqwest::Client::new();
    inference_client::complete_prompt(
        &client,
        url,
        prompt,
        max_length,
        temperature,
        vec!["<|im_end|>".to_string(), "</s>".to_string()],
        None,
    )
    .await
}

async fn corrector_connect() -> Json<serde_json::Value> {
    // Update tuning config to re-enable corrector
    let config_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    if let Ok(content) = std::fs::read_to_string(config_path) {
        let updated = content.replace("skip_corrector = true", "skip_corrector = false");
        let _ = std::fs::write(config_path, updated);
    }
    info!("Corrector CONNECTED");
    Json(serde_json::json!({"message": "Corrector connected. Will be used on next request."}))
}

async fn corrector_disconnect() -> Json<serde_json::Value> {
    let config_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    if let Ok(content) = std::fs::read_to_string(config_path) {
        let updated = content.replace("skip_corrector = false", "skip_corrector = true");
        let _ = std::fs::write(config_path, updated);
    }
    info!("Corrector DISCONNECTED");
    Json(serde_json::json!({"message": "Corrector disconnected. Qwen flies solo."}))
}

/// Send a task to any GPU endpoint in agent mode (with tools).
/// POST /cluster/agent/run { "endpoint": "http://...:5001", "message": "do something" }
async fn run_agent_task(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    let endpoint_in = body.get("endpoint").and_then(|v| v.as_str()).unwrap_or("");
    let message = body.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let role = body
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();

    let use_mtp_pool = endpoint_in.is_empty()
        || endpoint_in == "mtp"
        || endpoint_in == "auto-mtp"
        || role == "mtp"
        || (role == "reviewer" && endpoint_in.is_empty());

    let endpoint = if message.is_empty() {
        String::new()
    } else if use_mtp_pool {
        let client = reqwest::Client::new();
        let pool = routing::load_mtp_pool();
        if let Some(u) = routing::first_online_llama(&client, &pool).await {
            u
        } else {
            routing::resolve_reviewer_endpoint().await
        }
    } else {
        endpoint_in.to_string()
    };

    if endpoint.is_empty() || message.is_empty() {
        return Json(serde_json::json!({
            "error": "message required; endpoint required unless role=reviewer or endpoint=mtp"
        }));
    }

    let template = body
        .get("template")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| routing::template_for_endpoint(&routing::load_cluster_routing(), &endpoint));

    let engine = body.get("engine").and_then(|v| v.as_str()).map(String::from);

    let mcp_worker_url = std::env::var("MCP_WORKER_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| Some("http://127.0.0.1:8090".to_string()));

    let config = agent_dispatch::AgentConfig {
        endpoint_url: endpoint.clone(),
        project_root: paths::project_root(),
        nautivecs_url: "http://127.0.0.1:5003/query".to_string(),
        wso_url: "http://127.0.0.1:5010/search".to_string(),
        mcp_worker_url,
        max_tokens: 12288,
        temperature: 0.4,
        safe_mode: body.get("safe_mode").and_then(|v| v.as_bool()).unwrap_or(false),
        chat_template: template,
        engine,
    };

    info!("Agent task dispatched to {}: {}...", endpoint, &message[..message.len().min(80)]);
    let result = agent_dispatch::run_agent_loop(&config, message).await;
    info!("Agent task complete: {}...", &result[..result.len().min(100)]);

    Json(serde_json::json!({"response": result, "endpoint_used": endpoint}))
}

// --- Cluster Control API ---

async fn get_cluster_config() -> Json<serde_json::Value> {
    let config_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    match std::fs::read_to_string(config_path) {
        Ok(content) => {
            match content.parse::<toml::Table>() {
                Ok(table) => {
                    // Convert TOML to JSON for the frontend
                    let mut result = serde_json::json!({});
                    
                    // Bootstrap section
                    if let Some(bootstrap) = table.get("bootstrap") {
                        result["bootstrap"] = toml_to_json(bootstrap);
                    }
                    
                    // GPUs
                    if let Some(gpus) = table.get("gpu").and_then(|v| v.as_array()) {
                        result["gpus"] = serde_json::Value::Array(
                            gpus.iter().map(|g| toml_to_json(g)).collect()
                        );
                    }
                    
                    // Workers
                    if let Some(workers) = table.get("worker").and_then(|v| v.as_array()) {
                        result["workers"] = serde_json::Value::Array(
                            workers.iter().map(|w| toml_to_json(w)).collect()
                        );
                    }
                    
                    Json(result)
                }
                Err(e) => Json(serde_json::json!({"error": format!("TOML parse error: {}", e)})),
            }
        }
        Err(e) => Json(serde_json::json!({"error": format!("Config not found: {}", e)})),
    }
}

async fn save_cluster_config(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    let config_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    
    // Rebuild TOML from the JSON config
    let mut toml_str = String::from("# Cluster Configuration — managed by web UI\n\n");
    
    if let Some(bootstrap) = body.get("bootstrap") {
        toml_str.push_str("[bootstrap]\n");
        if let Some(m) = bootstrap.get("model").and_then(|v| v.as_str()) {
            toml_str.push_str(&format!("model = \"{}\"\n", m));
        }
        if let Some(p) = bootstrap.get("port").and_then(|v| v.as_u64()) {
            toml_str.push_str(&format!("port = {}\n", p));
        }
        toml_str.push_str("backend = \"cpu\"\n");
        if let Some(t) = bootstrap.get("template").and_then(|v| v.as_str()) {
            toml_str.push_str(&format!("template = \"{}\"\n", t));
        }
        toml_str.push('\n');
    }
    
    if let Some(gpus) = body.get("gpus").and_then(|v| v.as_array()) {
        for gpu in gpus {
            toml_str.push_str("[[gpu]]\n");
            if let Some(id) = gpu.get("id").and_then(|v| v.as_u64()) {
                toml_str.push_str(&format!("id = {}\n", id));
            }
            if let Some(name) = gpu.get("name").and_then(|v| v.as_str()) {
                toml_str.push_str(&format!("name = \"{}\"\n", name));
            }
            if let Some(vram) = gpu.get("vram_mb").and_then(|v| v.as_u64()) {
                toml_str.push_str(&format!("vram_mb = {}\n", vram));
            }
            toml_str.push('\n');
        }
    }
    
    if let Some(workers) = body.get("workers").and_then(|v| v.as_array()) {
        for w in workers {
            toml_str.push_str("[[worker]]\n");
            if let Some(n) = w.get("name").and_then(|v| v.as_str()) {
                toml_str.push_str(&format!("name = \"{}\"\n", n));
            }
            if let Some(r) = w.get("role").and_then(|v| v.as_str()) {
                toml_str.push_str(&format!("role = \"{}\"\n", r));
            }
            if let Some(g) = w.get("gpu").and_then(|v| v.as_u64()) {
                toml_str.push_str(&format!("gpu = {}\n", g));
            }
            if let Some(m) = w.get("model").and_then(|v| v.as_str()) {
                toml_str.push_str(&format!("model = \"{}\"\n", m));
            }
            if let Some(p) = w.get("port").and_then(|v| v.as_u64()) {
                toml_str.push_str(&format!("port = {}\n", p));
            }
            if let Some(t) = w.get("template").and_then(|v| v.as_str()) {
                toml_str.push_str(&format!("template = \"{}\"\n", t));
            }
            let enabled = w.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
            toml_str.push_str(&format!("enabled = {}\n\n", enabled));
        }
    }
    
    match std::fs::write(config_path, &toml_str) {
        Ok(_) => Json(serde_json::json!({"message": "Config saved successfully."})),
        Err(e) => Json(serde_json::json!({"error": format!("Failed to write config: {}", e)})),
    }
}

async fn list_available_models() -> Json<serde_json::Value> {
    let models_dir = "/codebase/models";
    let mut models: Vec<String> = Vec::new();
    
    if let Ok(entries) = std::fs::read_dir(models_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "gguf" {
                    models.push(path.to_string_lossy().to_string());
                }
            }
        }
    }
    models.sort();
    Json(serde_json::json!(models))
}

/// Resolve a worker path param to its index in the worker array.
/// Accepts either a numeric index ("0", "1") or a worker name ("GemmaBig").
fn resolve_worker_idx(workers: &[toml::Value], path: &str) -> Option<usize> {
    if let Ok(n) = path.parse::<usize>() {
        if n < workers.len() { return Some(n); }
    }
    workers.iter().position(|w| {
        w.get("name").and_then(|v| v.as_str()) == Some(path)
    })
}

async fn start_worker(axum::extract::Path(path): axum::extract::Path<String>) -> Json<serde_json::Value> {
    // Read config, get worker details, spawn cesarops-inference process
    let config_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();

    let workers_arr = table.get("worker").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let idx = match resolve_worker_idx(&workers_arr, &path) {
        Some(i) => i,
        None => return Json(serde_json::json!({"error": format!("Worker not found: {}", path)})),
    };

    if let Some(workers) = table.get("worker").and_then(|v| v.as_array()) {
        if let Some(worker) = workers.get(idx) {
            let model = worker.get("model").and_then(|v| v.as_str()).unwrap_or("");
            let port = worker.get("port").and_then(|v| v.as_integer()).unwrap_or(5010);
            let gpu = worker.get("gpu").and_then(|v| v.as_integer()).unwrap_or(0);
            let name = worker.get("name").and_then(|v| v.as_str()).unwrap_or("worker");
            let host = worker.get("host").and_then(|v| v.as_str()).unwrap_or("local");

            // Optional koboldcpp launch flags. Pulled from cluster_config.toml
            // [[worker]] entries — these matter on Pascal because the auto-pick
            // path will spread layers across both P100s and OOM the second
            // worker we try to launch.
            let vulkan_device = worker.get("vulkan_device").and_then(|v| v.as_integer()).unwrap_or(gpu);
            let gpulayers = worker.get("gpulayers").and_then(|v| v.as_integer()).unwrap_or(99);
            let contextsize = worker.get("contextsize").and_then(|v| v.as_integer()).unwrap_or(8192);
            let threads = worker.get("threads").and_then(|v| v.as_integer()).unwrap_or(4);
            let usecublas = worker.get("usecublas").and_then(|v| v.as_integer()).unwrap_or(0);
            let ramlayers = worker.get("ramlayers").and_then(|v| v.as_integer()).unwrap_or(0);
            let tensor_split: Vec<String> = worker
                .get("tensor_split")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| {
                            x.as_integer()
                                .map(|i| i.to_string())
                                .or_else(|| x.as_str().map(String::from))
                        })
                        .collect()
                })
                .unwrap_or_default();

            if model.is_empty() {
                return Json(serde_json::json!({"error": "No model assigned to this worker."}));
            }

            // Check if this is a remote node
            if host != "local" {
                return Json(serde_json::json!({"error": format!("Remote workers ({}) must be started on their host machine.", host)}));
            }

            // Determine engine. Honor the explicit `engine` field in the
            // worker config first; fall back to the filename heuristic only
            // when the operator hasn't pinned it. Without this, koboldcpp-only
            // models (Qwen3.6-35B-A3B Q4_K_M needs ssm/expert ops the native
            // engine doesn't yet implement) get launched on cesarops-inference
            // and panic with "Buffer size > max buffer size".
            let engine_pin = worker.get("engine").and_then(|v| v.as_str()).unwrap_or("");
            let backend = worker.get("backend").and_then(|v| v.as_str()).unwrap_or("cuda");
            let native_quants = ["q4_0", "q4_k_m", "q6_k", "q8_0", "f16", "f32", "bf16"];
            let model_lower = model.to_lowercase();
            let use_native = match engine_pin {
                "cesarops-inference" | "native" | "wgpu" => true,
                "koboldcpp" | "llama-server" | "llama.cpp" => false,
                _ => native_quants.iter().any(|q| model_lower.contains(q)),
            };
            let effective = inference_client::effective_worker_engine(engine_pin, use_native);

            let cmd = if use_native {
                format!(
                    "nohup /codebase/repos/wreckhunter2000-1/cesarops-inference/target/release/cesarops-inference --model {} --port {} --backend wgpu --gpu {} > /tmp/worker_{}.log 2>&1 &",
                    model, port, gpu, idx
                )
            } else if effective == "koboldcpp" {
                let mut extra = String::new();
                if usecublas != 0 {
                    extra.push_str(&format!(" --usecublas {}", usecublas));
                }
                if ramlayers > 0 {
                    extra.push_str(&format!(" --ramlayers {}", ramlayers));
                }
                if !tensor_split.is_empty() {
                    extra.push_str(" --tensor_split");
                    for t in &tensor_split {
                        extra.push(' ');
                        extra.push_str(t);
                    }
                }
                format!(
                    "setsid /home/cesarops/koboldcpp --model {} --port {} \
                     --usevulkan {} --gpulayers {} --contextsize {} --threads {} \
                     --quiet --maingpu {}{} > /tmp/worker_{}.log 2>&1 < /dev/null & disown",
                    model, port, vulkan_device, gpulayers, contextsize, threads, vulkan_device, extra, idx
                )
            } else {
                inference_client::llama_server_spawn_cmd(
                    model,
                    port,
                    backend,
                    gpulayers,
                    contextsize,
                    threads,
                    &tensor_split,
                    idx,
                )
            };

            let engine = effective;

            match std::process::Command::new("bash").arg("-c").arg(&cmd).output() {
                Ok(_) => Json(serde_json::json!({
                    "message": format!(
                        "Started {} on GPU {} (vulkan_device {}, gpulayers {}) port {} via {}",
                        name, gpu, vulkan_device, gpulayers, port, engine
                    )
                })),
                Err(e) => Json(serde_json::json!({"error": format!("Failed to start: {}", e)})),
            }
        } else {
            Json(serde_json::json!({"error": "Worker index out of range"}))
        }
    } else {
        Json(serde_json::json!({"error": "No workers in config"}))
    }
}

async fn stop_worker(axum::extract::Path(path): axum::extract::Path<String>) -> Json<serde_json::Value> {
    let config_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();

    let workers_arr = table.get("worker").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let idx = match resolve_worker_idx(&workers_arr, &path) {
        Some(i) => i,
        None => return Json(serde_json::json!({"error": format!("Worker not found: {}", path)})),
    };

    if let Some(workers) = table.get("worker").and_then(|v| v.as_array()) {
        if let Some(worker) = workers.get(idx) {
            let port = worker.get("port").and_then(|v| v.as_integer()).unwrap_or(5010);
            let name = worker.get("name").and_then(|v| v.as_str()).unwrap_or("worker");
            
            // Kill process on that port
            let cmd = format!("fuser -k {}/tcp 2>/dev/null", port);
            let _ = std::process::Command::new("bash").arg("-c").arg(&cmd).output();
            
            Json(serde_json::json!({"message": format!("Stopped {} (port {})", name, port)}))
        } else {
            Json(serde_json::json!({"error": "Worker index out of range"}))
        }
    } else {
        Json(serde_json::json!({"error": "No workers in config"}))
    }
}

async fn start_all_workers() -> Json<serde_json::Value> {
    let repo = paths::project_root();
    let mut started: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let mcp_bin = format!("{}/cesarops-mcp-worker/target/release/cesarops-mcp-worker", repo);
    let scripts = [
        ("p100_cycle", format!("{}/scripts/p100_cycle.sh start", repo)),
        ("vision_workers", format!(
            "VISION_MODE=${{VISION_MODE:-cpu}} REPO={} bash {}/scripts/start_vision_workers.sh start",
            repo, repo
        )),
        ("mcp_worker", format!(
            "MCP_DELEGATE_TOOLS=1 nohup {} --port 8090 --project-root {} \
             > /tmp/mcp-worker.log 2>&1 &",
            mcp_bin, repo
        )),
    ];

    for (name, cmd) in &scripts {
        match std::process::Command::new("bash").arg("-c").arg(cmd).output() {
            Ok(o) if o.status.success() => started.push(name.to_string()),
            Ok(o) => errors.push(format!(
                "{}: {}",
                name,
                String::from_utf8_lossy(&o.stderr)[..500.min(o.stderr.len())].to_string()
            )),
            Err(e) => errors.push(format!("{}: {}", name, e)),
        }
    }

    Json(serde_json::json!({
        "started": started,
        "errors": errors,
        "hint": "cesarops2 lab: ssh cesarops@10.0.0.201 bash scripts/cesarops2_research_lab.sh start"
    }))
}

async fn stop_all_workers() -> Json<serde_json::Value> {
    let _ = std::process::Command::new("bash")
        .arg("-c")
        .arg("pkill -f 'cesarops-inference.*--backend wgpu' ; pkill -f 'llama-server.*--port' ; pkill -f 'koboldcpp.*--model'")
        .output();
    Json(serde_json::json!({"message": "All GPU workers stopped."}))
}

// ── DII Node Registry Handlers ──────────────────────────────────────────────

/// POST /cluster/node/register — cesarops-node daemon calls this on startup.
async fn node_register(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let node_id = payload["node_id"].as_str().unwrap_or("unknown").to_string();

    let registration = NodeRegistration {
        node_id: node_id.clone(),
        hardware: payload["hardware"].clone(),
        available_models: serde_json::from_value(payload["available_models"].clone())
            .unwrap_or_default(),
        listen_port: payload["listen_port"].as_u64().unwrap_or(5500) as u16,
        last_heartbeat: None,
        last_seen: now,
    };

    state.node_registry.lock().await.insert(node_id.clone(), registration);
    info!("node_register: {} registered", node_id);

    Json(serde_json::json!({ "status": "registered", "node_id": node_id }))
}

/// POST /cluster/node/heartbeat — cesarops-node sends this every 10s.
async fn node_heartbeat(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let node_id = payload["node_id"].as_str().unwrap_or("");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let mut registry = state.node_registry.lock().await;

    if let Some(node) = registry.get_mut(node_id) {
        node.last_heartbeat = Some(NodeHeartbeat {
            state: payload["state"].as_str().unwrap_or("unknown").to_string(),
            model: payload["model"].as_str().map(|s| s.to_string()),
            port: payload["port"].as_u64().map(|p| p as u16),
            gpu: payload["gpu"].clone(),
            queue_depth: payload["queue_depth"].as_u64().unwrap_or(0) as u32,
            all_gpus: payload["all_gpus"].clone(),
        });
        node.last_seen = now;
        // Also update hardware.all_gpus if present (re-registration on heartbeat)
        if payload["all_gpus"].is_array() && !payload["all_gpus"].as_array().unwrap().is_empty() {
            if let Some(hw) = node.hardware.as_object_mut() {
                hw.insert("all_gpus".to_string(), payload["all_gpus"].clone());
            }
        }
        Json(serde_json::json!({ "status": "ok" }))
    } else {
        Json(serde_json::json!({ "error": "unknown node, register first" }))
    }
}

/// GET /cluster/nodes — list all registered DII nodes with online status.
async fn list_registered_nodes(State(state): State<AppState>) -> Json<serde_json::Value> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let registry = state.node_registry.lock().await;

    let nodes: Vec<serde_json::Value> = registry.values().map(|n| {
        let online = now.saturating_sub(n.last_seen) < 30;
        let mut val = serde_json::to_value(n).unwrap_or_default();
        val["online"] = serde_json::json!(online);
        val["last_seen_secs_ago"] = serde_json::json!(now.saturating_sub(n.last_seen));
        val
    }).collect();

    Json(serde_json::Value::Array(nodes))
}

/// Discover which nodes are online — merges DII registry + legacy port probes.
async fn discover_nodes(State(state): State<AppState>) -> Json<serde_json::Value> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 1. Registry nodes — no HTTP probe needed, heartbeat IS the probe.
    {
        let registry = state.node_registry.lock().await;
        for (name, reg) in registry.iter() {
            let online = now.saturating_sub(reg.last_seen) < 30;
            let (node_state, model, port, gpu_val) = match &reg.last_heartbeat {
                Some(hb) => (hb.state.clone(), hb.model.clone(), hb.port, hb.gpu.clone()),
                None => ("unknown".to_string(), None, None, serde_json::Value::Null),
            };

            results.push(serde_json::json!({
                "name": name,
                "online": online,
                "source": "registry",
                "gpu": reg.hardware.get("gpu").and_then(|v| v.as_str()).unwrap_or("?"),
                "state": node_state,
                "model": model,
                "port": port,
                "listen_port": reg.listen_port,
                "gpu_info": gpu_val,
                "last_seen_secs_ago": now.saturating_sub(reg.last_seen),
                "services_online": online,
                "available_models": reg.available_models,
            }));
            seen_names.insert(name.clone());
        }
    }

    // 2. Legacy nodes from cluster_config.toml — probe ports via HTTP.
    let config_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    let tailscale_peers = get_tailscale_peers().await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();

    if let Some(nodes) = table.get("known_nodes").and_then(|v| v.as_array()) {
        for node in nodes {
            let name = node.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
            if seen_names.contains(name) {
                continue; // Already covered by registry
            }

            let ip = node.get("ip").and_then(|v| v.as_str()).unwrap_or("");

            // Skip localhost legacy entries if we have any registry node — the
            // local cesarops-node daemon already reports this box via heartbeat.
            if (ip == "127.0.0.1" || ip == "localhost") && !seen_names.is_empty() {
                continue;
            }

            let gpu_label = node.get("gpu").and_then(|v| v.as_str()).unwrap_or("");
            let empty_ports = vec![];
            let ports = node.get("ports").and_then(|v| v.as_array()).unwrap_or(&empty_ports);

            let ts_online = if ip == "127.0.0.1" || ip == "localhost" {
                true
            } else if ip.starts_with("10.") || ip.starts_with("192.168.") {
                true
            } else {
                tailscale_peers.iter().any(|p| p.0 == ip && p.1)
            };

            let mut port_status = Vec::new();
            for port_val in ports {
                let port = port_val.as_integer().unwrap_or(0);
                let online = if ts_online {
                    let probe_paths = ["/api/extra/version", "/v1/models", "/health"];
                    let mut found = false;
                    for path in probe_paths {
                        let url = format!("http://{}:{}{}", ip, port, path);
                        if let Ok(resp) = client.get(&url).send().await {
                            if resp.status().is_success() {
                                found = true;
                                break;
                            }
                        }
                    }
                    found
                } else {
                    false
                };
                port_status.push(serde_json::json!({"port": port, "online": online}));
            }

            let any_online = port_status.iter()
                .any(|p| p.get("online").and_then(|v| v.as_bool()).unwrap_or(false));

            results.push(serde_json::json!({
                "name": name,
                "ip": ip,
                "online": ts_online,
                "source": "legacy_probe",
                "gpu": gpu_label,
                "state": "unknown",
                "services_online": any_online,
                "ports": port_status,
            }));
            seen_names.insert(name.to_string());
        }
    }

    Json(serde_json::json!(results))
}

/// Parse `tailscale status` to get peer online/offline state
async fn get_tailscale_peers() -> Vec<(String, bool)> {
    let output = match tokio::process::Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .await
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut peers = Vec::new();

    // Self is always online
    if let Some(self_node) = val.get("Self") {
        if let Some(addrs) = self_node.get("TailscaleIPs").and_then(|v| v.as_array()) {
            for addr in addrs {
                if let Some(ip) = addr.as_str() {
                    peers.push((ip.to_string(), true));
                }
            }
        }
    }

    // Peers
    if let Some(peer_map) = val.get("Peer").and_then(|v| v.as_object()) {
        for (_key, peer) in peer_map {
            let online = peer.get("Online").and_then(|v| v.as_bool()).unwrap_or(false);
            if let Some(addrs) = peer.get("TailscaleIPs").and_then(|v| v.as_array()) {
                for addr in addrs {
                    if let Some(ip) = addr.as_str() {
                        peers.push((ip.to_string(), online));
                    }
                }
            }
        }
    }

    peers
}

/// Helper: convert a TOML value to serde_json::Value
fn toml_to_json(val: &toml::Value) -> serde_json::Value {
    match val {
        toml::Value::String(s) => serde_json::json!(s),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::json!(b),
        toml::Value::Array(arr) => serde_json::Value::Array(arr.iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            let mut map = serde_json::Map::new();
            for (k, v) in t {
                map.insert(k.clone(), toml_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        _ => serde_json::Value::Null,
    }
}

// ── Per-card cluster control handlers ───────────────────────────────────────

/// POST /cluster/worker/{name}/apply — apply all settings for one worker
async fn worker_apply(
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    info!("Worker apply: {} config={}", name, body.to_string().chars().take(80).collect::<String>());
    // Persist to cluster_config.toml
    update_worker_config(&name, &body);
    Json(serde_json::json!({"message": format!("Worker {} settings applied", name)}))
}

/// POST /cluster/worker/{name}/set_injection { "enabled": bool }
async fn worker_set_injection(
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let enabled = body.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    info!("Worker {} vector injection → {}", name, enabled);
    update_worker_field(&name, "inject_vectors", serde_json::json!(enabled));
    Json(serde_json::json!({"message": format!("{} injection={}", name, enabled)}))
}

/// POST /cluster/worker/{name}/set_backend { "backend": "cuda"|"vulkan"|"cpu" }
async fn worker_set_backend(
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let backend = body.get("backend").and_then(|v| v.as_str()).unwrap_or("vulkan");
    info!("Worker {} backend → {}", name, backend);
    update_worker_field(&name, "backend", serde_json::json!(backend));
    Json(serde_json::json!({"message": format!("{} backend={}", name, backend)}))
}

/// POST /cluster/corrector/set_function { "function": "json_fixer", "enabled": bool }
async fn corrector_set_function(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    let func = body.get("function").and_then(|v| v.as_str()).unwrap_or("");
    let enabled = body.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    info!("Corrector function {} → {}", func, enabled);
    // Persist to cluster_config.toml corrector_functions table
    update_corrector_function(func, enabled);
    Json(serde_json::json!({"message": format!("corrector.{} = {}", func, enabled)}))
}

/// GET /cluster/engines — probe what inference engines are installed
async fn get_available_engines() -> Json<serde_json::Value> {
    let mut available: Vec<&str> = Vec::new();

    // Check local binaries
    if std::path::Path::new(inference_client::LLAMA_SERVER_BIN).exists() {
        available.push("llama-server");
    }
    if std::path::Path::new("/home/cesarops/wreckhunter2000-1/cesarops-inference/target/release/cesarops-inference").exists() {
        available.push("cesarops-inference");
    }
    if std::path::Path::new("/usr/bin/koboldcpp").exists()
        || std::path::Path::new("/home/cesarops/koboldcpp").exists()
        || std::path::Path::new("/home/cesarops/benchmark/koboldcpp").exists() {
        available.push("koboldcpp");
    }
    // Check ollama
    if reqwest::Client::new()
        .get("http://localhost:11434/api/tags")
        .timeout(std::time::Duration::from_secs(1))
        .send().await.map(|r| r.status().is_success()).unwrap_or(false) {
        available.push("ollama");
    }

    // Per-node engines. M2200 (100.110.214.86) keeps Kobold; others default to llama-server.
    let per_node = serde_json::json!({
        "127.0.0.1":       available,
        "10.0.0.201":      ["llama-server", "koboldcpp"],
        "100.110.214.86":  ["koboldcpp", "llama-server"],
    });

    Json(serde_json::json!({ "available": available, "per_node": per_node }))
}

/// POST /cluster/memory_pool/create { "name": "pool1", "members": ["Coder","Thinker"] }
async fn memory_pool_create(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("pool1");
    let members: Vec<String> = body.get("members")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    info!("Memory pool created: {} members={:?}", name, members);
    // Persist pool to cluster_config.toml
    Json(serde_json::json!({"message": format!("Pool '{}' created with {} members", name, members.len())}))
}

/// GET /cluster/config — return full worker + pool config for the panel
async fn get_cluster_config_full() -> Json<serde_json::Value> {
    let config_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();

    // Build workers array from [[worker]] sections
    let workers: Vec<serde_json::Value> = table.get("worker")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(|w| {
            let t = w.as_table().cloned().unwrap_or_default();
            serde_json::json!({
                "name":          t.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "role":          t.get("role").and_then(|v| v.as_str()).unwrap_or(""),
                "node_ip":       t.get("host").and_then(|v| v.as_str()).unwrap_or("127.0.0.1"),
                "port":          t.get("port").and_then(|v| v.as_integer()).unwrap_or(5001),
                "engine":        t.get("engine").and_then(|v| v.as_str()).unwrap_or("llama-server"),
                "backend":       t.get("backend").and_then(|v| v.as_str()).unwrap_or("vulkan"),
                "inject_vectors":t.get("inject_vectors").and_then(|v| v.as_bool()).unwrap_or(true),
                "memory_pool":   t.get("memory_pool").and_then(|v| v.as_str()).unwrap_or(""),
                "model":         t.get("model").and_then(|v| v.as_str()).unwrap_or(""),
                "corrector_functions": t.get("corrector_functions").map(|v| {
                    serde_json::to_value(v).unwrap_or_default()
                }).unwrap_or_default(),
            })
        }).collect())
        .unwrap_or_default();

    let pools: Vec<serde_json::Value> = table.get("memory_pool")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(|p| {
            let t = p.as_table().cloned().unwrap_or_default();
            serde_json::json!({
                "name":    t.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "members": t.get("members").map(|v| serde_json::to_value(v).unwrap_or_default()).unwrap_or_default(),
            })
        }).collect())
        .unwrap_or_default();

    Json(serde_json::json!({ "workers": workers, "pools": pools }))
}

/// GET /cluster/routing — agents, presets, live routing state
async fn get_routing_status() -> Json<serde_json::Value> {
    let cluster = routing::load_cluster_routing();
    let state = routing::load_routing_state();
    let resolved = routing::resolve_endpoints();
    let presets: Vec<serde_json::Value> = cluster
        .presets
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "description": p.description,
                "chat_agent": p.chat_agent,
                "coder_endpoint": p.coder_endpoint,
                "workers_start": p.workers_start,
            })
        })
        .collect();
    let agents: Vec<serde_json::Value> = cluster
        .agents
        .iter()
        .map(|a| {
            serde_json::json!({
                "name": a.name,
                "endpoint": a.endpoint,
                "hardware": a.hardware,
                "role": a.role,
                "model": a.model,
                "template": a.template,
            })
        })
        .collect();
    let gpus: Vec<serde_json::Value> = {
        let content = std::fs::read_to_string(routing::CFG_PATH).unwrap_or_default();
        let table: toml::Table = content.parse().unwrap_or_default();
        table
            .get("gpu")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|g| {
                        let t = g.as_table()?;
                        Some(serde_json::json!({
                            "id": t.get("id")?.as_integer().unwrap_or(0),
                            "name": t.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "vram_mb": t.get("vram_mb").and_then(|v| v.as_integer()).unwrap_or(0),
                        }))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    Json(serde_json::json!({
        "agents": agents,
        "presets": presets,
        "routing_state": state,
        "resolved": resolved,
        "gpus": gpus,
        "nicknames": cluster.nicknames,
        "workers": cluster.workers,
    }))
}

/// POST /cluster/routing — set chat agent + optional endpoint overrides
async fn save_routing_state(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    let mut state = routing::load_routing_state();
    if let Some(v) = body.get("chat_agent").and_then(|v| v.as_str()) {
        state.chat_agent = v.to_string();
    }
    for key in [
        "coder_endpoint",
        "reviewer_endpoint",
        "thinker_endpoint",
        "corrector_endpoint",
        "draft_endpoint",
    ] {
        if let Some(v) = body.get(key).and_then(|v| v.as_str()) {
            match key {
                "coder_endpoint" => state.coder_endpoint = v.to_string(),
                "reviewer_endpoint" => state.reviewer_endpoint = v.to_string(),
                "thinker_endpoint" => state.thinker_endpoint = v.to_string(),
                "corrector_endpoint" => state.corrector_endpoint = v.to_string(),
                "draft_endpoint" => state.draft_endpoint = v.to_string(),
                _ => {}
            }
        }
    }
    routing::save_routing_state(&state);
    Json(serde_json::json!({"message": "Routing saved", "routing_state": state}))
}

/// POST /cluster/routing/preset/{id} — apply preset, optionally start workers
async fn apply_routing_preset(
    axum::extract::Path(preset_id): axum::extract::Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let start_workers = body
        .get("start_workers")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let preset = match routing::apply_preset_to_state(&preset_id) {
        Ok(p) => p,
        Err(e) => return Json(serde_json::json!({"error": e})),
    };

    let mut started = Vec::new();
    let mut failed = Vec::new();
    if start_workers && !preset.workers_stop.is_empty() {
        for w in &preset.workers_stop {
            let url = format!("http://127.0.0.1:9100/cluster/worker/{}/stop", w);
            let _ = reqwest::Client::new().post(&url).send().await;
        }
    }
    if start_workers {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .unwrap();
        for w in &preset.workers_start {
            let url = format!("http://127.0.0.1:9100/cluster/worker/{}/start", w);
            match client.post(&url).send().await {
                Ok(r) if r.status().is_success() => started.push(w.clone()),
                Ok(r) => failed.push(format!("{} (HTTP {})", w, r.status())),
                Err(e) => failed.push(format!("{} ({})", w, e)),
            }
        }
    }

    Json(serde_json::json!({
        "message": format!("Applied routing preset '{}'", preset.name),
        "preset": preset.id,
        "started": started,
        "failed": failed,
    }))
}

// ── Config persistence helpers ───────────────────────────────────────────────

// ── Config persistence helpers ───────────────────────────────────────────────
//
// Round-trip via toml_edit so we preserve comments, ordering, and inline-table
// formatting in cluster_config.toml. The cluster panel's APPLY / per-card
// toggles persist real config changes, not just log lines.

const CFG_PATH: &str = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";

fn json_to_toml_edit(v: &serde_json::Value) -> toml_edit::Item {
    use toml_edit::{Item, Value, Array, InlineTable, Formatted};
    match v {
        serde_json::Value::Null => Item::Value(Value::String(Formatted::new(String::new()))),
        serde_json::Value::Bool(b) => Item::Value(Value::Boolean(Formatted::new(*b))),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Item::Value(Value::Integer(Formatted::new(i)))
            } else {
                Item::Value(Value::Float(Formatted::new(n.as_f64().unwrap_or(0.0))))
            }
        }
        serde_json::Value::String(s) => Item::Value(Value::String(Formatted::new(s.clone()))),
        serde_json::Value::Array(arr) => {
            let mut a = Array::new();
            for item in arr {
                if let Item::Value(val) = json_to_toml_edit(item) {
                    a.push(val);
                }
            }
            Item::Value(Value::Array(a))
        }
        serde_json::Value::Object(obj) => {
            let mut t = InlineTable::new();
            for (k, val) in obj {
                if let Item::Value(v) = json_to_toml_edit(val) {
                    t.insert(k, v);
                }
            }
            Item::Value(Value::InlineTable(t))
        }
    }
}

fn find_worker_idx(doc: &toml_edit::DocumentMut, name: &str) -> Option<usize> {
    let workers = doc.get("worker")?.as_array_of_tables()?;
    workers.iter().position(|t| {
        t.get("name").and_then(|v| v.as_str()) == Some(name)
    })
}

fn update_worker_config(name: &str, config: &serde_json::Value) {
    let content = match std::fs::read_to_string(CFG_PATH) {
        Ok(s) => s,
        Err(e) => { warn!("update_worker_config read: {}", e); return; }
    };
    let mut doc: toml_edit::DocumentMut = match content.parse() {
        Ok(d) => d,
        Err(e) => { warn!("update_worker_config parse: {}", e); return; }
    };
    let idx = match find_worker_idx(&doc, name) {
        Some(i) => i,
        None => { warn!("update_worker_config: '{}' not found", name); return; }
    };
    if let Some(workers) = doc.get_mut("worker").and_then(|v| v.as_array_of_tables_mut()) {
        if let Some(table) = workers.get_mut(idx) {
            if let Some(obj) = config.as_object() {
                for (k, v) in obj {
                    if k == "corrector_functions" || v.is_string() || v.is_boolean() || v.is_number() {
                        table.insert(k, json_to_toml_edit(v));
                    }
                }
            }
        }
    }
    if let Err(e) = std::fs::write(CFG_PATH, doc.to_string()) {
        warn!("update_worker_config write: {}", e);
    } else {
        info!("Worker '{}' persisted to cluster_config.toml", name);
    }
}

fn update_worker_field(name: &str, field: &str, value: serde_json::Value) {
    let content = match std::fs::read_to_string(CFG_PATH) {
        Ok(s) => s,
        Err(e) => { warn!("update_worker_field read: {}", e); return; }
    };
    let mut doc: toml_edit::DocumentMut = match content.parse() {
        Ok(d) => d,
        Err(e) => { warn!("update_worker_field parse: {}", e); return; }
    };
    let idx = match find_worker_idx(&doc, name) {
        Some(i) => i,
        None => { warn!("update_worker_field: '{}' not found", name); return; }
    };
    if let Some(workers) = doc.get_mut("worker").and_then(|v| v.as_array_of_tables_mut()) {
        if let Some(table) = workers.get_mut(idx) {
            table.insert(field, json_to_toml_edit(&value));
        }
    }
    if let Err(e) = std::fs::write(CFG_PATH, doc.to_string()) {
        warn!("update_worker_field write: {}", e);
    } else {
        info!("Worker '{}' field '{}' persisted", name, field);
    }
}

fn update_corrector_function(func: &str, enabled: bool) {
    let content = match std::fs::read_to_string(CFG_PATH) {
        Ok(s) => s,
        Err(e) => { warn!("update_corrector_function read: {}", e); return; }
    };
    let mut doc: toml_edit::DocumentMut = match content.parse() {
        Ok(d) => d,
        Err(e) => { warn!("update_corrector_function parse: {}", e); return; }
    };
    let mut written = false;
    if let Some(workers) = doc.get_mut("worker").and_then(|v| v.as_array_of_tables_mut()) {
        for w in workers.iter_mut() {
            let is_corrector = w.get("role").and_then(|v| v.as_str()) == Some("correct");
            if !is_corrector { continue; }
            let entry = w.entry("corrector_functions").or_insert_with(|| {
                toml_edit::Item::Value(toml_edit::Value::InlineTable(toml_edit::InlineTable::new()))
            });
            if let toml_edit::Item::Value(toml_edit::Value::InlineTable(t)) = entry {
                t.insert(func, toml_edit::Value::Boolean(toml_edit::Formatted::new(enabled)));
                written = true;
            }
        }
    }
    if !written {
        let entry = doc.entry("corrector_functions").or_insert_with(|| {
            toml_edit::Item::Table(toml_edit::Table::new())
        });
        if let toml_edit::Item::Table(t) = entry {
            t.insert(func, toml_edit::Item::Value(
                toml_edit::Value::Boolean(toml_edit::Formatted::new(enabled))
            ));
        }
    }
    if let Err(e) = std::fs::write(CFG_PATH, doc.to_string()) {
        warn!("update_corrector_function write: {}", e);
    } else {
        info!("corrector_functions.{} = {} persisted", func, enabled);
    }
}

/// POST /validate { "prompt": "optional" }
async fn validate_endpoint(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let prompt = body
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("The quick brown fox jumps over the lazy dog. In Rust, a vector is");

    let config = validator::ValidatorConfig {
        main_endpoint: state.config.read().await.coder_url.clone(),
        ref_endpoint: state.config.read().await.validator_url.clone(),
        n_tokens: 10,
        min_agreement: 0.4,
    };

    let result = validator::run_validation(&config, prompt).await;
    Json(serde_json::to_value(&result).unwrap_or_default())
}

/// GET /validate/ping — quick liveness check of both main engine and P1000.
async fn validate_ping(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cfg = state.config.read().await;
    let main_up = validator::ping(&cfg.coder_url).await;
    let p1000_up = validator::ping(&cfg.validator_url).await;

    let main_tps = if main_up {
        validator::benchmark_tps(&cfg.coder_url, 5).await
    } else {
        None
    };
    let p1000_tps = if p1000_up {
        validator::benchmark_tps(&cfg.validator_url, 5).await
    } else {
        None
    };

    Json(serde_json::json!({
        "main_engine": {
            "url": cfg.coder_url,
            "online": main_up,
            "tps": main_tps,
        },
        "p1000_validator": {
            "url": cfg.validator_url,
            "online": p1000_up,
            "tps": p1000_tps,
        },
        "status": if main_up { "ok" } else { "main_engine_down" },
    }))
}

// ── IDE Backend Handlers ─────────────────────────────────────────────────────

/// GET /ide — serve the IDE HTML page (will be replaced by Gemma's output)
async fn ide_page() -> Html<&'static str> {
    Html(include_str!("ide.html"))
}

/// GET /ide/file?path=/path/to/file — read a file's contents
async fn ide_read_file(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let path = match params.get("path") {
        Some(p) => p,
        None => return Json(serde_json::json!({"error": "path parameter required"})),
    };
    match std::fs::read_to_string(path) {
        Ok(content) => Json(serde_json::json!({"path": path, "content": content})),
        Err(e) => Json(serde_json::json!({"error": format!("read: {}", e), "path": path})),
    }
}

/// POST /ide/file — write a file
async fn ide_write_file(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    let path = match body.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return Json(serde_json::json!({"error": "path required"})),
    };
    let content = match body.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return Json(serde_json::json!({"error": "content required"})),
    };
    match std::fs::write(path, content) {
        Ok(_) => Json(serde_json::json!({"status": "ok", "path": path})),
        Err(e) => Json(serde_json::json!({"error": format!("write: {}", e)})),
    }
}

/// GET /ide/tree?root=/path — list directory tree (one level deep)
async fn ide_file_tree(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let root = params.get("root").map(|s| s.as_str())
        .unwrap_or("/home/cesarops/wreckhunter2000-1");
    let mut entries = Vec::new();
    if let Ok(dir) = std::fs::read_dir(root) {
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && name != ".env" { continue; }
            let is_dir = entry.path().is_dir();
            entries.push(serde_json::json!({
                "name": name,
                "path": entry.path().to_string_lossy(),
                "type": if is_dir { "dir" } else { "file" },
            }));
        }
    }
    entries.sort_by(|a, b| {
        let a_dir = a["type"] == "dir";
        let b_dir = b["type"] == "dir";
        b_dir.cmp(&a_dir).then(a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")))
    });
    Json(serde_json::Value::Array(entries))
}

/// POST /ide/exec — run a shell command and return output
async fn ide_exec(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    let command = match body.get("command").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return Json(serde_json::json!({"error": "command required"})),
    };
    let cwd = body.get("cwd").and_then(|v| v.as_str())
        .unwrap_or("/home/cesarops/wreckhunter2000-1");

    let output = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            Json(serde_json::json!({
                "exit_code": out.status.code(),
                "stdout": stdout,
                "stderr": stderr,
            }))
        }
        Err(e) => Json(serde_json::json!({"error": format!("exec: {}", e)})),
    }
}

/// POST /ide/chat/stream — proxy a completion stream to llama-server (default) or Kobold.
async fn ide_chat_stream(
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    use axum::body::Body;

    let endpoint = body.get("endpoint").and_then(|v| v.as_str())
        .unwrap_or("http://127.0.0.1:5001");
    let prompt = body.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
    let max_length = body.get("max_length").and_then(|v| v.as_u64()).unwrap_or(1024);
    let temperature = body.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.4);
    let engine = body.get("engine").and_then(|v| v.as_str());

    let (url, payload) = if inference_client::uses_kobold_generate_api(engine) {
        (
            format!("{}/api/extra/generate/stream", endpoint.trim_end_matches('/')),
            serde_json::json!({
                "prompt": prompt,
                "max_length": max_length,
                "temperature": temperature,
                "top_p": 0.95,
                "rep_pen": 1.1,
            }),
        )
    } else {
        (
            format!("{}/v1/completions", endpoint.trim_end_matches('/')),
            serde_json::json!({
                "prompt": prompt,
                "max_tokens": max_length,
                "temperature": temperature,
                "stream": true,
            }),
        )
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let resp = match client.post(&url).json(&payload).send().await {
        Ok(r) => r,
        Err(e) => {
            let body = Body::from(format!("data: {{\"error\": \"{}\"}}\n\n", e));
            return axum::response::Response::builder()
                .header("Content-Type", "text/event-stream")
                .header("Cache-Control", "no-cache")
                .body(body)
                .unwrap();
        }
    };

    // Read full response and forward as SSE (kobold NDJSON or llama-server chunks).
    let body_bytes = resp.bytes().await.unwrap_or_default();
    let body = Body::from(body_bytes);

    axum::response::Response::builder()
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(body)
        .unwrap()
}

// ── Model Swap Route ────────────────────────────────────────────────────────

/// POST /orchestrator/swap — swap a model on any node in the fleet.
///
/// Body:
/// ```json
/// {
///   "host": "10.0.0.41",        // node IP (or "local" for T440 P100s)
///   "worker": "GemmaBig",       // worker name (for local swaps)
///   "model_path": "/path/to/model.gguf",
///   "port": 5100,
///   "gpu_layers": 999,
///   "context_size": 8192
/// }
/// ```
async fn orchestrator_swap_model(
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let host = body.get("host").and_then(|v| v.as_str()).unwrap_or("local");
    let model_path = match body.get("model_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return Json(serde_json::json!({"error": "model_path required"})),
    };
    let port = body.get("port").and_then(|v| v.as_u64()).unwrap_or(5100) as u16;
    let gpu_layers = body.get("gpu_layers").and_then(|v| v.as_u64()).unwrap_or(999) as u32;
    let context_size = body.get("context_size").and_then(|v| v.as_u64()).unwrap_or(8192) as u32;

    if host == "local" {
        let worker = body.get("worker").and_then(|v| v.as_str()).unwrap_or("GemmaBig");
        match orchestrator::swap_local_worker(worker, model_path).await {
            Ok(model) => Json(serde_json::json!({"status": "ok", "model": model, "worker": worker})),
            Err(e) => Json(serde_json::json!({"error": e})),
        }
    } else {
        match orchestrator::swap_model_on_node(host, model_path, port, gpu_layers, context_size).await {
            Ok(model) => Json(serde_json::json!({"status": "ok", "model": model, "host": host, "port": port})),
            Err(e) => Json(serde_json::json!({"error": e})),
        }
    }
}

// ── Webhook Mission Intake ──────────────────────────────────────────────────

fn operator_scenario_from_json(body: &serde_json::Value) -> Result<orchestrator::OperatorScenario, String> {
    let raw_text = body
        .get("scenario")
        .or_else(|| body.get("raw_text"))
        .and_then(|v| v.as_str())
        .unwrap_or("satellite mission")
        .to_string();
    let priority = body.get("priority").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
    let bbox: Option<[f64; 4]> = body
        .get("bbox")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let days_back = body.get("days_back").and_then(|v| v.as_u64()).map(|d| d as u32);
    let spec_path = body
        .get("spec_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let knobs = body.get("knobs").cloned();
    let stages: Option<Vec<String>> = body.get("stages").and_then(|v| {
        v.as_array().map(|arr| {
            arr.iter()
                .filter_map(|s| s.as_str().map(|x| x.to_string()))
                .collect()
        })
    });
    let mut dry_run = body.get("dry_run").and_then(|v| v.as_bool());
    if dry_run.is_none() {
        if let Some(k) = body.get("knobs") {
            dry_run = k
                .get("dry_run_download")
                .and_then(|v| v.as_bool());
        }
    }
    if dry_run.is_none() {
        if let Some(ref sp) = spec_path {
            if let Ok(content) = std::fs::read_to_string(sp) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    dry_run = v
                        .get("knobs")
                        .and_then(|k| k.get("dry_run_download"))
                        .and_then(|v| v.as_bool());
                }
            }
        }
    }
    let pipeline_mode = body
        .get("pipeline_mode")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            if spec_path.is_some() {
                Some("sequential".to_string())
            } else {
                None
            }
        });

    Ok(orchestrator::OperatorScenario {
        raw_text,
        priority,
        bbox,
        days_back,
        spec_path,
        knobs,
        stages,
        dry_run,
        pipeline_mode,
    })
}

/// POST /webhook/satellite — n8n entry: JSON mission spec + optional knob overrides.
async fn webhook_satellite(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let mut body = body;
    if body.get("spec_path").is_none() {
        body["spec_path"] = serde_json::json!(
            "/codebase/repos/wreckhunter2000-1/pipelines/satellite/missions/straits_known_wreck_validation.json"
        );
    }
    if body.get("scenario").is_none() && body.get("raw_text").is_none() {
        body["scenario"] = serde_json::json!("straits known wreck satellite validation");
    }
    if body.get("bbox").is_none() {
        body["bbox"] = serde_json::json!([45.6, -85.6, 46.2, -84.3]);
    }
    if body.get("dry_run").is_none() {
        body["dry_run"] = serde_json::json!(true);
    }
    body["pipeline_mode"] = serde_json::json!("sequential");
    body["source"] = serde_json::json!("webhook_satellite");
    webhook_mission(State(state), Json(body)).await
}

/// POST /webhook/mission — accept a mission from cesarops.com or any external source.
/// Returns immediately with a mission_id; execution runs in background.
async fn webhook_mission(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let scenario = match operator_scenario_from_json(&body) {
        Ok(s) => s,
        Err(e) => return Json(serde_json::json!({"error": e})),
    };
    let scenario_text = scenario.raw_text.clone();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap();
    let mission_id = format!("{:016x}", now.as_nanos() & 0xFFFFFFFFFFFFFFFF);
    let source = body.get("source").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let callback_url = body.get("callback_url").and_then(|v| v.as_str()).map(|s| s.to_string());

    let record = MissionRecord {
        id: mission_id.clone(),
        source: source.clone(),
        scenario_text: scenario_text.clone(),
        status: "running".to_string(),
        submitted_at: now.as_secs(),
        completed_at: None,
        report: None,
    };

    {
        let mut missions = state.missions.lock().await;
        missions.push(record);
        // Cap at 50 entries.
        if missions.len() > 50 {
            let excess = missions.len() - 50;
            missions.drain(0..excess);
        }
    }

    // Spawn background execution.
    let state_clone = state.clone();
    let mid = mission_id.clone();
    tokio::spawn(async move {
        let report = orchestrator::execute_mission(scenario).await;

        // Update mission record.
        {
            let mut missions = state_clone.missions.lock().await;
            if let Some(rec) = missions.iter_mut().find(|m| m.id == mid) {
                rec.status = report.status.clone();
                rec.completed_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
                );
                rec.report = Some(report.clone());
            }
        }

        // Best-effort callback.
        if let Some(url) = callback_url {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            let _ = client.post(&url).json(&report).send().await;
        }
    });

    Json(serde_json::json!({
        "status": "accepted",
        "mission_id": mission_id,
    }))
}

/// GET /webhook/missions — list recent missions.
async fn list_missions(State(state): State<AppState>) -> Json<serde_json::Value> {
    let missions = state.missions.lock().await;
    let list: Vec<serde_json::Value> = missions.iter().map(|m| {
        serde_json::json!({
            "id": m.id,
            "source": m.source,
            "scenario": m.scenario_text,
            "status": m.status,
            "submitted_at": m.submitted_at,
            "completed_at": m.completed_at,
        })
    }).collect();
    Json(serde_json::Value::Array(list))
}

/// GET /webhook/missions/:id — single mission status + report when complete.
async fn get_mission(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    let missions = state.missions.lock().await;
    match missions.iter().find(|m| m.id == id) {
        Some(m) => Json(serde_json::json!({
            "id": m.id,
            "source": m.source,
            "scenario": m.scenario_text,
            "status": m.status,
            "submitted_at": m.submitted_at,
            "completed_at": m.completed_at,
            "report": m.report,
        })),
        None => Json(serde_json::json!({"error": "mission not found", "id": id})),
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("cesarops_forge_v2=info")
        .init();

    let resolved = routing::resolve_endpoints();
    let config = ForgeConfig {
        coder_url: resolved.coder_url.clone(),
        thinker_url: resolved.thinker_url.clone(),
        corrector_url: resolved.corrector_url.clone(),
        nautivecs_url: "http://127.0.0.1:5003/query".to_string(),
        wso_url: "http://127.0.0.1:5010/search".to_string(),
        project_root: paths::project_root(),
        validator_url: resolved.validator_url.clone(),
        chat_agent: resolved.chat_agent.clone(),
        chat_template: resolved.chat_template.clone(),
        chat_model: resolved.chat_model.clone(),
    };

    let state = AppState {
        conversation: Arc::new(Mutex::new(Vec::new())),
        config: Arc::new(RwLock::new(config)),
        interrupt: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        steering: Arc::new(Mutex::new(Vec::new())),
        node_registry: Arc::new(Mutex::new(std::collections::HashMap::new())),
        missions: Arc::new(Mutex::new(Vec::new())),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/cluster", get(cluster_panel))
        // ── IDE routes ───────────────────────────────────────────────────
        .route("/ide", get(ide_page))
        .route("/ide/file", get(ide_read_file).post(ide_write_file))
        .route("/ide/tree", get(ide_file_tree))
        .route("/ide/exec", post(ide_exec))
        .route("/ide/chat/stream", post(ide_chat_stream))
        // ── Webhook intake ───────────────────────────────────────────────
        .route("/webhook/mission", post(webhook_mission))
        .route("/webhook/satellite", post(webhook_satellite))
        .route("/webhook/missions", get(list_missions))
        .route("/webhook/missions/{id}", get(get_mission))
        .route("/health", get(health))
        .route("/send", post(send_message))
        .route("/clear", post(clear))
        .route("/interrupt", post(interrupt))
        .route("/steer", post(steer))
        .route("/monitor", get(monitor))
        .route("/cluster/config", get(get_cluster_config).post(save_cluster_config))
        .route("/cluster/models", get(list_available_models))
        .route("/cluster/preset/config", get(get_preset_config).post(save_preset_config))
        .route("/cluster/preset/activate", post(activate_preset))
        .route("/cluster/preset/launch", post(launch_preset))
        .route("/cluster/preset/deactivate", post(deactivate_preset))
        .route("/cluster/freeform/apply", post(apply_freeform))
        .route("/tool/{name}", post(invoke_tool))
        .route("/mode", get(get_mode))
        .route("/mode/cesarops", post(mode_cesarops))
        .route("/mode/coding", post(mode_coding))
        .route("/code", post(route_code_request))
        .route("/cluster/agent/run", post(run_agent_task))
        .route("/cluster/corrector/connect", post(corrector_connect))
        .route("/cluster/corrector/disconnect", post(corrector_disconnect))
        .route("/validate", post(validate_endpoint))
        .route("/validate/ping", get(validate_ping))
        .route("/cluster/worker/{idx}/start", post(start_worker))
        .route("/cluster/worker/{idx}/stop", post(stop_worker))
        .route("/cluster/start-all", post(start_all_workers))
        .route("/cluster/stop-all", post(stop_all_workers))
        .route("/cluster/discover", get(discover_nodes))
        // ── DII Node Registry ────────────────────────────────────────────
        .route("/cluster/node/register",  post(node_register))
        .route("/cluster/node/heartbeat", post(node_heartbeat))
        .route("/cluster/nodes",          get(list_registered_nodes))
        // ── New per-card control routes ──────────────────────────────────
        .route("/cluster/worker/{name}/apply",         post(worker_apply))
        .route("/cluster/worker/{name}/set_injection", post(worker_set_injection))
        .route("/cluster/worker/{name}/set_backend",   post(worker_set_backend))
        .route("/cluster/corrector/set_function",      post(corrector_set_function))
        .route("/cluster/engines",                     get(get_available_engines))
        .route("/cluster/memory_pool/create",          post(memory_pool_create))
        .route("/cluster/config/full",                 get(get_cluster_config_full))
        .route("/cluster/routing",                     get(get_routing_status).post(save_routing_state))
        .route("/cluster/routing/preset/{id}",         post(apply_routing_preset))
        // ── Mission orchestrator (T9 design / T11 implementation) ─────────
        .route("/orchestrator/probe",   get(orchestrator::orchestrator_probe))
        .route("/orchestrator/plan",    post(orchestrator::orchestrator_plan))
        .route("/orchestrator/execute", post(orchestrator::orchestrator_execute))
        .route("/orchestrator/swap",    post(orchestrator_swap_model))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 9100));
    info!("cesarops-forge-v2 (Self-Healing Knowledge Translator) on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
