use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use std::sync::Arc;

use crate::engine::ThoughtEngine;
use crate::models::{TaskRequest, TaskResponse};

pub async fn create_task(
    State(engine): State<Arc<ThoughtEngine>>,
    Json(request): Json<TaskRequest>,
) -> Result<Json<TaskResponse>, (StatusCode, Json<String>)> {
    match engine.process_task(request).await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            tracing::error!("Task processing failed: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(format!("Task processing failed: {}", e)),
            ))
        }
    }
}

pub async fn get_task(
    State(engine): State<Arc<ThoughtEngine>>,
    Path(task_id): Path<String>,
) -> Result<Json<TaskResponse>, (StatusCode, Json<String>)> {
    let tasks = engine.tasks.read().await;

    match tasks.get(&task_id) {
        Some(response) => Ok(Json(response.clone())),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(format!("Task {} not found", task_id)),
        )),
    }
}

pub async fn health(
    State(engine): State<Arc<ThoughtEngine>>,
) -> Json<serde_json::Value> {
    let kobold_healthy = engine.kobold.health_check().await;

    Json(serde_json::json!({
        "status": "ok",
        "kobold": kobold_healthy
    }))
}
