//! LLM Router — manages MoE expert weight loading and KV cache on P100 HBM2.
//!
//! When in LLM mode, the P100s hold:
//!   - Active expert weights (3B active params from 30B total MoE)
//!   - KV cache ring buffer for attention heads
//!
//! Inactive experts are stored in Tier 2 (94GB host DDR4).
//! Long-horizon context is stored on Tier 3 (cesarops2 network node).

use anyhow::Result;
use std::sync::Arc;
use tracing::info;

use crate::cluster::HybridClusterCoordinator;

/// Run the LLM inference loop.
///
/// In production, this would:
/// 1. Load the active MoE expert weights from host RAM into P100 HBM2
/// 2. Accept token generation requests via the API
/// 3. Route expert activations across the two P100s
/// 4. Stream generated tokens back to the caller
///
/// For now, this delegates to KoboldCPP which handles the actual inference.
/// The hybrid engine's role is to manage the buffer swap when transitioning
/// between LLM and Spatial modes.
pub async fn run(
    coordinator: Arc<HybridClusterCoordinator>,
    supervisor_addr: &str,
) -> Result<()> {
    info!("LLM Router: initialising MoE weight management");
    info!("LLM Router: supervisor node at {}", supervisor_addr);
    info!("LLM Router: {} P100 nodes available for KV cache", coordinator.nodes.len());
    info!("LLM Router: host RAM pool capacity = {}MB",
        coordinator.host_ram_pool.capacity() / (1024 * 1024));

    // In the current architecture, KoboldCPP handles actual inference.
    // The hybrid engine manages the memory layout transitions.
    // When a spatial job arrives, the coordinator pages out the LLM buffers
    // and stages the GeoTIFF tile — then resumes LLM mode after the scan.

    info!("LLM Router: delegating inference to KoboldCPP on port 5001");
    info!("LLM Router: monitoring for spatial interrupt requests...");

    // Keep alive — the coordinator handles mode transitions via its atomic flag.
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        // Check if a spatial job has interrupted us.
        if !crate::LLM_MODE_ACTIVE.load(std::sync::atomic::Ordering::Acquire) {
            info!("LLM Router: spatial interrupt detected — yielding P100 buffers");
            // Wait for spatial pass to complete, then resume.
            coordinator.resume_llm_mode().await;
            info!("LLM Router: spatial pass complete — resuming LLM mode");
        }
    }
}
