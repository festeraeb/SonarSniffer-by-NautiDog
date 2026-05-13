use serde::{Serialize, Deserialize};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use thiserror::Error;

/// Precision modes supported by the grid
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Precision {
    FP16,
    FP32,
    FP64,
    INT8,
}

/// NVIDIA Architecture identification
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NvidiaArch {
    Pascal,
    Volta,
    Turing,
    Unknown(String),
}

/// Hardware node types in the grid
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hardware {
    Pascal,
    Turing,
    Volta,
    Xeon,
    TPU,
    RemoteNode,
    WebGpu,
}

/// Ban levels for node health
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BanLevel {
    KLine,
    GLine,
    ZLine,
}

/// Dispatch tier for task routing
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DispatchTier {
    Specialist,
    Adaptive,
    Preprocessor,
}

/// A task to be dispatched to a compute node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeTask {
    pub data_size: usize,
    pub precision: Precision,
    pub shader_path: String,
    pub params: Vec<u8>,
}

/// Tensor data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tensor {
    pub data: Vec<u8>,
    pub shape: Vec<usize>,
    pub dtype: Precision,
}

/// Profile of a specific compute device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    pub name: String,
    pub arch: NvidiaArch,
    pub sm_version: u32,
    pub vram_total_mb: u64,
    pub vram_free_mb: u64,
    pub tflops: f32,
    pub bandwidth_gbps: f32,
    #[serde(skip)]
    pub utilization: Arc<AtomicU64>,
    pub socket_id: Option<u32>,
    pub has_f16: bool,
}

/// Summary of a GPU node for discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuNode {
    pub name: String,
    pub arch: NvidiaArch,
    pub sm_version: u32,
    pub features: Vec<String>,
    pub vram_mb: u64,
    pub bandwidth_gbps: f32,
}

/// Errors returned by the warp-grid system
#[derive(Debug, Error)]
pub enum Error {
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Shader compilation failed: {0}")]
    ShaderCompileFailed(String),

    #[error("QUIC connection timeout")]
    QuicTimeout,

    #[error("Node is KLined: {0}")]
    KLined(String),

    #[error("VRAM exhausted")]
    VramExhausted,

    #[error("NUMA topology mismatch: {0}")]
    NumaMismatch(String),

    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Parse Error: {0}")]
    ParseError(String),
}
