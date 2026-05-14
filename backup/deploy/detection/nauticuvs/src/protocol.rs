use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskType {
    ScoutPass,
    SyntheticTiling,
    AnalystPass,
    TemporalStacking,
    CodeGeneration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
    pub id: String,
    pub task_type: TaskType,
    pub payload: serde_json::Value,
    pub required_vram_gb: u32,
    pub required_fp64: bool,
    pub requires_tpu: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapabilities {
    pub node_id: String,
    pub total_vram_gb: u32,
    pub available_vram_gb: u32,
    pub has_fp64: bool,
    pub has_tpu: bool,
    pub gpu_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeRole {
    Dispatcher,
    Scout,   // TPU or low-power GPU
    Analyst, // High-VRAM/FP64 GPU
    CoderReasoningLead, // 8GB+ GPU
    CoderExecutionWorker, // 6GB GPU
    SuperAgent, // 24GB+ GPU
    Idle,
}
