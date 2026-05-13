pub mod types;
pub mod numa;
pub mod pool;
pub mod metrics;
pub mod mem;
pub mod translator;

pub use types::*;

use numa::NumaTopology;
use tracing::{info, warn};

/// Main entry point for the Warp Grid distributed compute runtime.
pub struct WarpGrid {
    pub numa: NumaTopology,
}

impl WarpGrid {
    /// Initialize the grid, detecting NUMA topology and local GPU capabilities.
    pub fn new() -> Result<Self, Error> {
        let numa = NumaTopology::detect()?;
        info!("WarpGrid initialized: {} NUMA nodes, {} GPUs detected",
            numa.socket_count, numa.gpu_affinity.len());

        // Check for F16 support would go here once wgpu adapter is enumerated
        // For now, log the GPU-to-socket mapping
        for (gpu_idx, socket_id) in &numa.gpu_affinity {
            info!("  GPU {} → Socket {}", gpu_idx, socket_id);
        }

        Ok(WarpGrid { numa })
    }
}
