//! Hybrid Cluster Coordinator — manages dual P100 wgpu devices and
//! orchestrates zero-copy buffer swaps between LLM and Spatial roles.
//!
//! Memory tiers:
//!   Tier 1 — GPU HBM2 (32GB total): active KV cache / active compute shaders
//!   Tier 2 — Host DDR4 (94GB):      inactive MoE expert weights / offloaded KV history
//!   Tier 3 — Network (cesarops2):   long-horizon context / supervisor orchestration

use anyhow::{Context, Result};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use tracing::{info, warn};
use wgpu::util::DeviceExt;

use crate::LLM_MODE_ACTIVE;

// ── Buffer size constants ─────────────────────────────────────────────────────

/// KV ring buffer per P100: 12GB — holds active attention heads + KV cache.
/// Leaves 4GB headroom for shader workgroups and staging.
const KV_RING_BUFFER_BYTES: u64 = 12 * 1024 * 1024 * 1024;

/// Spatial staging buffer per P100: 4GB — GeoTIFF tile ingestion + shader output.
const SPATIAL_STAGING_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Anomaly output buffer: 64MB — sparse detection results after in-shader reduction.
const ANOMALY_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

// ── P100 Node ─────────────────────────────────────────────────────────────────

/// Represents one Tesla P100 GPU with its wgpu device, queue, and pre-allocated buffers.
/// Buffers are allocated once at startup and reused across role flips — no re-allocation.
pub struct P100Node {
    pub device: Arc<wgpu::Device>,
    pub queue:  Arc<wgpu::Queue>,

    /// Tier 1: LLM KV cache ring buffer — stays allocated even in Spatial mode.
    /// We never destroy it; we simply stop submitting to it during Spatial passes.
    pub llm_kv_ring_buffer: wgpu::Buffer,

    /// Tier 1: GeoTIFF tile staging — written via PCIe Gen3 DMA from host.
    pub spatial_staging_buffer: wgpu::Buffer,

    /// Tier 1: Sparse anomaly output — read back to host after in-shader reduction.
    pub anomaly_output_buffer: wgpu::Buffer,

    /// Tracks bytes currently staged in spatial_staging_buffer.
    pub staged_bytes: AtomicU64,
}

impl P100Node {
    async fn init(adapter: wgpu::Adapter, gpu_index: usize) -> Result<Self> {
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some(&format!("P100-{}", gpu_index)),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .context("Failed to create wgpu device for P100")?;

        let device = Arc::new(device);
        let queue  = Arc::new(queue);

        // Pre-allocate KV ring buffer — never freed, just unused during Spatial mode.
        // Using STORAGE | COPY_DST so the LLM attention kernel can write into it.
        let llm_kv_ring_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label:              Some(&format!("P100-{} KV Ring Buffer", gpu_index)),
            size:               KV_RING_BUFFER_BYTES,
            usage:              wgpu::BufferUsages::STORAGE
                              | wgpu::BufferUsages::COPY_DST
                              | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        // Pre-allocate spatial staging buffer — written by host DMA, read by compute shader.
        let spatial_staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label:              Some(&format!("P100-{} Spatial Staging", gpu_index)),
            size:               SPATIAL_STAGING_BYTES,
            usage:              wgpu::BufferUsages::STORAGE
                              | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Pre-allocate anomaly output buffer — written by compute shader, read back to host.
        let anomaly_output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label:              Some(&format!("P100-{} Anomaly Output", gpu_index)),
            size:               ANOMALY_OUTPUT_BYTES,
            usage:              wgpu::BufferUsages::STORAGE
                              | wgpu::BufferUsages::COPY_SRC
                              | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        info!(
            "P100-{}: KV={}GB staging={}GB anomaly={}MB — buffers allocated",
            gpu_index,
            KV_RING_BUFFER_BYTES / (1024 * 1024 * 1024),
            SPATIAL_STAGING_BYTES / (1024 * 1024 * 1024),
            ANOMALY_OUTPUT_BYTES / (1024 * 1024),
        );

        Ok(Self {
            device,
            queue,
            llm_kv_ring_buffer,
            spatial_staging_buffer,
            anomaly_output_buffer,
            staged_bytes: AtomicU64::new(0),
        })
    }
}

// ── Hybrid Cluster Coordinator ────────────────────────────────────────────────

/// Manages both P100 nodes and coordinates role transitions.
///
/// Role flip is zero-copy: we never reallocate buffers. Instead we:
///   - LLM → Spatial: stop submitting to KV buffer, DMA GeoTIFF into staging buffer
///   - Spatial → LLM: stop dispatching compute shaders, resume KV submissions
pub struct HybridClusterCoordinator {
    pub nodes: Vec<P100Node>,

    /// Tier 2: host RAM pool for inactive MoE expert weights.
    /// 94GB DDR4 — holds all 30B parameters when not active on GPU.
    /// In production: replace Vec<u8> with a memory-mapped file or
    /// a custom allocator backed by huge pages for NUMA-aware access.
    pub host_ram_pool: Vec<u8>,

    /// Counts in-flight spatial dispatches — coordinator waits for zero
    /// before allowing a role flip back to LLM mode.
    pub active_spatial_dispatches: Arc<AtomicU64>,
}

impl HybridClusterCoordinator {
    /// Discover all available P100 adapters and initialise one P100Node per GPU.
    pub async fn init() -> Result<Self> {
        let instance = wgpu::Instance::default();

        // Enumerate all high-performance adapters — one per physical P100.
        let adapters: Vec<wgpu::Adapter> = instance
            .enumerate_adapters(wgpu::Backends::VULKAN | wgpu::Backends::METAL | wgpu::Backends::DX12)
            .await
            .into_iter()
            .filter(|a| {
                let info = a.get_info();
                // Only discrete GPUs — skip software/CPU adapters.
                info.device_type == wgpu::DeviceType::DiscreteGpu
            })
            .collect();

        if adapters.is_empty() {
            warn!("No discrete GPU adapters found — falling back to default adapter");
        }

        let mut nodes = Vec::new();

        if adapters.is_empty() {
            // Fallback: use the default adapter (may be integrated or software)
            if let Ok(adapter) = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
            {
                let info = adapter.get_info();
                info!("Fallback adapter: {} ({:?})", info.name, info.device_type);
                nodes.push(P100Node::init(adapter, 0).await?);
            }
        } else {
            for (i, adapter) in adapters.into_iter().enumerate() {
                let info = adapter.get_info();
                info!("GPU {}: {} ({:?})", i, info.name, info.device_type);
                nodes.push(P100Node::init(adapter, i).await?);
            }
        }

        // Tier 2: pre-allocate a modest host RAM pool for MoE weight staging.
        // In production this would be a memory-mapped file on the 94GB DDR4.
        // We use 1GB here as a placeholder — the real pool is managed by the LLM router.
        let host_ram_pool = Vec::with_capacity(1024 * 1024 * 1024);

        Ok(Self {
            nodes,
            host_ram_pool,
            active_spatial_dispatches: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Transition a P100 node from LLM mode to Spatial mode.
    ///
    /// Zero-copy: we DMA the GeoTIFF layer directly into the pre-allocated
    /// spatial staging buffer over PCIe Gen3. The KV ring buffer is left
    /// intact — we simply stop submitting to it.
    pub async fn page_out_llm_and_stage_spatial(
        &self,
        node_idx: usize,
        geotiff_layer: &[u8],
    ) -> Result<()> {
        let node = self.nodes.get(node_idx)
            .context("Node index out of range")?;

        let layer_bytes = geotiff_layer.len() as u64;
        if layer_bytes > SPATIAL_STAGING_BYTES {
            anyhow::bail!(
                "GeoTIFF layer ({} MB) exceeds spatial staging buffer ({} MB). \
                 Tile the input before staging.",
                layer_bytes / (1024 * 1024),
                SPATIAL_STAGING_BYTES / (1024 * 1024),
            );
        }

        // DMA write directly into GPU staging buffer over PCIe Gen3.
        // wgpu handles the host→device transfer; no intermediate copy.
        node.queue.write_buffer(
            &node.spatial_staging_buffer,
            0,
            geotiff_layer,
        );

        // Record staged byte count for the compute shader dispatch.
        node.staged_bytes.store(layer_bytes, Ordering::Release);

        // Signal global mode — atomic, no lock needed.
        LLM_MODE_ACTIVE.store(false, Ordering::SeqCst);
        self.active_spatial_dispatches.fetch_add(1, Ordering::AcqRel);

        info!(
            "P100-{}: staged {}MB GeoTIFF → spatial mode active",
            node_idx,
            layer_bytes / (1024 * 1024)
        );

        Ok(())
    }

    /// Transition back to LLM mode after spatial pass completes.
    ///
    /// Waits for all in-flight spatial dispatches to drain before flipping.
    /// The KV ring buffer was never freed — LLM can resume immediately.
    pub async fn resume_llm_mode(&self) {
        // Spin-wait for spatial dispatches to drain — typically microseconds.
        let mut spins = 0u32;
        while self.active_spatial_dispatches.load(Ordering::Acquire) > 0 {
            tokio::task::yield_now().await;
            spins += 1;
            if spins > 10_000 {
                warn!("LLM resume: waiting for {} spatial dispatches to drain",
                    self.active_spatial_dispatches.load(Ordering::Acquire));
                spins = 0;
            }
        }

        LLM_MODE_ACTIVE.store(true, Ordering::SeqCst);
        info!("All P100 nodes: spatial dispatches drained — LLM mode resumed");
    }

    /// Signal that a spatial dispatch has completed.
    pub fn spatial_dispatch_complete(&self) {
        self.active_spatial_dispatches.fetch_sub(1, Ordering::AcqRel);
    }

    /// Returns the total GPU VRAM across all nodes in bytes.
    pub fn total_vram_bytes(&self) -> u64 {
        self.nodes.len() as u64 * (KV_RING_BUFFER_BYTES + SPATIAL_STAGING_BYTES + ANOMALY_OUTPUT_BYTES)
    }
}
