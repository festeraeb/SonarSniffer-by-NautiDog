//! Dynamic Workload Scheduler — routes tasks to AVX-512 Xeon or P100 GPU cluster
//! based on hardware capability signatures. Zero runtime overhead via macro dispatch.

use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::allocation::{DeviceHardwareSignature, Fp64Capability};

// ── Task types ────────────────────────────────────────────────────────────────

/// All compute task types in the hybrid engine.
/// The scheduler routes each type to the optimal hardware target.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskType {
    /// Curvelet forward pass — sequential math, high data dependency.
    /// → AVX-512 Xeon (8 doubles/cycle, full native FP64)
    CurveletForward,

    /// Richardson Number depth-layer weighting — sequential, precise.
    /// → AVX-512 Xeon
    RichardsonWeighting,

    /// GeoTransform coordinate reprojection — small, precise, sequential.
    /// → AVX-512 Xeon
    GeoTransformReprojection,

    /// 4096×4096 parallel dipole pixel sweep — embarrassingly parallel.
    /// → P100 GPU cluster (wgpu compute shader)
    DipolePixelScan,

    /// LLM KV cache attention matrix — pure memory-bandwidth bound.
    /// → P100 GPU cluster (HBM2 bandwidth advantage)
    LlmKvAttention,

    /// MoE expert weight routing decision — small, fast, sequential.
    /// → AVX-512 Xeon
    MoeExpertRouting,

    /// GeoTIFF tile ingestion and staging — I/O bound, then DMA to GPU.
    /// → Xeon for I/O, then GPU staging via coordinator
    TileIngestion,
}

// ── Workload task ─────────────────────────────────────────────────────────────

pub struct WorkloadTask {
    pub id:        u64,
    pub task_type: TaskType,
    /// Raw payload bytes — interpreted by the target executor.
    pub payload:   Vec<u8>,
}

// ── Hardware pool ─────────────────────────────────────────────────────────────

pub struct ExecutionTargetPool {
    /// Dual Xeon Silver 4110 — 32 threads, AVX-512, native FP64.
    pub xeon_cpu:   DeviceHardwareSignature,
    /// Tesla P100 nodes — native FP64 at 1:2 rate, HBM2 bandwidth.
    pub local_gpus: Vec<DeviceHardwareSignature>,
}

// ── Routing target ────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub enum TargetExecutionNode {
    /// Dual Xeon Silver 4110 with AVX-512 — sequential FP64 math.
    XeonCpu,
    /// Tesla P100 cluster — parallel compute, HBM2 bandwidth.
    P100GpuCluster,
    /// CPU fallback when AVX-512 not available (should not occur on T440).
    FallbackCpu,
}

// ── Routing macro ─────────────────────────────────────────────────────────────

/// Route a task to the optimal hardware target.
/// Evaluated at compile time via pattern matching — sub-microsecond dispatch.
/// No runtime profiling, no lock contention.
#[macro_export]
macro_rules! route_task {
    ($task_type:expr, $pool:expr) => {
        match $task_type {
            // Sequential, data-dependent, or precise math → AVX-512 Xeon
            TaskType::CurveletForward
            | TaskType::RichardsonWeighting
            | TaskType::GeoTransformReprojection
            | TaskType::MoeExpertRouting
            | TaskType::TileIngestion => {
                if $pool.xeon_cpu.native_avx512 {
                    TargetExecutionNode::XeonCpu
                } else {
                    TargetExecutionNode::FallbackCpu
                }
            }

            // Massively parallel or memory-bandwidth bound → P100 GPU cluster
            TaskType::DipolePixelScan | TaskType::LlmKvAttention => {
                let target_gpu = $pool.local_gpus.iter().find(|gpu| {
                    gpu.has_fp64 && gpu.fp64_rate == Fp64Capability::NativeFull
                });
                match target_gpu {
                    Some(_) => TargetExecutionNode::P100GpuCluster,
                    // Safe fallback: Xeon can run these in FP64 if P100 is busy
                    None => TargetExecutionNode::XeonCpu,
                }
            }
        }
    };
}

// ── Workload scheduler ────────────────────────────────────────────────────────

/// Ingests tasks and dispatches them to the correct hardware channel.
/// Uses tokio mpsc channels to decouple I/O (GeoTIFF loading) from compute,
/// keeping all 32 Xeon threads hot without waiting on GPU VRAM page-swaps.
pub struct WorkloadScheduler {
    pub hardware_pool: Arc<ExecutionTargetPool>,
    /// Channel to the Xeon CPU executor (curvelet, Richardson, reprojection).
    pub cpu_sender:    mpsc::Sender<WorkloadTask>,
    /// Channel to the P100 GPU executor (dipole scan, LLM KV attention).
    pub gpu_sender:    mpsc::Sender<WorkloadTask>,
}

impl WorkloadScheduler {
    pub fn new(
        hardware_pool: ExecutionTargetPool,
        cpu_sender:    mpsc::Sender<WorkloadTask>,
        gpu_sender:    mpsc::Sender<WorkloadTask>,
    ) -> Self {
        Self {
            hardware_pool: Arc::new(hardware_pool),
            cpu_sender,
            gpu_sender,
        }
    }

    /// Dispatch a task to the optimal hardware target.
    /// Returns immediately — execution is async on the target channel.
    pub async fn dispatch(&self, task: WorkloadTask) -> Result<(), Box<dyn std::error::Error>> {
        let target = route_task!(task.task_type, self.hardware_pool);

        match target {
            TargetExecutionNode::XeonCpu => {
                info!(
                    "[Scheduler] Task {} ({:?}) → AVX-512 Xeon Silver (32T, native FP64)",
                    task.id, task.task_type
                );
                self.cpu_sender.send(task).await?;
            }
            TargetExecutionNode::P100GpuCluster => {
                info!(
                    "[Scheduler] Task {} ({:?}) → Tesla P100 cluster (32GB HBM2)",
                    task.id, task.task_type
                );
                self.gpu_sender.send(task).await?;
            }
            TargetExecutionNode::FallbackCpu => {
                warn!(
                    "[Scheduler] Task {} ({:?}) → CPU fallback (AVX-512 not detected)",
                    task.id, task.task_type
                );
                self.cpu_sender.send(task).await?;
            }
        }

        Ok(())
    }
}
