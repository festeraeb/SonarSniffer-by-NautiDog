//! n8n-style task dispatcher — accepts scan jobs, dispatches tiles through the pipeline,
//! tracks progress, reports results.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use axum::{extract::State, extract::Path, response::Json};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::types::{ScanRequest, ScanJob, JobStatus, GeoTile, DetectionResult, MissionAction};
use crate::pipeline::TripleLockPipeline;

pub struct AppState {
    pub pipeline: TripleLockPipeline,
    pub jobs: RwLock<HashMap<String, ScanJob>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            pipeline: TripleLockPipeline::new(),
            jobs: RwLock::new(HashMap::new()),
        }
    }
}

/// Health check — reports worker status
pub async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    let (scout, validator, jitter) = state.pipeline.check_workers().await;
    Json(json!({
        "service": "cesarops-detection",
        "status": "ok",
        "workers": {
            "scout_1060": scout,
            "validator_p1000": validator,
            "jitter_tpu": jitter
        }
    }))
}

/// Submit a scan job — tiles get processed through the triple-lock pipeline
pub async fn submit_scan(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ScanRequest>,
) -> Json<Value> {
    let job_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    // Convert inputs to GeoTiles
    let tiles: Vec<GeoTile> = req.tiles.iter().enumerate().map(|(i, t)| {
        GeoTile {
            id: format!("{}_{}", req.region, i),
            lat: t.lat,
            lon: t.lon,
            image_b64: t.image_b64.clone(),
            bands: vec![],
            timestamp: now,
        }
    }).collect();

    let tile_count = tiles.len();

    let job = ScanJob {
        id: job_id.clone(),
        region: req.region.clone(),
        tiles: tiles.clone(),
        status: JobStatus::Running,
        results: Vec::new(),
        submitted_at: now,
        completed_at: None,
    };

    // Store job
    {
        let mut jobs = state.jobs.write().await;
        jobs.insert(job_id.clone(), job);
    }

    // Spawn background processing
    let state_clone = state.clone();
    let job_id_clone = job_id.clone();
    tokio::spawn(async move {
        tracing::info!("[{}] Processing {} tiles through triple-lock pipeline", job_id_clone, tile_count);

        let mut results: Vec<DetectionResult> = Vec::new();
        let mut confirmed_count = 0u32;

        for tile in &tiles {
            match state_clone.pipeline.process_tile(tile).await {
                Ok(result) => {
                    if result.action == MissionAction::Confirmed {
                        confirmed_count += 1;
                        tracing::info!(
                            "[{}] CONFIRMED: tile {} at ({}, {})",
                            job_id_clone, tile.id, tile.lat, tile.lon
                        );
                    }
                    results.push(result);
                }
                Err(e) => {
                    tracing::error!("[{}] Pipeline error on tile {}: {}", job_id_clone, tile.id, e);
                }
            }
        }

        // Update job status
        let mut jobs = state_clone.jobs.write().await;
        if let Some(job) = jobs.get_mut(&job_id_clone) {
            job.status = JobStatus::Completed;
            job.results = results;
            job.completed_at = Some(chrono::Utc::now().timestamp());
        }

        tracing::info!(
            "[{}] Scan complete. {}/{} tiles confirmed as detections.",
            job_id_clone, confirmed_count, tile_count
        );
    });

    Json(json!({
        "job_id": job_id,
        "status": "running",
        "tiles": tile_count,
        "message": format!("Processing {} tiles through triple-lock pipeline", tile_count)
    }))
}

/// Get scan job status and results
pub async fn get_scan_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<Value> {
    let jobs = state.jobs.read().await;
    match jobs.get(&id) {
        Some(job) => {
            let confirmed: Vec<&DetectionResult> = job.results.iter()
                .filter(|r| r.action == MissionAction::Confirmed)
                .collect();
            Json(json!({
                "job_id": job.id,
                "region": job.region,
                "status": job.status,
                "total_tiles": job.tiles.len(),
                "processed": job.results.len(),
                "confirmed_detections": confirmed.len(),
                "detections": confirmed,
                "submitted_at": job.submitted_at,
                "completed_at": job.completed_at,
            }))
        }
        None => Json(json!({"error": "Job not found"})),
    }
}

/// List all worker nodes and their status
pub async fn list_workers(State(state): State<Arc<AppState>>) -> Json<Value> {
    let (scout, validator, jitter) = state.pipeline.check_workers().await;
    let (scout_ep, val_ep, jitter_ep) = state.pipeline.worker_endpoints().await;
    Json(json!({
        "workers": [
            {"name": "Scout", "model": "Florence-2 / CPU sim", "host": scout_ep, "online": scout},
            {"name": "Validator", "model": "Moondream2 / CPU sim", "host": val_ep, "online": validator},
            {"name": "Jitter", "model": "TPU / CPU sim", "host": jitter_ep, "online": jitter},
        ]
    }))
}
