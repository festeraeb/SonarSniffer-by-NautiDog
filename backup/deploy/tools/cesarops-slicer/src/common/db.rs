use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sled::{Db, Result};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SensorType {
    Optical,    // Sentinel-2
    Thermal,    // Landsat 8/9
    SAR,        // Sentinel-1 GRD
    Bathymetry, // SWOT / BAG
    Chemistry,  // PRISMA / DESIS
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReviewStatus {
    PendingLLM,
    ReviewedHighConfidence,
    ReviewedFalsePositive,
    ReviewedHumanInterventionRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyRecord {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub lat: f64,
    pub lon: f64,
    pub bbox: (f64, f64, f64, f64), // (min_lon, min_lat, max_lon, max_lat)
    pub sensor_type: SensorType,
    pub confidence_score: f32, // Based on initial physics heuristics
    pub imagery_path: Option<String>,
    pub metadata_json: String, // Additional arbitrary context for the LLM
    pub status: ReviewStatus,
}

impl AnomalyRecord {
    pub fn new(
        id: String,
        lat: f64,
        lon: f64,
        bbox: (f64, f64, f64, f64),
        sensor_type: SensorType,
        confidence_score: f32,
    ) -> Self {
        Self {
            id,
            timestamp: Utc::now(),
            lat,
            lon,
            bbox,
            sensor_type,
            confidence_score,
            imagery_path: None,
            metadata_json: "{}".to_string(),
            status: ReviewStatus::PendingLLM,
        }
    }
}

pub struct AnomalyQueue {
    db: Db,
}

impl AnomalyQueue {
    /// Opens or creates the Sled database used as the central anomaly queue.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    /// Pushes a new anomaly record (from a Producer) into the system.
    pub fn push_anomaly(&self, record: &AnomalyRecord) -> Result<()> {
        let id_bytes = record.id.as_bytes();
        let val_bytes = serde_json::to_vec(record).expect("Failed to serialize AnomalyRecord");

        self.db.insert(id_bytes, val_bytes)?;
        self.db.flush()?; // Ensure it hits disk
        Ok(())
    }

    /// Pulls all anomalies that are waiting for LLM assessment.
    pub fn pull_pending(&self) -> Result<Vec<AnomalyRecord>> {
        let mut pending = Vec::new();

        for item in self.db.iter() {
            let (_key, val) = item?;
            if let Ok(record) = serde_json::from_slice::<AnomalyRecord>(&val) {
                if matches!(record.status, ReviewStatus::PendingLLM) {
                    pending.push(record);
                }
            }
        }

        Ok(pending)
    }

    /// Updates the status of an existing record (used by the Consumer).
    pub fn update_status(&self, id: &str, new_status: ReviewStatus) -> Result<()> {
        if let Some(val) = self.db.get(id)? {
            if let Ok(mut record) = serde_json::from_slice::<AnomalyRecord>(&val) {
                record.status = new_status;
                let val_bytes =
                    serde_json::to_vec(&record).expect("Failed to serialize updated record");
                self.db.insert(id.as_bytes(), val_bytes)?;
                self.db.flush()?;
            }
        }
        Ok(())
    }
}
