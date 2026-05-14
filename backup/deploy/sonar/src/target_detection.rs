use serde::{Deserialize, Serialize};
use crate::garmin_rsd_parser::ParseResult;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DetectionMode {
    Off,
    Basic,
    Advanced,
}

impl DetectionMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "basic" => DetectionMode::Basic,
            "advanced" => DetectionMode::Advanced,
            _ => DetectionMode::Off,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionSettings {
    pub mode: DetectionMode,
    pub min_size: u32,
    pub max_size: u32,
    pub sensitivity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub ping_index: usize,
    pub sample_start: usize,
    pub sample_end: usize,
    pub intensity: f64,
    pub estimated_depth_m: Option<f64>,
    pub longitude: f64,
    pub latitude: f64,
    pub classification: String,
    pub size_class: String,
    pub blob_area: f64,
    pub width_m: f64,
    pub length_m: f64,
    pub avg_intensity: f64,
    pub confidence: f64,
    pub depth_m: f64,
    pub range_m: f64,
    pub channel: u32,
    pub channel_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionSummary {
    pub total: usize,
    pub total_detections: usize,
    pub fish_count: usize,
    pub structure_count: usize,
    pub debris_count: usize,
    pub wreck_count: usize,
    pub detections: Vec<Detection>,
}

pub fn detect_targets(_parse: &ParseResult, _settings: &DetectionSettings) -> DetectionSummary {
    // Placeholder — real detection logic would threshold intensity peaks
    DetectionSummary {
        total: 0,
        total_detections: 0,
        fish_count: 0,
        structure_count: 0,
        debris_count: 0,
        wreck_count: 0,
        detections: Vec::new(),
    }
}
