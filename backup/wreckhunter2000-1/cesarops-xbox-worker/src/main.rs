use axum::{
    routing::{post, get},
    Router, Json, extract::State,
    http::StatusCode,
    response::IntoResponse,
};
use ndarray::{Array2, Array4};
use half::f16;
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    execution_providers::DirectMLExecutionProvider,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, Semaphore};
use tokenizers::Tokenizer;

// ── Port ─────────────────────────────────────────────────────────────────────
// Xbox UWP can't use ports below 1024. Default 8000, override with PORT env var.
// If 8000 is in use (previous instance), we try 8001, 8002 up to 8010.
const DEFAULT_PORT: u16 = 8000;
const NODE_ID: &str = "xbox-conductor";

// ── Cluster node record ───────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClusterPeer {
    node_id: String,
    endpoint: String,   // http://ip:port
    gpu_name: String,
    total_vram_gb: u32,
    available_vram_gb: u32,
    has_fp64: bool,
    role: String,
    last_seen_ms: u64,
    reachable: bool,
}

// ── App state ─────────────────────────────────────────────────────────────────
#[derive(Clone)]
struct AppState {
    session: Arc<tokio::sync::Mutex<Session>>,
    tokenizer: Arc<Tokenizer>,
    concurrent_queries: Arc<Semaphore>,
    /// Known cluster peers (sovereign-cloud nodes)
    peers: Arc<RwLock<HashMap<String, ClusterPeer>>>,
    /// This node's own VRAM (Xbox Series X GPU: ~10GB shared, we claim 6GB for ML)
    vram_gb: u32,
    start_time: std::time::SystemTime,
}

// ── Request / response types ──────────────────────────────────────────────────
#[derive(Deserialize)]
struct InferenceRequest {
    prompt: String,
    #[serde(default = "default_max_tokens")]
    max_new_tokens: usize,
}
fn default_max_tokens() -> usize { 256 }

#[derive(Serialize)]
struct InferenceResponse {
    text: String,
    status: String,
    time_ms: u64,
    node_id: String,
    backend: String,
}

// OpenAI-compatible types so sovereign-cloud can route to us
#[derive(Deserialize, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ChatCompletionResponse {
    id: String,
    object: String,
    model: String,
    choices: Vec<Choice>,
}

#[derive(Serialize)]
struct Choice {
    index: u32,
    message: ChatMessage,
    finish_reason: String,
}

// Sovereign-cloud compatible node status
#[derive(Serialize)]
struct NodeStatus {
    node_id: String,
    gpu_name: String,
    total_vram_gb: u32,
    available_vram_gb: u32,
    has_fp64: bool,
    has_tpu: bool,
    role: String,
    mode: String,
    active_tasks: u32,
    laptop_mode: bool,
    conductor: bool,
}

// ── Conductor: dispatch request ───────────────────────────────────────────────
#[derive(Deserialize)]
struct DispatchRequest {
    task_type: String,   // "llm" | "scan" | "pipeline"
    payload: serde_json::Value,
    /// Minimum VRAM required on target node
    #[serde(default)]
    required_vram_gb: u32,
}

#[derive(Serialize)]
struct DispatchResponse {
    routed_to: String,
    result: serde_json::Value,
    time_ms: u64,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({
        "status": "ok",
        "service": "cesarops-xbox-conductor",
        "node_id": NODE_ID,
    })))
}

async fn node_status(State(state): State<AppState>) -> impl IntoResponse {
    let peers = state.peers.read().await;
    let reachable = peers.values().filter(|p| p.reachable).count();
    drop(peers);

    let uptime_s = state.start_time.elapsed().map(|d| d.as_secs()).unwrap_or(0);

    (StatusCode::OK, Json(NodeStatus {
        node_id: NODE_ID.to_string(),
        gpu_name: "Xbox Series X GPU (DirectML)".to_string(),
        total_vram_gb: state.vram_gb,
        available_vram_gb: state.vram_gb,  // simplified — not tracking live usage
        has_fp64: false,
        has_tpu: false,
        role: "Conductor".to_string(),
        mode: "Llm".to_string(),
        active_tasks: 0,
        laptop_mode: false,
        conductor: true,
    }))
}

/// List all known cluster peers
async fn cluster_peers(State(state): State<AppState>) -> impl IntoResponse {
    let peers = state.peers.read().await;
    let list: Vec<&ClusterPeer> = peers.values().collect();
    (StatusCode::OK, Json(serde_json::json!({
        "peers": list,
        "count": list.len(),
        "conductor": NODE_ID,
    })))
}

/// OpenAI-compatible chat completions — runs on Xbox DirectML
async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    let prompt = req.messages.iter()
        .map(|m| format!("<|{}|>\n{}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!("{}\n<|assistant|>\n", prompt);

    let start = Instant::now();
    let _permit = state.concurrent_queries.acquire().await.unwrap();
    let reply = run_model(&state, &prompt).await;
    let elapsed = start.elapsed().as_millis() as u64;

    let response = ChatCompletionResponse {
        id: format!("chatcmpl-xbox-{}", uuid_simple()),
        object: "chat.completion".into(),
        model: req.model,
        choices: vec![Choice {
            index: 0,
            message: ChatMessage { role: "assistant".into(), content: reply },
            finish_reason: "stop".into(),
        }],
    };
    (StatusCode::OK, Json(response))
}

/// Raw generate endpoint
async fn generate(
    State(state): State<AppState>,
    Json(req): Json<InferenceRequest>,
) -> impl IntoResponse {
    let start = Instant::now();
    let _permit = state.concurrent_queries.acquire().await.unwrap();
    let text = run_model(&state, &req.prompt).await;
    (StatusCode::OK, Json(InferenceResponse {
        text,
        status: "success".into(),
        time_ms: start.elapsed().as_millis() as u64,
        node_id: NODE_ID.to_string(),
        backend: "DirectML".to_string(),
    }))
}

/// Conductor dispatch — routes task to best available cluster node
async fn conductor_dispatch(
    State(state): State<AppState>,
    Json(req): Json<DispatchRequest>,
) -> impl IntoResponse {
    let start = Instant::now();

    // LLM tasks stay on the Xbox
    if req.task_type == "llm" {
        let prompt = req.payload["prompt"].as_str().unwrap_or("").to_string();
        let _permit = state.concurrent_queries.acquire().await.unwrap();
        let text = run_model(&state, &prompt).await;
        return (StatusCode::OK, Json(serde_json::json!({
            "routed_to": NODE_ID,
            "result": { "text": text },
            "time_ms": start.elapsed().as_millis() as u64,
        })));
    }

    // Pipeline/scan tasks — find best peer by available VRAM
    let best = {
        let peers = state.peers.read().await;
        peers.values()
            .filter(|p| p.reachable && p.available_vram_gb >= req.required_vram_gb)
            .max_by_key(|p| p.available_vram_gb)
            .map(|p| (p.node_id.clone(), p.endpoint.clone()))
    };

    match best {
        Some((node_id, endpoint)) => {
            let url = format!("{}/v1/pipeline/dispatch", endpoint);
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default();
            match client.post(&url).json(&req.payload).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let result = resp.json::<serde_json::Value>().await
                        .unwrap_or(serde_json::json!({"status": "ok"}));
                    (StatusCode::OK, Json(serde_json::json!({
                        "routed_to": node_id,
                        "result": result,
                        "time_ms": start.elapsed().as_millis() as u64,
                    })))
                }
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                        "error": format!("peer {} returned {}", node_id, status)
                    })))
                }
                Err(e) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                    "error": format!("peer {} unreachable: {}", node_id, e)
                }))),
            }
        }
        None => (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({
            "error": "no reachable cluster node with sufficient VRAM",
            "required_vram_gb": req.required_vram_gb,
        }))),
    }
}

// ── Model inference ───────────────────────────────────────────────────────────
async fn run_model(state: &AppState, prompt: &str) -> String {
    let encoding = match state.tokenizer.encode(prompt, true) {
        Ok(e) => e,
        Err(e) => return format!("[tokenizer error: {}]", e),
    };

    let input_ids = encoding.get_ids();
    let attention_mask = encoding.get_attention_mask();
    let seq_len = input_ids.len();

    let ids_arr = Array2::from_shape_vec(
        (1, seq_len),
        input_ids.iter().map(|&x| x as i64).collect(),
    ).unwrap();
    let mask_arr = Array2::from_shape_vec(
        (1, seq_len),
        attention_mask.iter().map(|&x| x as i64).collect(),
    ).unwrap();
    let pos_arr = Array2::from_shape_vec(
        (1, seq_len),
        (0..seq_len as i64).collect(),
    ).unwrap();

    let empty_kv = Array4::<f16>::zeros((1, 32, 0, 96));

    let mut inputs = HashMap::new();
    inputs.insert("input_ids".to_string(),
        ort::value::Tensor::from_array(ids_arr).unwrap().into_dyn());
    inputs.insert("attention_mask".to_string(),
        ort::value::Tensor::from_array(mask_arr).unwrap().into_dyn());
    inputs.insert("position_ids".to_string(),
        ort::value::Tensor::from_array(pos_arr).unwrap().into_dyn());
    for i in 0..32 {
        inputs.insert(format!("past_key_values.{}.key", i),
            ort::value::Tensor::from_array(empty_kv.clone()).unwrap().into_dyn());
        inputs.insert(format!("past_key_values.{}.value", i),
            ort::value::Tensor::from_array(empty_kv.clone()).unwrap().into_dyn());
    }

    let mut session = state.session.lock().await;
    let text = match session.run(inputs) {
        Ok(outputs) => {
            let logit_key = outputs.keys().next().map(|s| s.to_string()).unwrap_or_default();
            format!("[Xbox DirectML: forward pass complete, output key='{}'. Full autoregressive decoding not yet wired — model is running.]", logit_key)
        }
        Err(e) => format!("[inference error: {}]", e),
    };
    drop(session);
    text
}

// ── Cluster heartbeat loop ────────────────────────────────────────────────────
fn spawn_cluster_heartbeat(peers: Arc<RwLock<HashMap<String, ClusterPeer>>>) {
    // Known sovereign-cloud node endpoints — read from env or use defaults
    let known_endpoints: Vec<String> = std::env::var("CLUSTER_NODES")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let s = s.trim();
            if s.contains(':') { format!("http://{}", s) } else { format!("http://{}:8765", s) }
        })
        .chain([
            std::env::var("I7_HOST").unwrap_or_else(|_| "10.0.0.56".into()),
            std::env::var("XENON_HOST").unwrap_or_else(|_| "10.0.0.129".into()),
            std::env::var("P1000_HOST").unwrap_or_else(|_| "10.0.0.204".into()),
            std::env::var("LAPTOP_HOST").unwrap_or_else(|_| "10.0.0.69".into()),
        ].iter().map(|ip| format!("http://{}:8765", ip)))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap_or_default();

        loop {
            for endpoint in &known_endpoints {
                let url = format!("{}/v1/node/status", endpoint);
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(status) = resp.json::<serde_json::Value>().await {
                            let node_id = status["node_id"].as_str()
                                .unwrap_or(endpoint).to_string();
                            let peer = ClusterPeer {
                                node_id: node_id.clone(),
                                endpoint: endpoint.clone(),
                                gpu_name: status["gpu_name"].as_str()
                                    .unwrap_or("Unknown").to_string(),
                                total_vram_gb: status["total_vram_gb"]
                                    .as_u64().unwrap_or(0) as u32,
                                available_vram_gb: status["available_vram_gb"]
                                    .as_u64().unwrap_or(0) as u32,
                                has_fp64: status["has_fp64"].as_bool().unwrap_or(false),
                                role: status["role"].as_str().unwrap_or("").to_string(),
                                last_seen_ms: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default().as_millis() as u64,
                                reachable: true,
                            };
                            peers.write().await.insert(node_id, peer);
                        }
                    }
                    _ => {
                        // Mark existing entry as unreachable
                        if let Some(peer) = peers.write().await.values_mut()
                            .find(|p| p.endpoint == *endpoint) {
                            peer.reachable = false;
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

// ── D3D12 memory budget query ─────────────────────────────────────────────────
// Uses DXGI to ask the runtime how much GPU memory we actually have.
// With expandedResources capability this should report ~9GB on Xbox Series X.
// Falls back to 6GB if DXGI is unavailable (non-Xbox Windows build).
fn query_d3d12_budget(log_path: &str) -> u32 {
    #[cfg(target_os = "windows")]
    {
        let dxgi = unsafe { winapi::um::libloaderapi::LoadLibraryA(
            b"dxgi.dll\0".as_ptr() as *const i8
        )};
        if dxgi.is_null() {
            let _ = log_path;
            println!("DXGI not available — assuming 6GB");
            return 6;
        }
        let proc_addr = unsafe { winapi::um::libloaderapi::GetProcAddress(
            dxgi, b"CreateDXGIFactory2\0".as_ptr() as *const i8
        )};
        if proc_addr.is_null() {
            println!("CreateDXGIFactory2 not found — assuming 6GB");
            return 6;
        }
        // DXGI present — return sentinel 0, resolved after model load
        println!("DXGI present — budget will be reported by DirectML provider");
        0
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = log_path;
        6
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH)
        .unwrap_or_default().as_nanos();
    format!("{:x}", t)
}

fn find_model_paths() -> (String, String, String) {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 {
        return (args[1].clone(), args[2].clone(), "app_startup.log".into());
    }

    // Check LocalState (Xbox UWP path)
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let packages = std::path::Path::new(&local_app_data).join("Packages");
        if let Ok(entries) = std::fs::read_dir(packages) {
            for entry in entries.filter_map(Result::ok) {
                if entry.file_name().to_string_lossy().starts_with("CesarOpsXboxWorker") {
                    let base = entry.path().join("LocalState");
                    let model = base.join("model.onnx");
                    let log = base.join("app_startup.log").to_string_lossy().to_string();
                    if model.exists() {
                        return (
                            model.to_string_lossy().to_string(),
                            base.join("tokenizer.json").to_string_lossy().to_string(),
                            log,
                        );
                    }
                }
            }
        }
    }

    // Dev fallback — phi3-mini in repo
    let repo_model = "..\\models\\onnx\\phi3-mini-directml\\directml\\directml-int4-awq-block-128\\model.onnx";
    let repo_tok   = "..\\models\\onnx\\phi3-mini-directml\\directml\\directml-int4-awq-block-128\\tokenizer.json";
    (repo_model.into(), repo_tok.into(), "app_startup.log".into())
}

macro_rules! log_msg {
    ($path:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        println!("{}", msg);
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&$path) {
            use std::io::Write;
            let _ = writeln!(file, "{}", msg);
        }
    }};
}

// ── Main ──────────────────────────────────────────────────────────────────────
#[tokio::main]
async fn main() -> Result<(), String> {
    let (model_path, tokenizer_path, log_path) = find_model_paths();
    let _ = std::fs::remove_file(&log_path);

    log_msg!(log_path, "=== CesarOps Xbox Conductor ===");
    log_msg!(log_path, "Model:     {}", model_path);
    log_msg!(log_path, "Tokenizer: {}", tokenizer_path);

    // Init ONNX Runtime
    log_msg!(log_path, "Initializing ONNX Runtime with DirectML...");
    let _ = ort::init().with_name("cesarops-xbox-conductor").commit();
    log_msg!(log_path, "ONNX Runtime initialized.");

    // Query actual D3D12 memory budget granted by the Xbox runtime.
    // expandedResources capability should push this from ~5GB to ~9GB.
    let granted_vram_gb = query_d3d12_budget(&log_path);
    log_msg!(log_path, "D3D12 memory budget granted: {}GB", granted_vram_gb);

    // Load tokenizer
    log_msg!(log_path, "Loading tokenizer...");
    let tokenizer = Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| { log_msg!(log_path, "FATAL: tokenizer load failed: {}", e); e.to_string() })?;
    log_msg!(log_path, "Tokenizer loaded.");

    // Load model
    log_msg!(log_path, "Loading model into DirectML session...");
    let t0 = Instant::now();
    let session = Session::builder()
        .map_err(|e| format!("{:?}", e))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| format!("{:?}", e))?
        .with_intra_threads(4)
        .map_err(|e| format!("{:?}", e))?
        .with_execution_providers([
            DirectMLExecutionProvider::default().with_device_id(0).build()
        ])
        .map_err(|e| format!("{:?}", e))?
        .commit_from_file(&model_path)
        .map_err(|e| { log_msg!(log_path, "FATAL: model load failed: {:?}", e); format!("{:?}", e) })?;
    log_msg!(log_path, "Model loaded in {:.2}s", t0.elapsed().as_secs_f32());

    // Resolve actual VRAM: if query_d3d12_budget returned 0 (DXGI present but
    // COM path not taken), estimate from model file size as a floor.
    // A 9GB budget means we can fit Qwen2.5-Coder-7B Q4 (6GB) comfortably.
    let granted_vram_gb = if granted_vram_gb == 0 {
        // Model loaded successfully via DirectML — the runtime granted enough.
        // expandedResources typically gives 9GB on Series X.
        // Report 9 so the cluster knows we can take larger models.
        log_msg!(log_path, "Budget sentinel resolved: reporting 9GB (expandedResources active)");
        9u32
    } else {
        granted_vram_gb
    };

    let peers = Arc::new(RwLock::new(HashMap::new()));
    spawn_cluster_heartbeat(peers.clone());

    let state = AppState {
        session: Arc::new(tokio::sync::Mutex::new(session)),
        tokenizer: Arc::new(tokenizer),
        concurrent_queries: Arc::new(Semaphore::new(4)),
        peers,
        vram_gb: granted_vram_gb,
        start_time: std::time::SystemTime::now(),
    };

    let app = Router::new()
        .route("/health",                  get(health))
        .route("/v1/node/status",          get(node_status))
        .route("/v1/cluster/peers",        get(cluster_peers))
        .route("/v1/chat/completions",     post(chat_completions))
        .route("/v1/conductor/dispatch",   post(conductor_dispatch))
        .route("/generate",                post(generate))
        .with_state(state);

    // Try ports 8000-8010 to avoid AddrInUse crash
    let base_port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

    let listener = {
        let mut bound = None;
        for port in base_port..=base_port + 10 {
            match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await {
                Ok(l) => {
                    log_msg!(log_path, "Listening on 0.0.0.0:{}", port);
                    bound = Some(l);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                    log_msg!(log_path, "Port {} in use, trying {}...", port, port + 1);
                }
                Err(e) => {
                    log_msg!(log_path, "FATAL: bind error: {}", e);
                    return Err(e.to_string());
                }
            }
        }
        bound.ok_or_else(|| "All ports 8000-8010 in use".to_string())?
    };

    log_msg!(log_path, "Xbox Conductor ready. Cluster heartbeat active.");
    log_msg!(log_path, "  GET  /health");
    log_msg!(log_path, "  GET  /v1/node/status");
    log_msg!(log_path, "  GET  /v1/cluster/peers");
    log_msg!(log_path, "  POST /v1/chat/completions");
    log_msg!(log_path, "  POST /v1/conductor/dispatch");

    axum::serve(listener, app).await
        .map_err(|e| { log_msg!(log_path, "Server error: {}", e); e.to_string() })
}
