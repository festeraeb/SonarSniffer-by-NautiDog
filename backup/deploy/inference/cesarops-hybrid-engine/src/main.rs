//! CESAROPS Hybrid Engine — entry point.
//!
//! Dynamically flips between:
//!   - MoE LLM inference (Qwen3-Coder-30B-A3B) using Tier 1 GPU HBM2
//!   - Nauticus spatial compute (curvelet + dipole shaders) using the same GPU buffers
//!
//! Hardware target:
//!   - Dual Tesla P100 16GB HBM2 (32GB total) — no tensor cores, FP32/FP16 compute
//!   - 94GB DDR4 system RAM — Tier 2 MoE expert weight storage
//!   - Network supervisor node (cesarops2, 32GB RAM) — Tier 3 long-context KV history

mod cluster;
mod context_engine;
mod fdct_kernels;
mod geotransform;
mod llm_router;
mod scheduler;
mod spatial_engine;
mod tool_db;

/// Re-export allocation types from sovereign-cloud for the scheduler.
pub mod allocation {
    pub use crate::fdct_kernels::{CurveletProcessor, FdctError, P100GpuBackend, XeonCpuBackend};

    /// FP64 capability classification — prevents routing precision math to weak silicon.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Fp64Capability {
        /// 1:2 FP64:FP32 rate — Tesla P100, V100, A100, Xeon AVX-512.
        NativeFull,
        /// 1:32 FP64:FP32 rate — Consumer Pascal (1070, 1060), Quadro P1000.
        EmulatedSlow,
    }

    /// Hardware signature for routing decisions.
    #[derive(Debug, Clone)]
    pub struct DeviceHardwareSignature {
        pub name: String,
        pub has_fp64: bool,
        pub fp64_rate: Fp64Capability,
        pub native_avx512: bool,
    }

    impl DeviceHardwareSignature {
        pub fn evaluate(name: &str, is_cpu: bool) -> Self {
            if is_cpu {
                let has_avx512 = is_x86_feature_detected!("avx512f");
                return Self {
                    name: name.to_string(),
                    has_fp64: true,
                    fp64_rate: Fp64Capability::NativeFull,
                    native_avx512: has_avx512,
                };
            }
            let n = name.to_uppercase();
            let is_true_fp64 = n.contains("TESLA P100")
                || n.contains("TESLA V100")
                || n.contains("A100");
            Self {
                name: name.to_string(),
                has_fp64: is_true_fp64,
                fp64_rate: if is_true_fp64 { Fp64Capability::NativeFull } else { Fp64Capability::EmulatedSlow },
                native_avx512: false,
            }
        }
    }
}

use anyhow::Result;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tracing::info;

/// Global mode flag — true = LLM mode, false = Spatial mode.
/// Flipped atomically by the coordinator; no locks needed for reads.
pub static LLM_MODE_ACTIVE: AtomicBool = AtomicBool::new(false);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("╔══════════════════════════════════════════════════════════╗");
    info!("║  CESAROPS Hybrid Engine — Vulkan/wgpu Spatial + LLM     ║");
    info!("╚══════════════════════════════════════════════════════════╝");

    // Initialise the multi-GPU cluster coordinator (one wgpu Device per P100)
    let coordinator = Arc::new(
        cluster::HybridClusterCoordinator::init().await?
    );

    info!("Cluster: {} P100 node(s) initialised", coordinator.nodes.len());
    for (i, node) in coordinator.nodes.iter().enumerate() {
        let info = node.device.limits();
        info!("  GPU {}: max_buffer_size={}MB", i, info.max_buffer_size / (1024 * 1024));
    }

    // Supervisor node address — Tier 3 long-context storage (cesarops2)
    let supervisor_addr = std::env::var("SUPERVISOR_ADDR")
        .unwrap_or_else(|_| "100.102.158.111:9000".into());
    info!("Supervisor node: {}", supervisor_addr);

    // Determine startup mode from env
    let start_in_llm_mode = std::env::var("START_MODE")
        .map(|v| v.to_lowercase() == "llm")
        .unwrap_or(false);

    if start_in_llm_mode {
        info!("Starting in LLM mode (Qwen3-30B-A3B)");
        LLM_MODE_ACTIVE.store(true, Ordering::SeqCst);
        llm_router::run(coordinator.clone(), &supervisor_addr).await?;
    } else {
        info!("Starting in Spatial mode (Nauticus pipeline)");
        LLM_MODE_ACTIVE.store(false, Ordering::SeqCst);
        spatial_engine::run(coordinator.clone(), &supervisor_addr).await?;
    }

    Ok(())
}
