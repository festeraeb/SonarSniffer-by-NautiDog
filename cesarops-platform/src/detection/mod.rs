use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Represents the priority level of a Search and Rescue operation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

/// Sources of data used by the CESARops pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataSource {
    /// Sentinel-1 SAR satellite imagery (GeoTIFF)
    Sar { path: PathBuf },
    /// Side-scan sonar survey data
    Sonar { path: PathBuf },
    /// General satellite imagery
    Satellite { path: PathBuf },
}

/// The input specification for a detection task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionJob {
    pub id: Uuid,
    /// Bounding box: (min_lat, min_lon, max_lat, max_lon)
    pub area: (f64, f64, f64, f64),
    pub sources: Vec<DataSource>,
    pub priority: Priority,
    pub created_at: DateTime<Utc>,
}

/// Classification of detected objects in the Great Lakes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum TargetClass {
    Wreck,
    Debris,
    Structure,
    Anomaly,
    Unknown,
}

/// A detected object with spatial and confidence metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionTarget {
    pub lat: f64,
    pub lon: f64,
    pub confidence: f32,
    pub classification: TargetClass,
    /// The specific source that triggered the detection
    pub source: String,
}

/// The final output of the detection pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    pub job_id: Uuid,
    pub targets: Vec<DetectionTarget>,
    pub processing_time: Duration,
    pub area_km2: f64,
    pub completed_at: DateTime<Utc>,
}

/// Executes the detection pipeline for a given job.
/// 
/// Currently implemented as a stub that returns an empty result.
/// In production, this would interface with the GPU cluster and 
/// process the Sentinel-1 and Sonar data.
pub fn run_detection(job: &DetectionJob) -> DetectionResult {
    let start = Instant::now();
    
    // Calculate area in km2 (simplified approximation for the stub)
    let lat_diff = (job.area.2 - job.area.0).abs();
    let lon_diff = (job.area.3 - job.area.1).abs();
    let area_km2 = (lat_diff * 111.0) * (lon_diff * 85.0); // Rough approximation

    // STUB: In a real implementation, the GPU processing logic would go here.
    // We simulate a short processing delay.
    let targets = Vec::new(); 
    
    let duration = start.elapsed();

    DetectionResult {
        job_id: job.id,
        targets,
        processing_time: duration,
        area_km2,
        completed_at: Utc::now(),
    }
}

/// Converts a DetectionResult into a GeoJSON string format.
/// 
/// This follows the RFC 7946 standard for GeoJSON.
pub fn export_geojson(result: &DetectionResult) -> String {
    let mut features = Vec::new();

    for target in &result.targets {
        let feature = serde_json::json!({
            "type": "Feature",
            "geometry": {
                "type": "Point",
                "coordinates": [target.lon, target.lat]
            },
            "properties": {
                "confidence": target.confidence,
                "classification": format!("{:?}", target.classification),
                "source": target.source,
                "job_id": result.job_id.to_string()
            }
        });
        features.push(feature);
    }

    let geojson = serde_json::json!({
        "type": "FeatureCollection",
        "metadata": {
            "job_id": result.job_id.to_string(),
            "area_km2": result.area_km2,
            "processing_time_ms": result.processing_time.as_millis(),
            "completed_at": result.completed_at.to_rfc3339()
        },
        "features": features
    });

    geojson.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_job_creation_and_empty_run() {
        let job = DetectionJob {
            id: Uuid::new_v4(),
            area: (45.0, -82.0, 46.0, -81.0),
            sources: vec![
                DataSource::Sar { path: PathBuf::from("/data/sentinel_1.tif") },
                DataSource::Sonar { path: PathBuf::from("/data/sonar_survey.dat") }
            ],
            priority: Priority::High,
            created_at: Utc::now(),
        };

        let result = run_detection(&job);
        assert_eq!(result.job_id, job.id);
        assert!(result.targets.is_empty());
        assert!(result.area_km2 > 0.0);
    }

    #[test]
    fn test_geojson_export_empty() {
        let result = DetectionResult {
            job_id: Uuid::new_v4(),
            targets: vec![],
            processing_time: Duration::from_secs(1),
            area_km2: 100.0,
            completed_at: Utc::now(),
        };

        let json_str = export_geojson(&result);
        assert!(json_str.contains("FeatureCollection"));
        assert!(json_str.contains("area_km2"));
    }
}
