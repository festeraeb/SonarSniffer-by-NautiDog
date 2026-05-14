use serde::{Deserialize, Serialize};

/// A geographic tile to process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoTile {
    pub id: String,
    pub lat: f64,
    pub lon: f64,
    pub image_b64: String,
    pub bands: Vec<String>,
    pub timestamp: i64,
}

/// Scout report from 1060 (Florence-2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoutReport {
    pub tile_id: String,
    pub has_anomaly: bool,
    pub confidence: f32,
    pub anomaly_type: String,
    pub description: String,
    pub bbox: Option<Vec<f32>>,
    pub processing_ms: f64,
}

/// Validation report from P1000 (Moondream2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub tile_id: String,
    pub has_anomaly: bool,
    pub confidence: f32,
    pub shape_analysis: String,
    pub description: String,
    pub material_guess: String,
    pub processing_ms: f64,
}

/// Jitter signature from TPU VM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JitterSignature {
    pub material: String,
    pub certainty: f32,
    pub depth_estimate_ft: f32,
    pub jitter_frequency_hz: f32,
    pub thermal_delta_c: f32,
    pub classification: String,
}

/// Pipeline result after all locks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    pub tile_id: String,
    pub action: MissionAction,
    pub scout: Option<ScoutReport>,
    pub validator: Option<ValidationReport>,
    pub jitter: Option<JitterSignature>,
    pub overall_confidence: f32,
    pub timestamp: i64,
}

/// What to do with this detection
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MissionAction {
    Standby,
    Investigate,
    Confirmed,
    Alert,
}

/// A scan job submitted to the dispatcher
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanJob {
    pub id: String,
    pub region: String,
    pub tiles: Vec<GeoTile>,
    pub status: JobStatus,
    pub results: Vec<DetectionResult>,
    pub submitted_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

/// Request to submit a scan
#[derive(Debug, Deserialize)]
pub struct ScanRequest {
    pub region: String,
    pub tiles: Vec<TileInput>,
}

#[derive(Debug, Deserialize)]
pub struct TileInput {
    pub lat: f64,
    pub lon: f64,
    pub image_b64: String,
}
