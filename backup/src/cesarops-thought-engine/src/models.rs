use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct TaskRequest {
    pub task_id: Option<String>,
    pub query: String,
    pub context_hint: Option<String>, // Optional hint for nautivecs search
}

#[derive(Serialize, Clone)]
pub struct TaskResponse {
    pub task_id: String,
    pub status: String, // 'processing', 'completed', 'failed', 'completed_with_warnings'
    pub result: Option<String>,
    pub error: Option<String>,
    pub steps: Vec<Step>,
}

#[derive(Serialize, Clone)]
pub struct Step {
    pub step: String,
    pub output: Option<String>,
}

// PlanSpec type (also used in clients.rs)
#[derive(Serialize, Deserialize, Clone)]
pub struct PlanSpec {
    pub sub_tasks: Vec<SubTask>,
    pub required_context: Vec<String>,
    pub output_format: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SubTask {
    pub description: String,
}
