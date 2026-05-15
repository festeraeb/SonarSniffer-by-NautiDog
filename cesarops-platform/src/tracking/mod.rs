use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use crate::AppState;

/// Represents a single GPS coordinate point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpsPoint {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub latitude: f64,
    pub longitude: f64,
}

/// Payload for updating a member's location.
#[derive(Debug, Deserialize)]
pub struct UpdateLocationPayload {
    pub member_id: Uuid,
    pub latitude: f64,
    pub longitude: f64,
}

/// Returns all current tracked positions.
/// Returns a Map of MemberID -> List of recent GpsPoints.
async fn get_positions(
    State(state): State<AppState>,
) -> Result<Json<HashMap<Uuid, Vec<GpsPoint>>>, StatusCode> {
    let positions = state.positions.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(positions.clone()))
}

/// Updates the location for a specific member.
/// Appends the new point to the member's history in the state.
async fn update_location(
    State(state): State<AppState>,
    Json(payload): Json<UpdateLocationPayload>,
) -> Result<StatusCode, StatusCode> {
    let mut positions = state.positions.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let new_point = GpsPoint {
        timestamp: chrono::Utc::now(),
        latitude: payload.latitude,
        longitude: payload.longitude,
    };

    let history = positions.entry(payload.member_id).or_insert_with(Vec::new);
    history.push(new_point);

    // Optional: Limit history size to prevent unbounded memory growth
    if history.len() > 100 {
        history.remove(0);
    }

    Ok(StatusCode::OK)
}

/// Retrieves the GPS history for a specific member.
async fn get_history(
    Path(member_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Vec<GpsPoint>>, StatusCode> {
    let positions = state.positions.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    match positions.get(&member_id) {
        Some(history) => Ok(Json(history.clone())),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Defines the routing table for the tracking module.
/// This is intended to be composed into the main application router using .merge().
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/update", post(update_location))
        .route("/positions", get(get_positions))
        .route("/history/:member_id", get(get_history))
}
