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
    "Conversation cleared"
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
    info!("CESAROPS ACTIVATED — locking resources for SAR scanning");
    Json(serde_json::json!({"message": "CESAROPS Preset activated. Resources locked for scanning."}))
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

async fn start_worker(axum::extract::Path(idx): axum::extract::Path<usize>) -> Json<serde_json::Value> {
    // Read config, get worker details, spawn cesarops-inference process
    let config_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    
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

async fn stop_worker(axum::extract::Path(idx): axum::extract::Path<usize>) -> Json<serde_json::Value> {
    let config_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    
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

            // Check if Tailscale sees this peer as online
            let ts_online = if ip == "127.0.0.1" {
                true // local is always online
            } else {
                tailscale_peers.iter().any(|p| p.0 == ip && p.1)
            };

            let mut port_status = Vec::new();
            for port_val in ports {
                let port = port_val.as_integer().unwrap_or(0);
                let url = format!("http://{}:{}/health", ip, port);
                let online = if ts_online {
                    match client.get(&url).send().await {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                let body: serde_json::Value = resp.json().await.unwrap_or_default();
                                Some(body)
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    }
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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("cesarops_forge_v2=info")
        .init();

    let config = ForgeConfig {
        coder_url: "http://127.0.0.1:5001".to_string(),  // Qwen3.6 on P100s
        thinker_url: "http://127.0.0.1:5557".to_string(),       // DeepSeek-R1 7B on Xeon CPU (thinker)
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
        .route("/cluster", get(cluster_panel))
        .route("/health", get(health))
        .route("/send", post(send_message))
        .route("/clear", post(clear))
        .route("/monitor", get(monitor))
        .route("/cluster/config", get(get_cluster_config).post(save_cluster_config))
        .route("/cluster/models", get(list_available_models))
        .route("/cluster/preset/config", get(get_preset_config).post(save_preset_config))
        .route("/cluster/preset/activate", post(activate_preset))
        .route("/cluster/preset/deactivate", post(deactivate_preset))
        .route("/cluster/worker/{idx}/start", post(start_worker))
        .route("/cluster/worker/{idx}/stop", post(stop_worker))
        .route("/cluster/start-all", post(start_all_workers))
        .route("/cluster/stop-all", post(stop_all_workers))
        .route("/cluster/discover", get(discover_nodes))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 9100));
    info!("cesarops-forge-v2 (Self-Healing Knowledge Translator) on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
