use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use crate::AppState;
use crate::types::{SearchSegment, SegmentStatus};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchEvent {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub event_type: String,
    pub description: String,
    pub operator: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/dispatch/case", post(create_case))
        .route("/dispatch/case/:id", get(get_case))
}

async fn create_case(
    State(_state): State<AppState>,
    Json(_event): Json<DispatchEvent>,
) -> Json<Vec<SearchSegment>> {
    // Implementation placeholder
    Json(vec![])
}

async fn get_case(
    State(_state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<SearchSegment> {
    // Implementation placeholder
    Json(SearchSegment {
        id,
        polygon: vec![],
        assigned_team: None,
        pod_score: 0.0,
        status: SegmentStatus::Unsearched,
    })
}
