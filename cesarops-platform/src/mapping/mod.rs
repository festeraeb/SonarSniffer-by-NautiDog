use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::types::{SearchSegment, SegmentStatus};
use crate::AppState;

/// Request body for creating a new Search Segment
#[derive(Deserialize)]
pub struct CreateSegmentRequest {
    pub polygon: Vec<(f64, f64)>,
    pub pod_score: f64,
}

/// Request body for updating an existing Search Segment
#[derive(Deserialize)]
pub struct UpdateSegmentRequest {
    pub status: Option<SegmentStatus>,
    pub assigned_team: Option<Uuid>,
    pub pod_score: Option<f64>,
}

/// GeoJSON structure for mapping clients
#[derive(Serialize)]
pub struct GeoJsonFeature {
    #[serde(rename = "type")]
    pub feature_type: String,
    pub geometry: GeoJsonGeometry,
    pub properties: GeoJsonProperties,
}

#[derive(Serialize)]
pub struct GeoJsonGeometry {
    #[serde(rename = "type")]
    pub geometry_type: String,
    pub coordinates: Vec<Vec<Vec<f64>>>, // Polygon coordinates
}

#[derive(Serialize)]
pub struct GeoJsonProperties {
    pub id: Uuid,
    pub status: SegmentStatus,
    pub assigned_team: Option<Uuid>,
    pub pod_score: f64,
}

#[derive(Serialize)]
pub struct GeoJsonCollection {
    #[serde(rename = "type")]
    pub collection_type: String,
    pub features: Vec<GeoJsonFeature>,
}

/// Main router for the mapping module
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/segments", post(create_segment).get(list_segments))
        .route(
            "/segments/:id",
            put(update_segment),
        )
        .route("/segments/geojson", get(get_segments_geojson))
}

/// POST /mapping/segments
async fn create_segment(
    State(state): State<AppState>,
    Json(payload): Json<CreateSegmentRequest>,
) -> Result<(StatusCode, Json<SearchSegment>), StatusCode> {
    let new_segment = SearchSegment {
        id: Uuid::new_v4(),
        polygon: payload.polygon,
        assigned_team: None,
        pod_score: payload.pod_score,
        status: SegmentStatus::Unsearched,
    };

    let mut segments = state.segments.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    segments.push(new_segment.clone());

    Ok((StatusCode::CREATED, Json(new_segment)))
}

/// GET /mapping/segments
async fn list_segments(
    State(state): State<AppState>,
) -> Result<Json<Vec<SearchSegment>>, StatusCode> {
    let segments = state.segments.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(segments.clone()))
}

/// PUT /mapping/segments/:id
async fn update_segment(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateSegmentRequest>,
) -> Result<Json<SearchSegment>, StatusCode> {
    let mut segments = state.segments.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let segment = segments
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;

    if let Some(status) = payload.status {
        segment.status = status;
    }
    if let Some(team_id) = payload.assigned_team {
        segment.assigned_team = Some(team_id);
    }
    if let Some(score) = payload.pod_score {
        segment.pod_score = score;
    }

    Ok(Json(segment.clone()))
}

/// GET /mapping/segments/geojson
/// Returns segments in GeoJSON format for frontend map integration
async fn get_segments_geojson(
    State(state): State<AppState>,
) -> Result<Json<GeoJsonCollection>, StatusCode> {
    let segments = state.segments.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let features: Vec<GeoJsonFeature> = segments
        .iter()
        .map(|s| {
            // Convert Vec<(f64, f64)> to GeoJSON format: Vec<Vec<f64>>
            // Note: GeoJSON polygons must be closed (first point == last point)
            let mut coords: Vec<Vec<f64>> = s.polygon.iter().map(|(lat, lon)| vec![*lon, *lat]).collect();
            
            // Ensure polygon is closed for GeoJSON spec
            if let Some(first) = coords.first() {
                if coords.last() != Some(first) {
                    coords.push(first.clone());
                }
            }

            GeoJsonFeature {
                feature_type: "Feature".to_string(),
                geometry: GeoJsonGeometry {
                    geometry_type: "Polygon".to_string(),
                    // GeoJSON polygons are arrays of linear rings
                    coordinates: vec![coords],
                },
                properties: GeoJsonProperties {
                    id: s.id,
                    status: s.status.clone(),
                    assigned_team: s.assigned_team,
                    pod_score: s.pod_score,
                },
            }
        })
        .collect();

    Ok(Json(GeoJsonCollection {
        collection_type: "FeatureCollection".to_string(),
        features,
    }))
}
