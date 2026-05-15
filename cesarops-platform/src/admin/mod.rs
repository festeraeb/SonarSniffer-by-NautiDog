use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use crate::AppState;
use crate::types::{TeamMember, ResponderStatus};

/// API routes for the Admin module.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/members", post(create_member).get(list_members))
        .route(
            "/members/:id/status",
            put(update_member_status),
        )
        .route(
            "/members/:id",
            delete(delete_member),
        )
}

// --- Request/Response DTOs ---

#[derive(Debug, Deserialize)]
pub struct CreateMemberRequest {
    pub name: String,
    pub role: crate::types::ResponderRole,
    pub callsign: String,
    pub status: crate::types::ResponderStatus,
}

#[derive(Debug, Deserialize)]
pub struct UpdateStatusRequest {
    pub status: ResponderStatus,
}

// --- Handlers ---

/// POST /admin/members
/// Creates a new team member in the AppState.
async fn create_member(
    State(state): State<AppState>,
    Json(payload): Json<CreateMemberRequest>,
) -> Result<(StatusCode, Json<TeamMember>), StatusCode> {
    let mut members = state.members.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let new_member = TeamMember {
        id: Uuid::new_v4(),
        name: payload.name,
        role: payload.role,
        callsign: payload.callsign,
        gps_position: None,
        status: payload.status,
    };

    members.push(new_member.clone());
    Ok((StatusCode::CREATED, Json(new_member)))
}

/// GET /admin/members
/// Returns a list of all team members.
async fn list_members(
    State(state): State<AppState>,
) -> Result<Json<Vec<TeamMember>>, StatusCode> {
    let members = state.members.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(members.clone()))
}

/// PUT /admin/members/:id/status
/// Updates the status of an existing team member.
async fn update_member_status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateStatusRequest>,
) -> Result<StatusCode, StatusCode> {
    let mut members = state.members.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let member = members
        .iter_mut()
        .find(|m| m.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;

    member.status = payload.status;
    Ok(StatusCode::OK)
}

/// DELETE /admin/members/:id
/// Removes a team member from the system.
async fn delete_member(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, StatusCode> {
    let mut members = state.members.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let initial_len = members.len();
    members.retain(|m| m.id != id);

    if members.len() == initial_len {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}
