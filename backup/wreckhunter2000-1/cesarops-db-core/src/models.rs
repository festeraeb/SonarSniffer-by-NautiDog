use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct WreckRecord {
    pub id: Uuid,
    pub source_id: Option<String>,
    pub name: String,
    pub status: String,
    pub depth_m: Option<f64>,
    pub region: String,
    // Stored as raw Wait-Known-Binary (WKB) bytes from PostGIS. We'll use geozero to parse.
    pub location_wkb: Vec<u8>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: Option<DateTime<Utc>>,
}
