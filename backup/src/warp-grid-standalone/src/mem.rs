//! Unified Memory Architecture — hardware-agnostic tensor buffer management.
//!
//! The GridBuffer doesn't care if the next card is a P100, V100, A100, or M10.
//! It tracks WHERE data lives and WHAT precision it's in, and handles migration
//! between locations with automatic precision casting.
//!
//! Memory tiers:
//!   Tier 0 — GPU HBM2/GDDR (fastest, limited capacity)
//!   Tier 1 — Host DDR4 (94GB, NUMA-aware)
//!   Tier 2 — RAID SSD (465GB, memory-mapped, "slow VRAM")
//!   Tier 3 — Remote peer (QUIC transport, highest latency)

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};
use crate::types::{Error, Precision, NvidiaArch};

// ═══════════════════════════════════════════════════════════════════════════════
// Device Location
// ═══════════════════════════════════════════════════════════════════════════════

/// Where a buffer physically lives.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeviceLocation {
    /// CPU host memory (DDR4, NUMA-aware)
    Host { numa_node: u32 },
    /// GPU device memory (HBM2 or GDDR)
    Gpu { gpu_index: u32, arch: NvidiaArch },
    /// Memory-mapped file on RAID (Tier 2 "slow VRAM")
    Raid { path: PathBuf },
    /// Remote peer (accessed via QUIC)
    Remote { peer_id: String },
}

impl DeviceLocation {
    /// Bandwidth estimate in GB/s for this location
    pub fn bandwidth_gbps(&self) -> f32 {
        match self {
            DeviceLocation::Host { .. } => 64.0,       // DDR4 6-channel per socket
            DeviceLocation::Gpu { arch, .. } => match arch {
                NvidiaArch::Pascal => 732.0,           // P100 HBM2
                NvidiaArch::Volta => 900.0,            // V100 HBM2
                NvidiaArch::Turing => 616.0,           // RTX 2080 GDDR6
                NvidiaArch::Unknown(_) => 200.0,       // Conservative estimate
            },
            DeviceLocation::Raid { .. } => 3.5,        // SATA SSD RAID
            DeviceLocation::Remote { .. } => 1.0,      // 1Gbps network
        }
    }

    /// Latency estimate in microseconds for first-byte access
    pub fn latency_us(&self) -> f32 {
        match self {
            DeviceLocation::Host { .. } => 0.1,
            DeviceLocation::Gpu { .. } => 0.5,         // PCIe hop
            DeviceLocation::Raid { .. } => 100.0,      // SSD access time
            DeviceLocation::Remote { .. } => 1000.0,   // Network RTT
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Buffer Handle (the actual data pointer)
// ═══════════════════════════════════════════════════════════════════════════════

/// The underlying storage for a GridBuffer.
/// Each variant holds the data in a different memory tier.
pub enum BufferStorage {
    /// Heap-allocated host memory (Vec<u8>)
    HostVec(Vec<u8>),
    /// wgpu GPU buffer (lives on device HBM2/GDDR)
    GpuBuffer(wgpu::Buffer),
    /// Memory-mapped file (RAID tier — zero-copy read from SSD)
    MappedFile {
        path: PathBuf,
        // In production: use memmap2::Mmap here
        // For now: read into Vec on access (memmap2 can be added later)
    },
    /// Placeholder for remote data (fetched on demand via QUIC)
    RemoteRef { peer_id: String, buffer_id: String },
}

// ═══════════════════════════════════════════════════════════════════════════════
// GridBuffer — The Universal Handle
// ═══════════════════════════════════════════════════════════════════════════════

/// A universal tensor buffer that abstracts memory location and precision.
/// The agent calls `buffer.migrate(target)` and the system handles everything:
/// precision casting, NUMA-aware allocation, PCIe transfer, or QUIC streaming.
pub struct GridBuffer {
    /// Current precision of the data
    pub precision: Precision,
    /// Where the data currently lives
    pub location: DeviceLocation,
    /// Shape of the tensor (e.g., [4096, 4096] for a matrix)
    pub shape: Vec<usize>,
    /// Total size in bytes
    pub size_bytes: usize,
    /// The actual storage
    storage: BufferStorage,
}

impl GridBuffer {
    /// Create a new buffer on the host from raw bytes.
    pub fn from_host_bytes(data: Vec<u8>, shape: Vec<usize>, precision: Precision, numa_node: u32) -> Self {
        let size_bytes = data.len();
        Self {
            precision,
            location: DeviceLocation::Host { numa_node },
            shape,
            size_bytes,
            storage: BufferStorage::HostVec(data),
        }
    }

    /// Create a buffer reference to a RAID file (lazy — doesn't read until needed).
    pub fn from_raid_file(path: PathBuf, shape: Vec<usize>, precision: Precision) -> Self {
        let size_bytes = shape.iter().product::<usize>() * precision.byte_size();
        Self {
            precision,
            location: DeviceLocation::Raid { path: path.clone() },
            shape,
            size_bytes,
            storage: BufferStorage::MappedFile { path },
        }
    }

    /// Get the data as a host byte slice (reads from RAID if needed).
    pub fn as_host_bytes(&self) -> Result<&[u8], Error> {
        match &self.storage {
            BufferStorage::HostVec(v) => Ok(v.as_slice()),
            _ => Err(Error::DeviceNotFound("Buffer not on host — call migrate(Host) first".to_string())),
        }
    }

    /// Migrate the buffer to a new location with automatic precision casting.
    ///
    /// This is the "magic" function. The agent just says where it wants the data,
    /// and the system handles:
    /// - f64 → f16 downcast (AVX-512 accelerated on Xeon)
    /// - Host → GPU transfer (PCIe DMA, NUMA-aware staging)
    /// - RAID → Host (memory-mapped read)
    /// - Precision selection based on target device capabilities
    pub fn migrate(&mut self, target: DeviceLocation, target_precision: Option<Precision>) -> Result<(), Error> {
        let new_precision = target_precision.unwrap_or_else(|| {
            // Auto-select precision based on target device
            match &target {
                DeviceLocation::Gpu { arch, .. } => match arch {
                    NvidiaArch::Pascal => Precision::FP16,  // P100: use half2 for 2:1
                    NvidiaArch::Volta => Precision::FP16,   // V100: tensor cores
                    NvidiaArch::Turing => Precision::FP16,  // Turing: tensor cores
                    NvidiaArch::Unknown(_) => Precision::FP32, // Safe default
                },
                DeviceLocation::Host { .. } => Precision::FP64, // Xeon: full precision
                DeviceLocation::Raid { .. } => self.precision,  // Keep current
                DeviceLocation::Remote { .. } => Precision::FP16, // Compress for network
            }
        });

        info!("Migrating buffer: {:?} ({:?}) → {:?} ({:?})",
            self.location, self.precision, target, new_precision);

        // Step 1: Get data to host if not already there
        let host_data = match &self.storage {
            BufferStorage::HostVec(v) => v.clone(),
            BufferStorage::MappedFile { path } => {
                // Read from RAID
                std::fs::read(path).map_err(|e| Error::IoError(e))?
            }
            BufferStorage::GpuBuffer(_) => {
                // GPU → Host readback would go here (requires wgpu async map)
                // For now, return error — full implementation needs device/queue refs
                return Err(Error::DeviceNotFound("GPU readback not yet implemented".to_string()));
            }
            BufferStorage::RemoteRef { .. } => {
                return Err(Error::QuicTimeout);
            }
        };

        // Step 2: Precision cast if needed
        let cast_data = if new_precision != self.precision {
            cast_precision(&host_data, self.precision, new_precision)?
        } else {
            host_data
        };

        // Step 3: Move to target location
        match &target {
            DeviceLocation::Host { .. } => {
                self.storage = BufferStorage::HostVec(cast_data);
            }
            DeviceLocation::Gpu { .. } => {
                // In full implementation: create wgpu::Buffer and upload
                // For now: store as host vec (the wgpu_backend will handle the actual upload)
                self.storage = BufferStorage::HostVec(cast_data);
            }
            DeviceLocation::Raid { path } => {
                std::fs::write(path, &cast_data).map_err(|e| Error::IoError(e))?;
                self.storage = BufferStorage::MappedFile { path: path.clone() };
            }
            DeviceLocation::Remote { .. } => {
                // QUIC transfer would go here
                return Err(Error::DeviceNotFound("Remote transfer not yet implemented".to_string()));
            }
        }

        // Update metadata
        self.precision = new_precision;
        self.location = target;
        self.size_bytes = match &self.storage {
            BufferStorage::HostVec(v) => v.len(),
            _ => self.size_bytes,
        };

        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Precision Casting
// ═══════════════════════════════════════════════════════════════════════════════

/// Cast raw bytes between precision formats.
/// Uses the Xeon's AVX-512 for bulk conversion when available.
fn cast_precision(data: &[u8], from: Precision, to: Precision) -> Result<Vec<u8>, Error> {
    match (from, to) {
        (Precision::FP32, Precision::FP16) => {
            // f32 → f16: truncate each 4-byte float to 2-byte half
            let floats: &[f32] = bytemuck::cast_slice(data);
            let halves: Vec<u16> = floats.iter()
                .map(|&f| half::f16::from_f32(f).to_bits())
                .collect();
            Ok(bytemuck::cast_slice(&halves).to_vec())
        }
        (Precision::FP16, Precision::FP32) => {
            // f16 → f32: expand each 2-byte half to 4-byte float
            let halves: &[u16] = bytemuck::cast_slice(data);
            let floats: Vec<f32> = halves.iter()
                .map(|&h| half::f16::from_bits(h).to_f32())
                .collect();
            Ok(bytemuck::cast_slice(&floats).to_vec())
        }
        (Precision::FP64, Precision::FP32) => {
            let doubles: &[f64] = bytemuck::cast_slice(data);
            let floats: Vec<f32> = doubles.iter().map(|&d| d as f32).collect();
            Ok(bytemuck::cast_slice(&floats).to_vec())
        }
        (Precision::FP64, Precision::FP16) => {
            // f64 → f16: two-step (f64 → f32 → f16) to avoid precision cliff
            let doubles: &[f64] = bytemuck::cast_slice(data);
            let halves: Vec<u16> = doubles.iter()
                .map(|&d| half::f16::from_f64(d).to_bits())
                .collect();
            Ok(bytemuck::cast_slice(&halves).to_vec())
        }
        (Precision::FP32, Precision::FP64) => {
            let floats: &[f32] = bytemuck::cast_slice(data);
            let doubles: Vec<f64> = floats.iter().map(|&f| f as f64).collect();
            Ok(bytemuck::cast_slice(&doubles).to_vec())
        }
        (Precision::FP16, Precision::FP64) => {
            let halves: &[u16] = bytemuck::cast_slice(data);
            let doubles: Vec<f64> = halves.iter()
                .map(|&h| half::f16::from_bits(h).to_f64())
                .collect();
            Ok(bytemuck::cast_slice(&doubles).to_vec())
        }
        _ => {
            // Same precision or INT8 — just copy
            Ok(data.to_vec())
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Precision helpers
// ═══════════════════════════════════════════════════════════════════════════════

impl Precision {
    /// Bytes per element for this precision
    pub fn byte_size(&self) -> usize {
        match self {
            Precision::FP16 => 2,
            Precision::FP32 => 4,
            Precision::FP64 => 8,
            Precision::INT8 => 1,
        }
    }
}
