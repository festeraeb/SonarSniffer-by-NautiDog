//! cesarops-node — lightweight DII node daemon.
//!
//! Runs on every GPU node. Registers with the forge, heartbeats, exposes
//! /spawn /stop /status /models /health for the orchestrator to control.

use axum::{extract::State, routing::{get, post}, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Clone, Deserialize)]
struct NodeConfig {
    forge_url: String,
    node_name: String,
    models_dir: String,
    #[serde(default = "default_port")]
    listen_port: u16,
    gpu_id: u32,
    backend: String,
    koboldcpp_path: String,
    #[serde(default = "default_hb")]
    heartbeat_interval_secs: u64,
    #[serde(default = "default_restarts")]
    max_restarts: u32,
}

fn default_port() -> u16 { 5500 }
fn default_hb() -> u64 { 10 }
fn default_restarts() -> u32 { 3 }

impl NodeConfig {
    fn load(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("read config {}: {}", path, e))?;
        toml::from_str(&content).map_err(|e| format!("parse config: {}", e))
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize, Debug)]
enum NodeState {
    Init,
    Registering,
    Idle,
    Loading,
    Serving,
    Error(String),
}

impl NodeState {
    fn as_str(&self) -> &str {
        match self {
            Self::Init => "init",
            Self::Registering => "registering",
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Serving => "serving",
            Self::Error(_) => "error",
        }
    }
}

#[derive(Clone, Serialize, Debug, Default)]
struct GpuInfo {
    name: String,
    vram_total_mb: u64,
    vram_used_mb: u64,
    temperature_c: u32,
    utilization_pct: u32,
}

struct AppInner {
    config: NodeConfig,
    state: RwLock<NodeState>,
    child: RwLock<Option<Child>>,
    active_model: RwLock<Option<String>>,
    active_port: RwLock<Option<u16>>,
    gpu_info: RwLock<GpuInfo>,
    all_gpus: RwLock<Vec<GpuEntry>>,
    start_time: Instant,
    restart_count: RwLock<u32>,
}

type AppState = Arc<AppInner>;

fn new_state(config: NodeConfig) -> AppState {
    Arc::new(AppInner {
        config,
        state: RwLock::new(NodeState::Init),
        child: RwLock::new(None),
        active_model: RwLock::new(None),
        active_port: RwLock::new(None),
        gpu_info: RwLock::new(GpuInfo::default()),
        all_gpus: RwLock::new(Vec::new()),
        start_time: Instant::now(),
        restart_count: RwLock::new(0),
    })
}

// ---------------------------------------------------------------------------
// GPU telemetry
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize, Debug, Default)]
struct GpuEntry {
    id: u32,
    name: String,
    vram_total_mb: u64,
    vram_used_mb: u64,
    temperature_c: u32,
    utilization_pct: u32,
}

async fn poll_gpu(state: &AppInner) {
    // Query ALL GPUs on this box, not just the configured one.
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,memory.total,memory.used,temperature.gpu,utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .await;

    let mut gpus = Vec::new();
    if let Ok(out) = output {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            for line in s.lines() {
                let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
                if parts.len() >= 6 {
                    gpus.push(GpuEntry {
                        id: parts[0].parse().unwrap_or(0),
                        name: parts[1].to_string(),
                        vram_total_mb: parts[2].parse().unwrap_or(0),
                        vram_used_mb: parts[3].parse().unwrap_or(0),
                        temperature_c: parts[4].parse().unwrap_or(0),
                        utilization_pct: parts[5].parse().unwrap_or(0),
                    });
                }
            }
        }
    }

    if gpus.is_empty() {
        gpus.push(GpuEntry {
            id: state.config.gpu_id,
            name: format!("{} (gpu {})", state.config.backend, state.config.gpu_id),
            ..Default::default()
        });
    }

    // Write the primary GPU (the one we manage) into the legacy single-gpu field,
    // and all GPUs into the multi-gpu field.
    let primary = gpus.iter()
        .find(|g| g.id == state.config.gpu_id)
        .cloned()
        .unwrap_or_else(|| gpus[0].clone());

    *state.gpu_info.write().await = GpuInfo {
        name: primary.name.clone(),
        vram_total_mb: primary.vram_total_mb,
        vram_used_mb: primary.vram_used_mb,
        temperature_c: primary.temperature_c,
        utilization_pct: primary.utilization_pct,
    };
    *state.all_gpus.write().await = gpus;
}

// ---------------------------------------------------------------------------
// Forge communication
// ---------------------------------------------------------------------------

async fn register(state: &AppInner) -> Result<(), String> {
    let gpu = state.gpu_info.read().await.clone();
    let all_gpus = state.all_gpus.read().await.clone();
    let models = scan_models(&state.config.models_dir);

    let gpus_json: Vec<serde_json::Value> = all_gpus.iter().map(|g| {
        serde_json::json!({
            "id": g.id,
            "name": g.name,
            "vram_mb": g.vram_total_mb,
        })
    }).collect();

    let payload = serde_json::json!({
        "node_id": state.config.node_name,
        "hardware": {
            "gpu": gpu.name,
            "vram_mb": gpu.vram_total_mb,
            "backend": state.config.backend,
            "all_gpus": gpus_json,
        },
        "available_models": models.iter().map(|m| &m.name).collect::<Vec<_>>(),
        "listen_port": state.config.listen_port,
    });
    let url = format!("{}/cluster/node/register", state.config.forge_url);
    reqwest::Client::new()
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("register: {}", e))?;
    Ok(())
}

async fn heartbeat(state: &AppInner) {
    let gpu = state.gpu_info.read().await.clone();
    let all_gpus = state.all_gpus.read().await.clone();
    let model = state.active_model.read().await.clone();
    let port = *state.active_port.read().await;
    let node_state = state.state.read().await.as_str().to_string();

    let gpus_json: Vec<serde_json::Value> = all_gpus.iter().map(|g| {
        serde_json::json!({
            "id": g.id,
            "name": g.name,
            "vram_used_mb": g.vram_used_mb,
            "vram_total_mb": g.vram_total_mb,
            "temp_c": g.temperature_c,
            "util_pct": g.utilization_pct,
        })
    }).collect();

    let payload = serde_json::json!({
        "node_id": state.config.node_name,
        "state": node_state,
        "model": model,
        "port": port,
        "gpu": {
            "vram_used_mb": gpu.vram_used_mb,
            "vram_total_mb": gpu.vram_total_mb,
            "temp_c": gpu.temperature_c,
            "util_pct": gpu.utilization_pct,
        },
        "all_gpus": gpus_json,
        "queue_depth": 0,
    });

    let url = format!("{}/cluster/node/heartbeat", state.config.forge_url);
    if let Err(e) = reqwest::Client::new().post(&url).json(&payload).send().await {
        warn!("heartbeat: {}", e);
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn get_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let gpu = state.gpu_info.read().await.clone();
    let model = state.active_model.read().await.clone();
    let port = *state.active_port.read().await;
    let node_state = state.state.read().await.as_str().to_string();
    Json(serde_json::json!({
        "state": node_state,
        "model": model,
        "port": port,
        "gpu": gpu,
        "uptime_secs": state.start_time.elapsed().as_secs(),
    }))
}

#[derive(Deserialize)]
struct SpawnReq {
    model_path: String,
    port: u16,
    #[serde(default = "default_layers")]
    gpu_layers: u32,
    #[serde(default = "default_ctx")]
    context_size: u32,
}
fn default_layers() -> u32 { 999 }
fn default_ctx() -> u32 { 8192 }

async fn spawn_handler(
    State(state): State<AppState>,
    Json(req): Json<SpawnReq>,
) -> Json<serde_json::Value> {
    // Reject if already serving.
    {
        let s = state.state.read().await;
        if matches!(*s, NodeState::Serving | NodeState::Loading) {
            let m = state.active_model.read().await.clone();
            return Json(serde_json::json!({
                "error": format!("already serving {:?}, call /stop first", m)
            }));
        }
    }

    *state.state.write().await = NodeState::Loading;

    // Build koboldcpp command with correct per-backend flag.
    let backend_flag = match state.config.backend.as_str() {
        "cuda" => "--usecuda".to_string(),
        "vulkan" => format!("--usevulkan {}", state.config.gpu_id),
        _ => "--usecpu".to_string(),
    };

    let mut cmd = Command::new(&state.config.koboldcpp_path);
    cmd.arg("--model").arg(&req.model_path)
        .arg("--port").arg(req.port.to_string())
        .args(backend_flag.split_whitespace())
        .arg("--gpulayers").arg(req.gpu_layers.to_string())
        .arg("--contextsize").arg(req.context_size.to_string())
        .arg("--quiet")
        .arg("--maingpu").arg(state.config.gpu_id.to_string());

    let child = match cmd.kill_on_drop(true).spawn() {
        Ok(c) => c,
        Err(e) => {
            *state.state.write().await = NodeState::Error(e.to_string());
            return Json(serde_json::json!({"error": format!("spawn: {}", e)}));
        }
    };

    *state.child.write().await = Some(child);
    *state.active_model.write().await = Some(req.model_path.clone());
    *state.active_port.write().await = Some(req.port);
    *state.restart_count.write().await = 0;

    // Poll for port binding (up to 120s).
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    for _ in 0..120 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let url = format!("http://127.0.0.1:{}/api/v1/model", req.port);
        if let Ok(r) = client.get(&url).send().await {
            if r.status().is_success() {
                *state.state.write().await = NodeState::Serving;
                info!("spawn: {} serving on port {}", req.model_path, req.port);
                return Json(serde_json::json!({"status": "ok", "model": req.model_path, "port": req.port}));
            }
        }
    }

    // Timeout — kill child.
    if let Some(mut c) = state.child.write().await.take() {
        let _ = c.kill().await;
    }
    *state.state.write().await = NodeState::Error("load timeout (120s)".to_string());
    Json(serde_json::json!({"error": "model load timeout after 120s"}))
}

async fn stop_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let model = state.active_model.read().await.clone().unwrap_or_default();
    if let Some(mut c) = state.child.write().await.take() {
        let _ = c.kill().await;
    }
    *state.state.write().await = NodeState::Idle;
    *state.active_model.write().await = None;
    *state.active_port.write().await = None;
    info!("stop: killed {}", model);
    Json(serde_json::json!({"stopped": model}))
}

#[derive(Serialize)]
struct ModelEntry { name: String, size_gb: f64 }

fn scan_models(dir: &str) -> Vec<ModelEntry> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "gguf") {
            if let Ok(meta) = std::fs::metadata(&path) {
                out.push(ModelEntry {
                    name: path.file_name().unwrap().to_string_lossy().to_string(),
                    size_gb: meta.len() as f64 / (1024.0 * 1024.0 * 1024.0),
                });
            }
        }
    }
    out
}

async fn models_handler(State(state): State<AppState>) -> Json<Vec<ModelEntry>> {
    Json(scan_models(&state.config.models_dir))
}

async fn health_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let gpu = state.gpu_info.read().await.clone();
    let node_state = state.state.read().await.as_str().to_string();
    let disk_ok = std::fs::metadata(&state.config.models_dir).is_ok();
    let child_alive = state.child.read().await.is_some();

    Json(serde_json::json!({
        "status": if gpu.vram_total_mb > 0 && disk_ok { "ok" } else { "degraded" },
        "state": node_state,
        "gpu_accessible": gpu.vram_total_mb > 0,
        "gpu_temp_c": gpu.temperature_c,
        "disk_ok": disk_ok,
        "child_alive": child_alive,
        "uptime_secs": state.start_time.elapsed().as_secs(),
    }))
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config_path = std::env::args().nth(1).unwrap_or_else(|| "cesarops-node.toml".to_string());
    let config = match NodeConfig::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("FATAL: {}", e);
            std::process::exit(1);
        }
    };

    let port = config.listen_port;
    let state = new_state(config);

    // Initial GPU poll.
    poll_gpu(&state).await;

    // Register with forge (best-effort).
    *state.state.write().await = NodeState::Registering;
    match register(&state).await {
        Ok(_) => {
            info!("registered with forge");
            *state.state.write().await = NodeState::Idle;
        }
        Err(e) => {
            warn!("registration failed (will retry via heartbeat): {}", e);
            *state.state.write().await = NodeState::Idle;
        }
    }

    // Background: heartbeat loop.
    let hb_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(hb_state.config.heartbeat_interval_secs));
        loop {
            interval.tick().await;
            heartbeat(&hb_state).await;
        }
    });

    // Background: GPU poll loop.
    let gpu_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            poll_gpu(&gpu_state).await;
        }
    });

    // Background: child crash monitor.
    let mon_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let mut child_guard = mon_state.child.write().await;
            if let Some(ref mut c) = *child_guard {
                match c.try_wait() {
                    Ok(Some(status)) if !status.success() => {
                        warn!("child crashed (exit {})", status);
                        *child_guard = None;
                        drop(child_guard);

                        let mut rc = mon_state.restart_count.write().await;
                        if *rc < mon_state.config.max_restarts {
                            *rc += 1;
                            warn!("will allow manual restart ({}/{})", *rc, mon_state.config.max_restarts);
                            *mon_state.state.write().await = NodeState::Idle;
                        } else {
                            *mon_state.state.write().await =
                                NodeState::Error("max restarts reached".to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    // Routes.
    let app = Router::new()
        .route("/status", get(get_status))
        .route("/spawn", post(spawn_handler))
        .route("/stop", post(stop_handler))
        .route("/models", get(models_handler))
        .route("/health", get(health_handler))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    info!("cesarops-node listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
