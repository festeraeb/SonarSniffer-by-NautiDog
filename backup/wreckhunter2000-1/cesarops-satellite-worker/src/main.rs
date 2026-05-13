//! CESAROPS Satellite Worker — Coral TPU Surface Pass Pipeline
//!
//! Breaks GeoTIFFs into tiles, runs fast glint/shadow detection on the Coral
//! Edge TPU (or CPU fallback), maps pixel detections to GPS coordinates, and
//! persists them to SQLite for temporal cluster matching.
//!
//! Strategy: Temporal Persistence
//! - Wrecks are stationary → features appear at same GPS coords across passes
//! - "Glint Pass": flags sun glint; persistent glint at a coordinate may indicate
//!   a shallow obstruction (mast, hull break)
//! - "Shadow Pass": flags anomalous geometric shapes (linear features, right angles)
//!
//! Endpoints:
//!   POST /satellite/scan        — Submit a GeoTIFF path for TPU surface pass
//!   POST /satellite/infer       — Submit a base64 image tile for inference
//!   GET  /satellite/hits        — Query recent detections with optional bbox
//!   GET  /satellite/clusters    — Query temporally clustered hotspots
//!   GET  /health                — Health check

mod tpu_infer;
mod tile_extractor;
mod detection_store;
mod cluster_matcher;

use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use detection_store::DetectionStore;
use tpu_infer::TpuEngine;

// ── State ────────────────────────────────────────────────────────────────────

struct WorkerState {
    tpu: Arc<TpuEngine>,
    store: Arc<DetectionStore>,
}

impl WorkerState {
    async fn new(db_path: &str) -> Result<Arc<Self>> {
        let tpu = Arc::new(TpuEngine::new().await);
        let store = Arc::new(DetectionStore::open(db_path).await?);
        Ok(Arc::new(Self { tpu, store }))
    }
}

// ── Request / Response Types ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ScanRequest {
    geotiff_path: String,
    tile_size: Option<u32>,
    run_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InferRequest {
    image_base64: String,
    lat: Option<f64>,
    lon: Option<f64>,
    tile_id: Option<String>,
    pass_type: Option<String>, // "glint" | "shadow" | "both"
}

#[derive(Debug, Serialize)]
struct DetectionResponse {
    pub detection_id: String,
    pub lat: f64,
    pub lon: f64,
    pub confidence: f32,
    pub pass_type: String,
    pub tile_id: Option<String>,
    pub run_id: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Serialize)]
struct ClusterResponse {
    pub cluster_id: String,
    pub center_lat: f64,
    pub center_lon: f64,
    pub detection_count: u32,
    pub pass_count: u32,
    pub max_confidence: f32,
    pub first_seen: i64,
    pub last_seen: i64,
}

#[derive(Debug, Deserialize)]
struct HitsQuery {
    lat_min: Option<f64>,
    lon_min: Option<f64>,
    lat_max: Option<f64>,
    lon_max: Option<f64>,
    pass_type: Option<String>,
    limit: Option<usize>,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({
        "status": "ok",
        "service": "satellite-worker",
    })))
}

/// Submit a GeoTIFF for full TPU surface pass scan.
async fn scan_geotiff(
    State(state): State<Arc<WorkerState>>,
    Json(req): Json<ScanRequest>,
) -> impl IntoResponse {
    let tile_size = req.tile_size.unwrap_or(640);
    let run_id = req.run_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    info!("Scan request: geotiff={} tile_size={} run_id={}", req.geotiff_path, tile_size, run_id);

    match tile_extractor::extract_and_infer(&state, &req.geotiff_path, tile_size, &run_id).await {
        Ok(count) => (StatusCode::OK, Json(serde_json::json!({
            "status": "complete",
            "run_id": run_id,
            "detections": count,
        }))),
        Err(e) => {
            warn!("Scan failed: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "status": "error",
                "error": e.to_string(),
            })))
        }
    }
}

/// Submit a single base64-encoded image tile for TPU inference.
async fn infer_tile(
    State(state): State<Arc<WorkerState>>,
    Json(req): Json<InferRequest>,
) -> impl IntoResponse {
    let img_bytes = match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &req.image_base64) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("invalid base64: {e}")}))),
    };

    let img = match image::ImageReader::with_format(
        std::io::Cursor::new(img_bytes),
        image::ImageFormat::Png,
    ).decode() {
        Ok(i) => i,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("invalid image: {e}")}))),
    };

    let pass_type = req.pass_type.as_deref().unwrap_or("both");
    let result = state.tpu.infer_image(&img, pass_type).await;

    // If coordinates provided, persist the detection
    let mut detections = Vec::new();
    if let (Some(lat), Some(lon)) = (req.lat, req.lon) {
        for det in &result.detections {
            let id = state.store.insert(&detection_store::DetectionRow {
                detection_id: uuid::Uuid::new_v4().to_string(),
                lat,
                lon,
                confidence: det.confidence,
                pass_type: det.pass_type.clone(),
                tile_id: req.tile_id.clone(),
                run_id: None,
                pixel_row: det.pixel_row,
                pixel_col: det.pixel_col,
                timestamp: chrono::Utc::now().timestamp(),
            }).await;
            detections.push(DetectionResponse {
                detection_id: id,
                lat,
                lon,
                confidence: det.confidence,
                pass_type: det.pass_type.clone(),
                tile_id: req.tile_id.clone(),
                run_id: None,
                timestamp: chrono::Utc::now().timestamp(),
            });
        }
    }

    (StatusCode::OK, Json(serde_json::json!({
        "detections": detections,
        "total": result.detections.len(),
        "took_ms": result.took_ms,
        "backend": result.backend,
    })))
}

/// Query recent detections with optional spatial filter.
async fn query_hits(
    State(state): State<Arc<WorkerState>>,
    Json(query): Json<HitsQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(200);
    let rows = state.store.query_detections(
        query.lat_min, query.lon_min, query.lat_max, query.lon_max,
        query.pass_type.as_deref(),
        limit,
    ).await;

    let hits: Vec<DetectionResponse> = rows.into_iter().map(|r| DetectionResponse {
        detection_id: r.detection_id,
        lat: r.lat,
        lon: r.lon,
        confidence: r.confidence,
        pass_type: r.pass_type,
        tile_id: r.tile_id,
        run_id: r.run_id,
        timestamp: r.timestamp,
    }).collect();

    (StatusCode::OK, Json(serde_json::json!({ "hits": hits, "count": hits.len() })))
}

/// Query temporally clustered hotspots (same location, multiple passes).
async fn query_clusters(
    State(state): State<Arc<WorkerState>>,
) -> impl IntoResponse {
    let clusters = cluster_matcher::find_clusters(&state.store).await;

    let resp: Vec<ClusterResponse> = clusters.into_iter().map(|c| ClusterResponse {
        cluster_id: c.cluster_id,
        center_lat: c.center_lat,
        center_lon: c.center_lon,
        detection_count: c.detection_count,
        pass_count: c.pass_count,
        max_confidence: c.max_confidence,
        first_seen: c.first_seen,
        last_seen: c.last_seen,
    }).collect();

    (StatusCode::OK, Json(serde_json::json!({ "clusters": resp, "count": resp.len() })))
}

// ── Router ───────────────────────────────────────────────────────────────────

fn router(state: Arc<WorkerState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/satellite/scan", post(scan_geotiff))
        .route("/satellite/infer", post(infer_tile))
        .route("/satellite/hits", post(query_hits))
        .route("/satellite/clusters", get(query_clusters))
        .with_state(state)
        .layer(CorsLayer::permissive())
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let db_path = std::env::var("SAT_DB_PATH")
        .unwrap_or_else(|_| "data/satellite_detections.db".to_string());
    let port: u16 = std::env::var("SAT_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8767);

    info!("=== CESAROPS Satellite Worker ===");
    info!("DB: {}  Port: {}", db_path, port);

    let state = WorkerState::new(&db_path).await?;
    let tpu_info = state.tpu.info().await;
    info!("TPU backend: {}", tpu_info);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!("Listening on http://0.0.0.0:{}", port);
    info!("Endpoints: POST /satellite/scan, POST /satellite/infer, POST /satellite/hits, GET /satellite/clusters");

    axum::serve(listener, router(state)).await?;

    Ok(())
}
