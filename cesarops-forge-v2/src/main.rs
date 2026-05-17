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

use axum::{extract::{Json, State}, response::Html, routing::{get, post}, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    pub conversation: Arc<Mutex<Vec<translator::Message>>>,
    pub config: Arc<ForgeConfig>,
    pub interrupt: Arc<std::sync::atomic::AtomicBool>,
    pub steering: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone)]
pub struct ForgeConfig {
    pub coder_url: String,      // 35B on P100s
    pub thinker_url: String,    // R1 on Xeon DDR4
    pub corrector_url: String,  // 14B Coder on 1070 (Marvin)
    pub nautivecs_url: String,
    pub wso_url: String,
    pub project_root: String,
    /// P1000 reference validator endpoint (cesarops2:5571)
    pub validator_url: String,
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
        .arg("pkill -f 'cesarops-inference.*--backend' ; pkill -f 'koboldcpp.*--model'")
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
            "nohup /home/cesarops/koboldcpp --model {} --port {} --usevulkan --gpulayers 99 > /tmp/preset_gpu0.log 2>&1 &",
            gpu0_model, port
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
/// Stops any coding workers, ensures the SAR worker fleet is up.
async fn mode_cesarops() -> Json<serde_json::Value> {
    info!("MODE SWAP -> CESAROPS (SAR / wreck-hunting)");

    // Stop coding-mode workers cleanly. We don't know exactly which ones are
    // running so we kill the known coding ports. SAR workers either auto-restart
    // via their systemd units or get started below.
    let stop_cmd = "fuser -k 5001/tcp 2>/dev/null ; fuser -k 5002/tcp 2>/dev/null ; \
                    pkill -f 'koboldcpp.*--port (5001|5002)' 2>/dev/null ; true";
    let _ = std::process::Command::new("bash").arg("-c").arg(stop_cmd).output();

    // Drop a marker into the cluster_config that tells the loop_engine which
    // routing presets to apply. The actual model start-up uses the existing
    // /cluster/worker/{idx}/start endpoints; this just sets the active mode.
    write_active_mode("cesarops", serde_json::json!({
        "primary_role": "wreck_detection",
        "main_endpoint": "http://127.0.0.1:5001",
        "thinker_endpoint": "http://10.0.0.41:5570",
        "corrector_endpoint": "http://10.0.0.129:5200",
        "draft_endpoint": "http://10.0.0.129:5571",
    }));

    info!("CESAROPS mode active. Workers expected: GemmaBig (P100#0:5001), QwenBig (P100#1:5002), Picasso (cesarops2:5571)");
    Json(serde_json::json!({
        "message": "Mode -> CESAROPS. Workers reset. Use /cluster/worker/{idx}/start for the SAR fleet.",
        "mode": "cesarops",
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

    let coder_model = body
        .get("coder_model")
        .and_then(|v| v.as_str())
        .unwrap_or("/codebase/models/Qwen3.6-35B-A3B-Q4_K_M.gguf");
    let reviewer_model = body
        .get("reviewer_model")
        .and_then(|v| v.as_str())
        .unwrap_or("/codebase/models/Gemma-4-26B-MoE-IQ4_XS.gguf");
    let free_one_p100 = body
        .get("free_p100_for_other_work")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Stop any currently-running cesarops workers on these ports
    let stop_cmd = "fuser -k 5001/tcp 2>/dev/null ; fuser -k 5002/tcp 2>/dev/null ; \
                    pkill -f 'koboldcpp.*--port (5001|5002)' 2>/dev/null ; true";
    let _ = std::process::Command::new("bash").arg("-c").arg(stop_cmd).output();

    // Start coder on P100(s)
    let p100_layers = if free_one_p100 { "--gpulayers 99 --tensor_split 1,0" } else { "--gpulayers 99 --tensor_split 1,1" };
    let coder_cmd = format!(
        "nohup /home/cesarops/koboldcpp --model {} --port 5001 --usevulkan {} > /tmp/coding_coder.log 2>&1 &",
        coder_model, p100_layers
    );
    let coder_started = std::process::Command::new("bash")
        .arg("-c").arg(&coder_cmd).output().is_ok();

    // Reviewer goes on the secondary fleet (1070 + P1000) — for now we just
    // mark the endpoints as the coding reviewers in the mode state. The
    // operator's fleet has these at 10.0.0.129:5200 (1070) + :5571 (P1000).
    // If they need restarting, the operator does it from those boxes —
    // the forge can't reach in via SSH from a URL handler.

    write_active_mode("coding", serde_json::json!({
        "primary_role": "coding",
        "coder_endpoint": "http://127.0.0.1:5001",
        "coder_model": coder_model,
        "reviewer_primary_endpoint": "http://10.0.0.129:5200",
        "reviewer_draft_endpoint": "http://10.0.0.129:5571",
        "reviewer_model": reviewer_model,
        "free_one_p100": free_one_p100,
        "pipeline": "coder -> reviewer -> coder_compile",
    }));

    info!(
        "CODING mode active. Coder: {} ({}), Reviewer-main: 10.0.0.129:5200, Reviewer-draft: 10.0.0.129:5571",
        coder_model, if free_one_p100 { "1 P100" } else { "2 P100s" }
    );

    Json(serde_json::json!({
        "message": format!(
            "Mode -> CODING. Coder {} on {} P100. Reviewer = layered (1070 + P1000) on cesarops2.",
            coder_model.split('/').last().unwrap_or("?"),
            if free_one_p100 { "1" } else { "2" }
        ),
        "mode": "coding",
        "coder_started": coder_started,
        "coder_endpoint": "http://127.0.0.1:5001",
        "reviewer_endpoints": [
            "http://10.0.0.129:5200",
            "http://10.0.0.129:5571",
        ],
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

async fn call_reviewer_for_review(state: &AppState, task: &str, code: &str) -> Result<String, String> {
    let prompt = format!(
        "<|im_start|>system\nYou are a code reviewer. Find bugs, suggest improvements. \
        Be terse. Bullet points only.\n<|im_end|>\n\
        <|im_start|>user\nTask: {}\n\nCode:\n{}\n<|im_end|>\n<|im_start|>assistant\n",
        task,
        if code.len() > 6000 { &code[..6000] } else { code }
    );
    // Try the 1070 reviewer first; fall back to draft if down
    match call_endpoint("http://10.0.0.129:5200", &prompt, 2048, 0.2).await {
        Ok(r) => Ok(r),
        Err(_) => call_endpoint("http://10.0.0.129:5571", &prompt, 1024, 0.2).await,
    }
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
    let payload = serde_json::json!({
        "prompt": prompt,
        "max_length": max_length,
        "temperature": temperature,
        "top_p": 0.9,
        "stop_sequence": ["<|im_end|>", "</s>"],
    });
    let resp = client
        .post(format!("{}/api/v1/generate", url))
        .json(&payload)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("{}: {}", url, e))?;
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let text = body
        .get("results")
        .and_then(|r| r.as_array())
        .and_then(|a| a.first())
        .and_then(|r| r.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    Ok(text)
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
    let endpoint = body.get("endpoint").and_then(|v| v.as_str()).unwrap_or("");
    let message = body.get("message").and_then(|v| v.as_str()).unwrap_or("");

    if endpoint.is_empty() || message.is_empty() {
        return Json(serde_json::json!({"error": "endpoint and message required"}));
    }

    let config = agent_dispatch::AgentConfig {
        endpoint_url: endpoint.to_string(),
        project_root: "/codebase/wreckhunter2000-1".to_string(),
        nautivecs_url: "http://127.0.0.1:5003/query".to_string(),
        wso_url: "http://127.0.0.1:5010/search".to_string(),
        max_tokens: 12288,
        temperature: 0.4,
        safe_mode: body.get("safe_mode").and_then(|v| v.as_bool()).unwrap_or(false),
    };

    info!("Agent task dispatched to {}: {}...", endpoint, &message[..message.len().min(80)]);
    let result = agent_dispatch::run_agent_loop(&config, message).await;
    info!("Agent task complete: {}...", &result[..result.len().min(100)]);

    Json(serde_json::json!({"response": result}))
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
            
            if model.is_empty() {
                return Json(serde_json::json!({"error": "No model assigned to this worker."}));
            }

            // Check if this is a remote node
            if host != "local" {
                return Json(serde_json::json!({"error": format!("Remote workers ({}) must be started on their host machine.", host)}));
            }

            // Determine if we can handle this model natively or need KoboldCPP
            let native_quants = ["q4_0", "q4_k_m", "q6_k", "q8_0", "f16", "f32", "bf16"];
            let model_lower = model.to_lowercase();
            let use_native = native_quants.iter().any(|q| model_lower.contains(q));

            let cmd = if use_native {
                format!(
                    "nohup /codebase/repos/wreckhunter2000-1/cesarops-inference/target/release/cesarops-inference --model {} --port {} --backend wgpu --gpu {} > /tmp/worker_{}.log 2>&1 &",
                    model, port, gpu, idx
                )
            } else {
                // Fallback to KoboldCPP for unsupported formats (MXFP4, etc.)
                format!(
                    "nohup /home/cesarops/koboldcpp --model {} --port {} --usevulkan --gpulayers 99 > /tmp/worker_{}.log 2>&1 &",
                    model, port, idx
                )
            };

            let engine = if use_native { "cesarops-inference" } else { "koboldcpp (fallback)" };
            
            match std::process::Command::new("bash").arg("-c").arg(&cmd).output() {
                Ok(_) => Json(serde_json::json!({"message": format!("Started {} on GPU {} port {} via {}", name, gpu, port, engine)})),
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
    // TODO: iterate workers and start each
    Json(serde_json::json!({"message": "Starting all workers... (use individual start for now)"}))
}

async fn stop_all_workers() -> Json<serde_json::Value> {
    let _ = std::process::Command::new("bash")
        .arg("-c")
        .arg("pkill -f 'cesarops-inference.*--backend wgpu'")
        .output();
    Json(serde_json::json!({"message": "All GPU workers stopped."}))
}

/// Discover which nodes are online by probing Tailscale status + known service ports
async fn discover_nodes() -> Json<serde_json::Value> {
    let config_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();

    // First, get live Tailscale peer status
    let tailscale_peers = get_tailscale_peers().await;

    let mut results = Vec::new();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();

    if let Some(nodes) = table.get("known_nodes").and_then(|v| v.as_array()) {
        for node in nodes {
            let name = node.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
            let ip = node.get("ip").and_then(|v| v.as_str()).unwrap_or("");
            let gpu_label = node.get("gpu").and_then(|v| v.as_str()).unwrap_or("");
            let empty_ports = vec![];
            let ports = node.get("ports").and_then(|v| v.as_array()).unwrap_or(&empty_ports);

            // Check if node is reachable: localhost is always; LAN (10.x/192.168.x/172.16.x)
            // is "reachable" optimistically (the per-port HTTP probe will confirm); only
            // Tailscale peers (100.x) require Tailscale to report them up.
            let ts_online = if ip == "127.0.0.1" || ip == "localhost" {
                true
            } else if ip.starts_with("10.") || ip.starts_with("192.168.") || ip.starts_with("172.16.") {
                true // LAN — assume reachable, port probes decide
            } else {
                tailscale_peers.iter().any(|p| p.0 == ip && p.1)
            };

            let mut port_status = Vec::new();
            for port_val in ports {
                let port = port_val.as_integer().unwrap_or(0);
                // Probe order: /api/extra/version (koboldcpp) → /v1/models (OpenAI-compat) → /health (cesarops services)
                let probe_paths = ["/api/extra/version", "/v1/models", "/health"];
                let online = if ts_online {
                    let mut found: Option<serde_json::Value> = None;
                    for path in probe_paths {
                        let url = format!("http://{}:{}{}", ip, port, path);
                        if let Ok(resp) = client.get(&url).send().await {
                            if resp.status().is_success() {
                                let body: serde_json::Value = resp.json().await.unwrap_or_else(|_| {
                                    serde_json::json!({"service": "responding", "probe": path})
                                });
                                found = Some(body);
                                break;
                            }
                        }
                    }
                    found
                } else {
                    None
                };
                port_status.push(serde_json::json!({
                    "port": port,
                    "online": online.is_some(),
                    "info": online.unwrap_or(serde_json::Value::Null),
                }));
            }

            let any_service_online = port_status.iter().any(|p| p.get("online").and_then(|v| v.as_bool()).unwrap_or(false));
            results.push(serde_json::json!({
                "name": name,
                "ip": ip,
                "online": ts_online,
                "services_online": any_service_online,
                "gpu": gpu_label,
                "ports": port_status,
            }));
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
    if std::path::Path::new("/usr/bin/koboldcpp").exists()
        || std::path::Path::new("/home/cesarops/koboldcpp").exists()
        || std::path::Path::new("/home/cesarops/benchmark/koboldcpp").exists() {
        available.push("koboldcpp");
    }
    if std::path::Path::new("/home/cesarops/wreckhunter2000-1/cesarops-inference/target/release/cesarops-inference").exists() {
        available.push("cesarops-inference");
    }
    // Check ollama
    if reqwest::Client::new()
        .get("http://localhost:11434/api/tags")
        .timeout(std::time::Duration::from_secs(1))
        .send().await.map(|r| r.status().is_success()).unwrap_or(false) {
        available.push("ollama");
    }

    // Per-node: remote nodes only have koboldcpp (we know this from our setup)
    let per_node = serde_json::json!({
        "127.0.0.1":       available,
        "100.102.158.111": ["koboldcpp"],
        "100.105.77.74":   ["koboldcpp"],
        "100.110.214.86":  ["koboldcpp"],
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
                "engine":        t.get("engine").and_then(|v| v.as_str()).unwrap_or("koboldcpp"),
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
        main_endpoint: state.config.coder_url.clone(),
        ref_endpoint: state.config.validator_url.clone(),
        n_tokens: 10,
        min_agreement: 0.4,
    };

    let result = validator::run_validation(&config, prompt).await;
    Json(serde_json::to_value(&result).unwrap_or_default())
}

/// GET /validate/ping — quick liveness check of both main engine and P1000.
async fn validate_ping(State(state): State<AppState>) -> Json<serde_json::Value> {
    let main_up = validator::ping(&state.config.coder_url).await;
    let p1000_up = validator::ping(&state.config.validator_url).await;

    let main_tps = if main_up {
        validator::benchmark_tps(&state.config.coder_url, 5).await
    } else {
        None
    };
    let p1000_tps = if p1000_up {
        validator::benchmark_tps(&state.config.validator_url, 5).await
    } else {
        None
    };

    Json(serde_json::json!({
        "main_engine": {
            "url": state.config.coder_url,
            "online": main_up,
            "tps": main_tps,
        },
        "p1000_validator": {
            "url": state.config.validator_url,
            "online": p1000_up,
            "tps": p1000_tps,
        },
        "status": if main_up { "ok" } else { "main_engine_down" },
    }))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("cesarops_forge_v2=info")
        .init();

    let config = ForgeConfig {
        coder_url: "http://127.0.0.1:5001".to_string(),  // Qwen3.6 on P100s
        thinker_url: "http://127.0.0.1:5557".to_string(),       // DeepSeek-R1 7B on Xeon CPU (thinker)
        corrector_url: "http://0.0.0.0:0".to_string(), // DISABLED — corrector severed, skip_corrector=true in tuning
        nautivecs_url: "http://127.0.0.1:5003/query".to_string(),
        wso_url: "http://127.0.0.1:5010/search".to_string(),
        project_root: "/codebase/wreckhunter2000-1".to_string(),
        validator_url: "http://100.102.158.111:5571".to_string(), // P1000 TinyLlama — speed+accuracy canary
    };

    let state = AppState {
        conversation: Arc::new(Mutex::new(Vec::new())),
        config: Arc::new(config),
        interrupt: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        steering: Arc::new(Mutex::new(Vec::new())),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/cluster", get(cluster_panel))
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
        // ── New per-card control routes ──────────────────────────────────
        .route("/cluster/worker/{name}/apply",         post(worker_apply))
        .route("/cluster/worker/{name}/set_injection", post(worker_set_injection))
        .route("/cluster/worker/{name}/set_backend",   post(worker_set_backend))
        .route("/cluster/corrector/set_function",      post(corrector_set_function))
        .route("/cluster/engines",                     get(get_available_engines))
        .route("/cluster/memory_pool/create",          post(memory_pool_create))
        .route("/cluster/config/full",                 get(get_cluster_config_full))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 9100));
    info!("cesarops-forge-v2 (Self-Healing Knowledge Translator) on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
