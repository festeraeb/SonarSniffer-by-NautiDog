use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CaseStatus { Active, Suspended, Closed, Cancelled }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Priority { Low, Medium, High, Critical }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SarCase {
    pub id: Uuid,
    pub status: CaseStatus,
    pub location: (f64, f64),
    pub created_at: DateTime<Utc>,
    pub priority: Priority,
    pub subject: Subject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subject {
    pub name: String,
    pub age: Option<u32>,
    pub last_seen: DateTime<Utc>,
    pub medical_conditions: Vec<String>,
    pub clothing_description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SegmentStatus { Unsearched, InProgress, Completed, Failed }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSegment {
    pub id: Uuid,
    pub polygon: Vec<(f64, f64)>,
    pub assigned_team: Option<Uuid>,
    pub pod_score: f64,
    pub status: SegmentStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponderRole { TeamLeader, FieldSearcher, K9Handler, DroneOperator, Medical, Communications }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponderStatus { Available, Deployed, OffDuty, Standby }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub id: Uuid,
    pub name: String,
    pub role: ResponderRole,
    pub callsign: String,
    pub gps_position: Option<(f64, f64)>,
    pub status: ResponderStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub description: String,
    pub operator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentUpdate {
    pub status: Option<SegmentStatus>,
    pub pod_score: Option<f64>,
    pub assigned_team: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpsUpdate {
    pub member_id: Uuid,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusUpdate {
    pub status: ResponderStatus,
}

