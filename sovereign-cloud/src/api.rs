use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use image::GenericImageView;
use tower_http::cors::CorsLayer;
use nauticuvs::protocol::{TaskRequest, TaskType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::allocation::AllocationEngine;
use crate::pipeline::PipelineManager;

// --- Hot-swap mode state ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ComputeMode {
    Llm,
    Vulkan,
}

// --- Idle mode control ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IdleMode {
    #[default]
    None,
    Research,
    Scan,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanHit {
    pub lat: f64,
    pub lon: f64,
    pub confidence: f32,
    pub pass: String,
    pub cell_label: String,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridCell {
    pub lat: f64,
    pub lon: f64,
    pub label: String,
}

// --- Node state ---

pub struct NodeState {
    pub allocation: Arc<AllocationEngine>,
    pub pipeline: Arc<PipelineManager>,
    pub research: Option<Arc<crate::research_engine::ResearchEngine>>,
    pub mode: RwLock<ComputeMode>,
    pub idle_mode: RwLock<IdleMode>,
    /// Circular buffer of recent scan hits (capped at 200)
    pub scan_hits: RwLock<Vec<ScanHit>>,
    /// Cell currently being scanned by the idle scanner
    pub current_cell: RwLock<Option<GridCell>>,
    /// mDNS discovery for peer nodes
    pub discovery: Option<Arc<crate::discovery::NodeDiscovery>>,
    /// Laptop mode — node is paused, idle scanner stopped, minimal CPU/GPU use
    pub laptop_mode: RwLock<bool>,
    /// Shutdown signal — set to true to trigger graceful exit
    pub shutdown: Arc<tokio::sync::Notify>,
}

impl NodeState {
    pub fn new(
        allocation: Arc<AllocationEngine>,
        pipeline: Arc<PipelineManager>,
        discovery: Option<Arc<crate::discovery::NodeDiscovery>>,
        research: Option<Arc<crate::research_engine::ResearchEngine>>,
    ) -> Arc<Self> {
        // LAPTOP_MODE env var starts the node paused with idle=None
        let laptop_mode = std::env::var("LAPTOP_MODE").map(|v| v == "1" || v.to_lowercase() == "true").unwrap_or(false);
        let initial_idle = if laptop_mode { IdleMode::None } else { IdleMode::Both };
        if laptop_mode {
            tracing::info!("LAPTOP_MODE=1 — starting with idle=None, background scanning disabled");
        }
        Arc::new(Self {
            allocation,
            pipeline,
            research,
            mode: RwLock::new(ComputeMode::Llm),
            idle_mode: RwLock::new(initial_idle),
            scan_hits: RwLock::new(Vec::new()),
            current_cell: RwLock::new(None),
            discovery,
            laptop_mode: RwLock::new(laptop_mode),
            shutdown: Arc::new(tokio::sync::Notify::new()),
        })
    }

    /// Push a new hit, dropping oldest when buffer exceeds 200.
    pub async fn push_hit(&self, hit: ScanHit) {
        let mut hits = self.scan_hits.write().await;
        hits.push(hit);
        if hits.len() > 200 {
            hits.remove(0);
        }
    }

    /// Hot-swap: flush current mode and initialize the new one.
    pub async fn swap_mode(&self, new_mode: ComputeMode) {
        let mut mode = self.mode.write().await;
        if *mode == new_mode {
            return;
        }
        info!("Hot-swap: {:?} → {:?}", *mode, new_mode);
        match new_mode {
            ComputeMode::Vulkan => {
                // LLM weights flushed — Vulkan pipeline takes the VRAM
                info!("Hot-swap: flushing LLM weights, initializing Vulkan compute pipeline");
            }
            ComputeMode::Llm => {
                info!("Hot-swap: releasing Vulkan pipeline, loading LLM weights");
            }
        }
        *mode = new_mode;
    }
}

// --- OpenAI-compatible types ---

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub model: String,
    pub choices: Vec<Choice>,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: String,
}

// --- Request / response types for direct pipeline dispatch ---

#[derive(Debug, Serialize)]
pub struct NodeStatus {
    pub node_id: String,
    pub gpu_name: String,
    pub total_vram_gb: u32,
    pub available_vram_gb: u32,
    pub has_fp64: bool,
    pub has_tpu: bool,
    pub role: String,
    pub mode: String,
    pub active_tasks: u32,
    pub laptop_mode: bool,
}

// --- Router ---

pub fn router(state: Arc<NodeState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/pipeline/dispatch", post(pipeline_dispatch))
        .route("/v1/pipeline/run", post(pipeline_run_full))
        .route("/v1/tpu/infer", post(tpu_infer))
        .route("/v1/tpu/image", post(tpu_image_infer))
        .route("/v1/node/status", get(node_status))
        .route("/v1/node/mode", post(set_mode))
        .route("/v1/cluster/peers", get(cluster_peers))
        .route("/v1/cluster/status", get(cluster_status))
        .route("/v1/cluster/llm/recommend", get(cluster_llm_recommend))
        .route("/v1/idle/status", get(idle_status))
        .route("/v1/idle/mode", post(set_idle_mode))
        .route("/v1/research/findings", get(research_findings))
        .route("/v1/research/hypotheses", get(research_hypotheses))
        .route("/v1/laptop/on", post(laptop_mode_on))
        .route("/v1/laptop/off", post(laptop_mode_off))
        .route("/v1/node/shutdown", post(node_shutdown))
        .with_state(state)
        .layer(CorsLayer::permissive())
}

// --- Handlers ---

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok", "service": "sovereign-cloud" })))
}

/// OpenAI-compatible chat completions endpoint.
/// Routes to pipeline dispatch or Ollama (LLM mode) depending on message content.
async fn chat_completions(
    State(state): State<Arc<NodeState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    let model = req.model;
    let messages = req.messages;

    let task_type = {
        let user_msg = messages.iter().rev().find(|m| m.role == "user");
        let content = user_msg.map(|m| m.content.as_str()).unwrap_or("");
        infer_task_from_message(content)
    };

    let reply = if let Some(tt) = task_type {
        state.swap_mode(ComputeMode::Vulkan).await;
        let prompt = messages.iter().rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let task_req = TaskRequest {
            id: uuid::Uuid::new_v4().to_string(),
            task_type: tt,
            payload: serde_json::json!({ "prompt": prompt }),
            required_vram_gb: 0,
            required_fp64: false,
            requires_tpu: false,
        };
        match state.pipeline.dispatch(task_req).await {
            Ok(result) => format!(
                "Pipeline pass '{}' completed. Confidence: {:.2}. Output: {}",
                result.pass,
                result.anomaly_confidence,
                serde_json::to_string_pretty(&result.output).unwrap_or_default()
            ),
            Err(e) => format!("Pipeline error: {}", e),
        }
    } else {
        // LLM mode — forward to KoboldCpp (or any OpenAI-compatible backend)
        // Priority: LLM_BASE_URL → KOBOLD_BASE_URL → OLLAMA_BASE_URL → localhost:5001 (koboldcpp default)
        state.swap_mode(ComputeMode::Llm).await;
        let llm_base = std::env::var("LLM_BASE_URL")
            .or_else(|_| std::env::var("KOBOLD_BASE_URL"))
            .or_else(|_| std::env::var("OLLAMA_BASE_URL"))
            .unwrap_or_else(|_| "http://localhost:5001/v1".into());
        let url = format!("{}/chat/completions", llm_base.trim_end_matches('/'));
        let payload = serde_json::json!({ "model": model.clone(), "messages": messages });
        let client = reqwest::Client::new();
        match client.post(&url).json(&payload).send().await {
            Ok(resp) => {
                resp.json::<serde_json::Value>().await
                    .ok()
                    .and_then(|b| b["choices"][0]["message"]["content"].as_str().map(str::to_string))
                    .unwrap_or_else(|| "[LLM response parse failed]".into())
            }
            Err(e) => format!("[LLM backend unavailable at {}: {}]", url, e),
        }
    };

    let response = ChatCompletionResponse {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        object: "chat.completion".into(),
        model,
        choices: vec![Choice {
            index: 0,
            message: ChatMessage { role: "assistant".into(), content: reply },
            finish_reason: "stop".into(),
        }],
    };

    (StatusCode::OK, Json(response))
}

/// Direct pipeline task dispatch endpoint.
async fn pipeline_dispatch(
    State(state): State<Arc<NodeState>>,
    Json(req): Json<TaskRequest>,
) -> impl IntoResponse {
    state.swap_mode(ComputeMode::Vulkan).await;
    match state.pipeline.dispatch(req).await {
        Ok(result) => (StatusCode::OK, Json(serde_json::to_value(result).unwrap())),
        Err(e) => {
            warn!("Pipeline dispatch error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() })))
        }
    }
}

/// Fire the full 4-pass pipeline over a geographic region.
#[derive(Debug, Deserialize)]
struct FullPipelineRequest {
    lat: f64,
    lon: f64,
    bands: Vec<f32>,
}

async fn pipeline_run_full(
    State(state): State<Arc<NodeState>>,
    Json(req): Json<FullPipelineRequest>,
) -> impl IntoResponse {
    state.swap_mode(ComputeMode::Vulkan).await;
    match state.pipeline.fire_full_pipeline(req.lat, req.lon, req.bands).await {
        Ok(results) => (StatusCode::OK, Json(serde_json::to_value(results).unwrap())),
        Err(e) => {
            warn!("Full pipeline error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() })))
        }
    }
}

/// TPU inference endpoint — accepts spectral bands, runs scout pass via Coral TPU.
/// Called remotely by nodes that lack a TPU (e.g. XENON → i7).
/// Set TPU_SERVER_URL=http://<i7>:8765 on any node that should offload here.
async fn tpu_infer(
    State(state): State<Arc<NodeState>>,
    Json(req): Json<FullPipelineRequest>,
) -> impl IntoResponse {
    let has_tpu = state.allocation.capabilities.read().await.has_tpu;
    if !has_tpu {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "no Coral TPU on this node" })),
        );
    }

    let task_req = nauticuvs::protocol::TaskRequest {
        id: uuid::Uuid::new_v4().to_string(),
        task_type: nauticuvs::protocol::TaskType::ScoutPass,
        payload: serde_json::json!({ "lat": req.lat, "lon": req.lon, "bands": req.bands }),
        required_vram_gb: 0,
        required_fp64: false,
        requires_tpu: true,
    };

    match state.pipeline.dispatch(task_req).await {
        Ok(result) => (StatusCode::OK, Json(serde_json::to_value(result).unwrap())),
        Err(e) => {
            warn!("TPU infer error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() })))
        }
    }
}

/// Image-based TPU inference — accepts a base64-encoded image tile for glint/shadow detection.
/// This is the primary endpoint for the satellite worker's surface pass pipeline.
#[derive(Debug, Deserialize)]
struct TpuImageRequest {
    image_base64: String,
    lat: Option<f64>,
    lon: Option<f64>,
    tile_id: Option<String>,
    pass_type: Option<String>, // "glint" | "shadow" | "both"
}

#[derive(Debug, Serialize)]
struct TpuImageDetection {
    pixel_row: u32,
    pixel_col: u32,
    confidence: f32,
    pass_type: String,
}

async fn tpu_image_infer(
    State(state): State<Arc<NodeState>>,
    Json(req): Json<TpuImageRequest>,
) -> impl IntoResponse {
    let has_tpu = state.allocation.capabilities.read().await.has_tpu;

    let pass_type = req.pass_type.as_deref().unwrap_or("both");

    // Decode image
    let img_bytes = match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &req.image_base64) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("invalid base64: {e}")}))),
    };

    let img = match image::ImageReader::with_format(std::io::Cursor::new(img_bytes), image::ImageFormat::Png).decode() {
        Ok(i) => i,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("invalid image: {e}")}))),
    };

    let (width, height) = image::GenericImageView::dimensions(&img);

    // Run glint/shadow detection (CPU heuristics — TPU path wired via edgetpu feature)
    let start = std::time::Instant::now();
    let mut detections: Vec<TpuImageDetection> = Vec::new();

    let gray = img.to_luma8();
    if pass_type == "glint" || pass_type == "both" {
        detections.extend(run_glint_detection(&gray));
    }
    if pass_type == "shadow" || pass_type == "both" {
        detections.extend(run_shadow_detection(&gray));
    }

    let took_ms = start.elapsed().as_millis() as u64;

    info!(
        "TPU image infer: {}x{} backend={} detections={} took_ms={}",
        width, height,
        if has_tpu { "edgetpu" } else { "cpu" },
        detections.len(),
        took_ms
    );

    (StatusCode::OK, Json(serde_json::json!({
        "detections": detections,
        "total": detections.len(),
        "took_ms": took_ms,
        "backend": if has_tpu { "edgetpu" } else { "cpu" },
        "image_size": [width, height],
    })))
}

fn run_glint_detection(img: &image::GrayImage) -> Vec<TpuImageDetection> {
    let (width, height) = img.dimensions();
    let pixels: Vec<u8> = img.pixels().map(|p| p[0]).collect();
    let mut sorted = pixels.clone();
    sorted.sort_unstable();
    let threshold_idx = (sorted.len() as f64 * 0.995) as usize;
    let threshold = sorted[threshold_idx.min(sorted.len() - 1)];
    if threshold == 0 { return Vec::new(); }

    let cell_size = 8u32;
    let mut cell_counts: std::collections::HashMap<(u32, u32), (u32, u64)> = std::collections::HashMap::new();
    for y in 0..height {
        for x in 0..width {
            let val = pixels[(y * width + x) as usize];
            if val >= threshold {
                let cell = (y / cell_size, x / cell_size);
                let entry = cell_counts.entry(cell).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += val as u64;
            }
        }
    }
    cell_counts.into_iter()
        .filter(|(_, (count, _))| *count >= 2)
        .map(|((cy, cx), (count, sum))| {
            let avg_brightness = (sum / count as u64) as f32 / 255.0;
            let density = count as f64 / (cell_size * cell_size) as f64;
            let confidence = (density * 5.0 * (avg_brightness as f64)).clamp(0.0, 1.0) as f32;
            TpuImageDetection { pixel_row: cy * cell_size + cell_size / 2, pixel_col: cx * cell_size + cell_size / 2, confidence, pass_type: "glint".into() }
        }).collect()
}

fn run_shadow_detection(img: &image::GrayImage) -> Vec<TpuImageDetection> {
    let (width, height) = img.dimensions();
    let get = |x: u32, y: u32| -> f32 {
        if x < width && y < height { img.get_pixel(x, y)[0] as f32 } else { 0.0 }
    };
    let mut gradients: Vec<f32> = Vec::new();
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let gx = -get(x-1,y-1) + get(x+1,y-1) - 2.0*get(x-1,y) + 2.0*get(x+1,y) - get(x-1,y+1) + get(x+1,y+1);
            let gy = -get(x-1,y-1) - 2.0*get(x,y-1) - get(x+1,y-1) + get(x-1,y+1) + 2.0*get(x,y+1) + get(x+1,y+1);
            gradients.push((gx*gx + gy*gy).sqrt());
        }
    }
    let mut sorted_grad = gradients.clone();
    sorted_grad.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let grad_threshold = sorted_grad.get((sorted_grad.len() as f64 * 0.90) as usize).copied().unwrap_or(50.0);

    let (inner_w, inner_h) = (width - 2, height - 2);
    let cell_size = 16u32;
    let mut cell_scores: std::collections::HashMap<(u32, u32), (u32, f64, f64)> = std::collections::HashMap::new();
    for y in 0..inner_h {
        for x in 0..inner_w {
            let grad = gradients[(y * inner_w + x) as usize];
            let brightness = get(x + 1, y + 1);
            if brightness < 60.0 && grad > grad_threshold {
                let entry = cell_scores.entry(((y+1)/cell_size, (x+1)/cell_size)).or_insert((0, 0.0, 0.0));
                entry.0 += 1; entry.1 += grad as f64; entry.2 += brightness as f64;
            }
        }
    }
    cell_scores.into_iter()
        .filter(|(_, (count, _, _))| *count >= 3)
        .map(|((cy, cx), (count, grad_sum, bright_sum))| {
            let avg_grad = (grad_sum / count as f64) as f32;
            let avg_bright = (bright_sum / count as f64) as f32 / 255.0;
            let gradient_score = (avg_grad / 200.0).clamp(0.0, 1.0);
            let darkness_score = (1.0 - avg_bright).clamp(0.0, 1.0);
            let confidence = (gradient_score * 0.6 + darkness_score * 0.4).clamp(0.0, 1.0);
            TpuImageDetection { pixel_row: cy * cell_size + cell_size / 2, pixel_col: cx * cell_size + cell_size / 2, confidence, pass_type: "shadow".into() }
        }).collect()
}

/// Node capabilities and current status.
async fn node_status(State(state): State<Arc<NodeState>>) -> impl IntoResponse {
    let caps = state.allocation.capabilities.read().await;
    let role = state.allocation.determine_role().await;
    let mode = state.mode.read().await;

    let status = NodeStatus {
        node_id: caps.node_id.clone(),
        gpu_name: caps.gpu_name.clone(),
        total_vram_gb: caps.total_vram_gb,
        available_vram_gb: caps.available_vram_gb,
        has_fp64: caps.has_fp64,
        has_tpu: caps.has_tpu,
        role: format!("{:?}", role),
        mode: format!("{:?}", *mode),
        active_tasks: state.pipeline.active_task_count().await,
        laptop_mode: *state.laptop_mode.read().await,
    };

    (StatusCode::OK, Json(status))
}

/// Full cluster status — self node + all discovered peers, each live-probed.
async fn cluster_status(State(state): State<Arc<NodeState>>) -> impl IntoResponse {
    // Self
    let caps = state.allocation.capabilities.read().await;
    let role = state.allocation.determine_role().await;
    let mode = state.mode.read().await;
    let mut nodes = vec![serde_json::json!({
        "node_id":           caps.node_id,
        "gpu_name":          caps.gpu_name,
        "total_vram_gb":     caps.total_vram_gb,
        "available_vram_gb": caps.available_vram_gb,
        "has_fp64":          caps.has_fp64,
        "has_tpu":           caps.has_tpu,
        "role":              format!("{:?}", role),
        "mode":              format!("{:?}", *mode),
        "active_tasks":      state.pipeline.active_task_count().await,
        "reachable":         true,
        "self":              true,
    })];
    drop(caps); drop(mode);

    // Peers
    if let Some(ref discovery) = state.discovery {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap_or_default();

        for peer in discovery.get_peers().await {
            // Try to get live status from the peer
            let port = 8765u16;
            let peer_host = peer.node_id.trim_end_matches(".local");
            let url = format!("http://{}:{}/v1/node/status", peer_host, port);
            let live = client.get(&url).send().await
                .ok()
                .map(|r| r.status().is_success())
                .unwrap_or(false);

            nodes.push(serde_json::json!({
                "node_id":           peer.node_id,
                "gpu_name":          peer.gpu_name,
                "total_vram_gb":     peer.total_vram_gb,
                "available_vram_gb": peer.available_vram_gb,
                "has_fp64":          peer.has_fp64,
                "has_tpu":           peer.has_tpu,
                "role":              "",
                "mode":              "",
                "active_tasks":      0,
                "reachable":         live,
                "self":              false,
            }));
        }
    }

    let total_vram: u32 = nodes.iter()
        .filter(|n| n["reachable"].as_bool().unwrap_or(false))
        .filter_map(|n| n["total_vram_gb"].as_u64())
        .sum::<u64>() as u32;
    let avail_vram: u32 = nodes.iter()
        .filter(|n| n["reachable"].as_bool().unwrap_or(false))
        .filter_map(|n| n["available_vram_gb"].as_u64())
        .sum::<u64>() as u32;

    (StatusCode::OK, Json(serde_json::json!({
        "nodes": nodes,
        "total_nodes": nodes.len(),
        "cluster_total_vram_gb": total_vram,
        "cluster_available_vram_gb": avail_vram,
    })))
}

/// ONNX model catalogue — maps VRAM requirement to best-fit model.
/// Returns the best model the cluster can fit in available RAM,
/// plus the full ranked list so the UI can offer a picker.
async fn cluster_llm_recommend(State(state): State<Arc<NodeState>>) -> impl IntoResponse {
    // Catalogue: (name, hf_repo, filename, vram_gb_required, param_b, quant)
    // Sorted best-first (largest that fits).
    let catalogue = vec![
        ("Qwen2.5-Coder-32B Q4",  "Qwen/Qwen2.5-Coder-32B-Instruct-GGUF",  "qwen2.5-coder-32b-instruct-q4_k_m.gguf",  20u32, 32u32, "Q4_K_M"),
        ("Qwen2.5-Coder-14B Q4",  "Qwen/Qwen2.5-Coder-14B-Instruct-GGUF",  "qwen2.5-coder-14b-instruct-q4_k_m.gguf",  10,    14,    "Q4_K_M"),
        ("Qwen2.5-Coder-7B Q4",   "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF",   "qwen2.5-coder-7b-instruct-q4_k_m.gguf",   6,     7,     "Q4_K_M"),
        ("DeepSeek-Coder-V2 Q4",  "bartowski/DeepSeek-Coder-V2-Lite-Instruct-GGUF", "DeepSeek-Coder-V2-Lite-Instruct-Q4_K_M.gguf", 10, 16, "Q4_K_M"),
        ("Qwen2.5-Coder-3B Q8",   "Qwen/Qwen2.5-Coder-3B-Instruct-GGUF",   "qwen2.5-coder-3b-instruct-q8_0.gguf",     4,     3,     "Q8_0"),
        ("Qwen2.5-Coder-1.5B Q8", "Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF", "qwen2.5-coder-1.5b-instruct-q8_0.gguf",   2,     2,     "Q8_0"),
    ];

    // Collect available VRAM across all reachable nodes
    let self_vram = state.allocation.capabilities.read().await.available_vram_gb;
    let mut max_node_vram = self_vram;
    if let Some(ref discovery) = state.discovery {
        for peer in discovery.get_peers().await {
            if peer.available_vram_gb > max_node_vram {
                max_node_vram = peer.available_vram_gb;
            }
        }
    }

    let models: Vec<serde_json::Value> = catalogue.iter().map(|(name, repo, file, vram, params, quant)| {
        let fits = *vram <= max_node_vram;
        serde_json::json!({
            "name":       name,
            "hf_repo":    repo,
            "filename":   file,
            "vram_gb":    vram,
            "params_b":   params,
            "quant":      quant,
            "fits":       fits,
            "download_url": format!("https://huggingface.co/{}/resolve/main/{}", repo, file),
        })
    }).collect();

    let best = models.iter().find(|m| m["fits"].as_bool().unwrap_or(false));

    (StatusCode::OK, Json(serde_json::json!({
        "max_node_vram_gb": max_node_vram,
        "models": models,
        "recommended": best,
    })))
}

/// Discover peer nodes via mDNS — returns all currently visible CESARops nodes in cluster.
async fn cluster_peers(State(state): State<Arc<NodeState>>) -> impl IntoResponse {
    let peers = if let Some(ref discovery) = state.discovery {
        discovery
            .get_peers()
            .await
            .into_iter()
            .map(|p| {
                serde_json::json!({
                    "node_id": p.node_id,
                    "gpu_name": p.gpu_name,
                    "total_vram_gb": p.total_vram_gb,
                    "available_vram_gb": p.available_vram_gb,
                    "has_fp64": p.has_fp64,
                    "has_tpu": p.has_tpu,
                })
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "peers": peers,
            "count": peers.len(),
        })),
    )
}

#[derive(Debug, Deserialize)]
struct SetModeRequest {
    mode: String,
}

/// Manually trigger a hot-swap.
async fn set_mode(
    State(state): State<Arc<NodeState>>,
    Json(req): Json<SetModeRequest>,
) -> impl IntoResponse {
    let new_mode = match req.mode.to_lowercase().as_str() {
        "vulkan" | "compute" => ComputeMode::Vulkan,
        "llm" | "inference" => ComputeMode::Llm,
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("unknown mode: {}", other) })),
            );
        }
    };
    state.swap_mode(new_mode).await;
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

// --- Idle mode handlers ---

/// Returns idle mode, current scan cell, and recent hits.
async fn idle_status(State(state): State<Arc<NodeState>>) -> impl IntoResponse {
    let mode   = state.idle_mode.read().await.clone();
    let hits   = state.scan_hits.read().await.clone();
    let cell   = state.current_cell.read().await.clone();
    let active = state.pipeline.active_task_count().await;
    (StatusCode::OK, Json(serde_json::json!({
        "mode":         mode,
        "current_cell": cell,
        "recent_hits":  hits,
        "pipeline_active_tasks": active,
    })))
}

#[derive(Debug, Deserialize)]
struct SetIdleModeRequest { mode: String }

/// Set the idle scanner mode (none / research / scan / both).
async fn set_idle_mode(
    State(state): State<Arc<NodeState>>,
    Json(req): Json<SetIdleModeRequest>,
) -> impl IntoResponse {
    let new_mode = match req.mode.to_lowercase().as_str() {
        "none"     => IdleMode::None,
        "research" => IdleMode::Research,
        "scan"     => IdleMode::Scan,
        "both"     => IdleMode::Both,
        other => return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("unknown idle mode: {}", other) })),
        ),
    };
    info!("IdleMode → {:?}", new_mode);
    *state.idle_mode.write().await = new_mode;
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

// --- Research handlers ---

async fn research_findings(State(state): State<Arc<NodeState>>) -> impl IntoResponse {
    if let Some(ref research) = state.research {
        let findings = research.recent_findings(100).await;
        (StatusCode::OK, Json(findings))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(vec![]))
    }
}

async fn research_hypotheses(State(state): State<Arc<NodeState>>) -> impl IntoResponse {
    if let Some(ref research) = state.research {
        let hypotheses = research.pending_hypotheses().await;
        (StatusCode::OK, Json(hypotheses))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(vec![]))
    }
}

// --- Laptop mode handlers ---

/// Laptop mode ON — pause all background work, set idle=None, free GPU.
/// The node stays on the network and can still answer API calls,
/// but does zero background scanning or research ingestion.
async fn laptop_mode_on(State(state): State<Arc<NodeState>>) -> impl IntoResponse {
    *state.laptop_mode.write().await = true;
    *state.idle_mode.write().await = IdleMode::None;
    *state.current_cell.write().await = None;
    info!("Laptop mode ON — background work paused");
    (StatusCode::OK, Json(serde_json::json!({ "laptop_mode": true, "idle_mode": "none" })))
}

/// Laptop mode OFF — resume cluster participation with idle=Both.
async fn laptop_mode_off(State(state): State<Arc<NodeState>>) -> impl IntoResponse {
    *state.laptop_mode.write().await = false;
    *state.idle_mode.write().await = IdleMode::Both;
    info!("Laptop mode OFF — resuming cluster participation");
    (StatusCode::OK, Json(serde_json::json!({ "laptop_mode": false, "idle_mode": "both" })))
}

/// Graceful shutdown — finishes active tasks then exits.
async fn node_shutdown(State(state): State<Arc<NodeState>>) -> impl IntoResponse {
    info!("Shutdown requested via API");
    // Signal the main loop to exit after a short delay
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        state.shutdown.notify_one();
    });
    (StatusCode::OK, Json(serde_json::json!({ "status": "shutting_down" })))
}

// --- Helpers ---

fn infer_task_from_message(content: &str) -> Option<TaskType> {
    let lower = content.to_lowercase();
    if lower.contains("scout") || lower.contains("glint") || lower.contains("hydrocarbon") {
        Some(TaskType::ScoutPass)
    } else if lower.contains("tile") || lower.contains("grid") || lower.contains("synthetic") {
        Some(TaskType::SyntheticTiling)
    } else if lower.contains("analyst") || lower.contains("curvelet") || lower.contains("bathymetry") || lower.contains("spectral") {
        Some(TaskType::AnalystPass)
    } else if lower.contains("stitch") || lower.contains("stack") || lower.contains("temporal") {
        Some(TaskType::TemporalStacking)
    } else if lower.contains("code") || lower.contains("rust") || lower.contains("shader") || lower.contains("wgsl") {
        Some(TaskType::CodeGeneration)
    } else {
        None
    }
}
