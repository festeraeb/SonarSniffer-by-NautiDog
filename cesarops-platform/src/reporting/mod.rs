use crate::AppState;
use axum::{extract::State, routing::get, Router, Json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/reporting/stats", get(get_stats))
}

async fn get_stats(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cases = state.cases.lock().unwrap();
    let segments = state.segments.lock().unwrap();
    Json(serde_json::json!({
        "total_cases": cases.len(),
        "active_cases": cases.iter().filter(|c| matches!(c.status, crate::types::CaseStatus::Active)).count(),
        "total_segments": segments.len(),
    }))
}
