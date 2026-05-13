# Warp-Grid: Technical Design

## Reference Architecture

### Core Struct

```rust
pub struct WarpGrid {
    local_pool: VirtualComputePool,
    remote_peers: PeerNetwork,
    kernel_forge: KernelForge,
    scheduler: GridScheduler,
}

impl WarpGrid {
    pub async fn dispatch(&self, task: ComputeTask) -> Result<Tensor, Error> {
        let profile = self.scheduler.analyze(&task);
        
        match profile.best_fit() {
            Hardware::Pascal => self.local_pool.execute_pascal(task).await,
            Hardware::Turing => self.local_pool.execute_tensor_cores(task).await,
            Hardware::RemoteNode => self.remote_peers.steal_cycles(task).await,
            Hardware::Xeon => self.local_pool.execute_avx512(task).await,
            Hardware::TPU => self.local_pool.execute_int8(task).await,
        }
    }
}
```

### Module Design

#### 1. Discovery (discovery.rs)

Enumerates all NVIDIA GPUs via wgpu, classifies by SM version, maps architecture-specific features.

```rust
pub enum NvidiaArch {
    Pascal(String),  // SM 6.0 (P100) or 6.1 (1070/P1000/P106)
    Volta(String),   // SM 7.0 (V100)
    Turing(String),  // SM 7.5 (RTX 20-series, T4)
}

pub struct GpuNode {
    pub name: String,
    pub arch: NvidiaArch,
    pub sm_version: (u32, u32),
    pub features: Vec<&'static str>,
    pub vram_mb: u64,
    pub bandwidth_gbps: f32,
}
```

Feature flags per architecture:
- P100 (SM 6.0): `["2:1_FP16", "HBM2", "NVLink"]`
- 1070/P1000/P106 (SM 6.1): `["Simulated_BF16"]`
- V100 (SM 7.0): `["Tensor_Cores", "BF16_Native", "HBM2"]`
- Turing (SM 7.5): `["Tensor_Cores", "Ray_Tracing", "INT8_DP4A"]`

#### 2. QUIC Transport (seti/quic.rs)

Uses quinn for sub-50ms P2P cycle stealing. Eliminates TCP head-of-line blocking.

```rust
pub struct GridPeer {
    endpoint: Endpoint,
}

impl GridPeer {
    pub async fn start_node(addr: SocketAddr) -> Self;
    pub async fn offload_task(&self, peer: SocketAddr, task: Vec<u8>) -> Vec<u8>;
}
```

Transport features:
- Multiplexed streams: tensor data + control signals simultaneously
- Unreliable datagrams: intermediate shader results (fire-and-forget)
- TLS 1.3 native: zero-cost encryption
- Multipath: WiFi + Ethernet simultaneously

#### 3. Self-Healing Live Update (update.rs)

Uses `self-replace` for atomic binary hot-swap. Uses `spirv-builder` for runtime shader regeneration.

```rust
pub fn deploy_new_grid_binary(new_bin_path: &Path) -> Result<(), std::io::Error>;
pub fn reload_spirv_shaders(arch: NvidiaArch);
```

#### 4. Kernel Forge (kernel_forge.rs)

JIT shader specialization per architecture:
- Detects target SM version at dispatch time
- Injects architecture-specific opcodes via naga transforms
- Maintains SPIR-V cache keyed by (shader_hash, target_arch)
- Hot-reloads shader packs without process restart

#### 5. Pipeline Parallelism (pipeline.rs)

Grid-level fusion that beats single-machine PCIe:
- While P100 processes Layer N, Layer N+1 data streams to remote card
- Compressed bf16 format for network transfer
- Overlap compute and transfer (double-buffering)

#### 6. Low-Memory Bridge (bridge.rs)

DDR3 node handling:
- Detects memory bandwidth at startup
- Flags DDR3 nodes as "preprocessor" role
- 1060/1070 on DDR3 compress data before sending to P100s on DDR4
- Never sends uncompressed FP32 to DDR3 nodes

## Performance Model: Hardware-Aware Tiering

The scheduler operates in two modes simultaneously: **Specialist** (optimized paths for known hardware) and **Adaptive** (generic fallbacks for unknown/guest nodes). Hardware is treated as "Capabilities" not "Models" — the binary switches between ultra-optimized assembly paths and portable fallbacks at dispatch time.

### Tier 1: Your Grid (Pascal/Skylake Specialist)

#### P100 FP16 2:1 Exploitation (GP100 — SM 6.0)

Unlike the 1070/P106 (SM 6.1), the P100 has a unique 2:1 FP16-to-FP32 throughput ratio. The Kernel Forge MUST emit specialized `f16vec2` (Half2) shaders for P100 targets:

```rust
/// P100-specific: pack two FP16 ops into one clock cycle (HFMA2)
/// Effective throughput: ~21 TFLOPS (vs ~10.6 TFLOPS FP32)
pub struct PascalFp16Strategy {
    /// Use f16vec2 vectorized operations — 2 ops per clock
    pub use_half2: bool,
    /// Pack adjacent tensor elements into vec2 for HFMA2
    pub vectorize_width: u32, // Always 2 for P100
}

impl PascalFp16Strategy {
    pub fn for_p100() -> Self {
        Self { use_half2: true, vectorize_width: 2 }
    }
    pub fn for_sm61() -> Self {
        // 1070/P1000/P106: FP16 is 1:64 ratio — NOT worth using
        // Fall back to FP32 path
        Self { use_half2: false, vectorize_width: 1 }
    }
}
```

##### SPIR-V Half2 Kernel Architecture

The P100 is the ONLY Pascal chip with double-rate half precision. On the 1070/P1000, FP16 is emulated (slower than FP32). The Kernel Forge emits different SPIR-V based on SM version:

**P100 Optimized Path** — uses `OpTypeFloat 16` + `OpCapability Float16`:
```glsl
#version 450
#extension GL_EXT_shader_explicit_arithmetic_types_float16 : require

layout(local_size_x = 64) in; // Optimized for Pascal warp size (32 threads × 2)

// float16_t2 (Packed FP16) — two ops per clock on P100
layout(set = 0, binding = 0) buffer Data { float16_t2[] packed_tiles; };

void main() {
    uint id = gl_GlobalInvocationID.x;

    // HFMA2: Half-precision Fused Multiply-Add on 2 elements simultaneously
    // On P100, this instruction is as fast as a single f32 add
    float16_t2 val = packed_tiles[id];
    float16_t2 factor = float16_t2(1.01hf, 1.01hf);

    packed_tiles[id] = (val * factor) + val;
}
```

**SPIR-V Capabilities Required** (emitted by naga for P100 targets):
- `OpCapability Float16`
- `OpCapability StorageBuffer16BitAccess`
- `OpCapability UniformAndStorageBuffer16BitAccess`
- `OpExtension "SPV_KHR_16bit_storage"`

**SM 6.1 Fallback** (1070/P1000/P106) — standard FP32, no half2:
```glsl
#version 450
layout(local_size_x = 64) in;
layout(set = 0, binding = 0) buffer Data { float[] tiles; };

void main() {
    uint id = gl_GlobalInvocationID.x;
    tiles[id] = (tiles[id] * 1.01) + tiles[id];
}
```

**Kernel Forge Decision Logic:**
```rust
pub fn select_shader_variant(&self, target: &DeviceProfile) -> ShaderPath {
    match target.sm_version {
        (6, 0) => {
            // P100: emit SPIR-V with Float16 capability + f16vec2 packing
            ShaderPath::PascalHalf2("shaders/pascal/matmul_half2.spv")
        }
        (6, 1) => {
            // 1070/P1000/P106: FP16 is 1:64 — use FP32 path
            ShaderPath::GenericF32("shaders/generic/matmul_f32.spv")
        }
        (7, 5) => {
            // Turing: Tensor Core wmma hints via naga injection
            ShaderPath::TuringTensorCore("shaders/turing/matmul_wmma.spv")
        }
        _ => ShaderPath::GenericF32("shaders/generic/matmul_f32.spv"),
    }
}
```

**Why this beats CUDA**: Standard CUDA doesn't automatically select double-rate FP16 paths for P100 vs emulated FP16 on 1070 — developers must manually use `__half2` intrinsics. By baking this into the Kernel Forge, warp-grid is more efficient out-of-the-box than a naive PyTorch/CUDA implementation.

WGSL shader selection summary:
- P100 → `shaders/pascal/matmul_half2.wgsl` (vectorized FP16, 2:1 throughput)
- 1070/P106 → `shaders/generic/matmul_f32.wgsl` (FP32, no FP16 benefit)
- Turing → `shaders/turing/matmul_wmma.wgsl` (Tensor Core hints)

#### Xeon 4110 NUMA-Aware Memory Pinning

Two sockets = two NUMA domains. Crossing the UPI interconnect adds ~100ns latency per access and loses ~30% bandwidth. The scheduler MUST pin GPU-feeding threads to the correct socket:

```rust
use hwloc::{Topology, ObjectType, MEMBIND_BIND, MEMBIND_STRICT};

/// NUMA topology for dual-socket Xeon 4110
/// Socket 0 → P100 #0 (PCIe slot closest to CPU 0)
/// Socket 1 → P100 #1 (PCIe slot closest to CPU 1)
pub struct NumaTopology {
    pub socket_count: u32,
    pub gpu_affinity: Vec<(GpuId, SocketId)>, // Which GPU is on which socket
    topo: Topology,
}

impl NumaTopology {
    /// Pin the current thread to a specific socket's cores AND memory domain
    /// This ensures all allocations stay on local DDR4 channels
    pub fn pin_worker_to_socket(&self, socket_index: u32) {
        let socket = self.topo.objects_at_depth(ObjectType::Package)
            .get(socket_index as usize)
            .expect("Socket not found");

        // 1. Bind THREAD to this socket's CPU cores
        let cpuset = socket.cpuset().unwrap();
        self.topo.set_cpubind(cpuset, hwloc::CPUBIND_THREAD).unwrap();

        // 2. Bind MEMORY to this socket's RAM (strict — no fallback to remote)
        let nodeset = socket.nodeset().unwrap();
        self.topo.set_membind(nodeset, MEMBIND_BIND, MEMBIND_STRICT)
            .expect("Failed to pin memory to NUMA node");
    }

    /// Pin the thread pool feeding P100 #0 to Socket 0's cores
    /// Pin the thread pool feeding P100 #1 to Socket 1's cores
    /// This eliminates the QPI/UPI tax on memory transfers
    pub fn pin_gpu_threads(&self) {
        for (gpu_id, socket_id) in &self.gpu_affinity {
            // Spawn dedicated thread pool pinned to the correct socket
            self.pin_worker_to_socket(*socket_id);
        }
    }

    /// Allocate staging buffers on the NUMA node closest to the target GPU
    /// Data stays on local 6-channel DDR4 controller (64 GB/s per socket)
    pub fn alloc_numa_local(&self, gpu: GpuId, size: usize) -> *mut u8;
}
```

**Your Grid** rules:
- `pin_worker_to_socket(0)` for threads feeding P100 #0
- `pin_worker_to_socket(1)` for threads feeding P100 #1
- Result: data stays on local 6-channel DDR4 (64 GB/s), never crosses UPI

**Any Grid** fallback:
- Use `MEMBIND_NEXTTOUCH` (lazy allocation)
- OS waits to see which CPU touches the memory first, then migrates the page
- Works on single-socket and unknown topologies without explicit pinning

Rules:
- Staging buffers for P100 #0 → allocated on Socket 0's DDR4 channels
- Staging buffers for P100 #1 → allocated on Socket 1's DDR4 channels
- AVX-512 compute threads → pinned to the socket with the most free memory
- Never let a thread on Socket 0 feed data to P100 #1 (UPI penalty: ~40% bandwidth loss)

#### PCIe Topology Awareness

```
Socket 0 ──── PCIe 3.0 x16 ──── P100 #0 (15.75 GB/s each direction)
    │
    └── DDR4 channels 0-2 (3× 21.3 GB/s = 64 GB/s)

Socket 1 ──── PCIe 3.0 x16 ──── P100 #1 (15.75 GB/s each direction)
    │
    └── DDR4 channels 3-5 (3× 21.3 GB/s = 64 GB/s)

UPI link between sockets: ~38.4 GB/s (penalty path — avoid)
```

### Tier 2: Any Grid (Adaptive Orchestration)

For unknown/guest hardware, the scheduler uses capability probing instead of hardcoded paths:

#### Capability Discovery

```rust
pub struct NodeCapabilities {
    pub fp16_ratio: f32,        // 2.0 for P100, 0.015 for 1070, 8.0 for Turing TC
    pub memory_type: MemType,   // HBM2, GDDR5, GDDR6, DDR3, DDR4
    pub bandwidth_gbps: f32,    // Measured at startup via microbenchmark
    pub numa_aware: bool,       // Multi-socket detected
    pub avx512: bool,           // CPU SIMD capability
    pub tensor_cores: bool,     // Turing/Ampere
}

pub enum DispatchTier {
    /// Known hardware with optimized paths (P100 half2, Xeon NUMA)
    Specialist(SpecialistConfig),
    /// Unknown hardware — use generic shaders, adaptive sharding
    Adaptive(AdaptiveConfig),
    /// Low-capability node — preprocessor role only
    Preprocessor,
}
```

Decision logic:
1. Probe `wgpu` adapter for FP16 support → if ratio < 1.0, skip FP16 path entirely
2. Measure memory bandwidth → if < 100 GB/s, classify as "preprocessor" tier
3. Check NUMA topology → if multi-socket, enable affinity pinning
4. If none of the specialist paths match → fall back to generic FP32 shaders

#### Virtual Device Sharding (Atomic Tiles)

Instead of sending whole tasks to one card, the "any grid" mode shards work into atomic tiles:

```rust
pub struct TileShardStrategy {
    /// Fast nodes (P100, HBM2): high-density compute tiles (center of image)
    pub fast_tiles: Vec<TileRegion>,
    /// Slow nodes (1060, DDR3): edge/background tiles (latency-tolerant)
    pub slow_tiles: Vec<TileRegion>,
    /// Best-effort nodes (WebGPU): overflow tiles (may not complete)
    pub overflow_tiles: Vec<TileRegion>,
}
```

Rules:
- Fast nodes get compute-dense tiles (center, high-frequency features)
- Slow nodes get edge tiles (background, low-frequency, latency-tolerant)
- WebGPU/guest nodes get overflow tiles (duplicated on a fast node as backup)
- If a slow node doesn't return within 2× expected time, the fast node's backup result is used

#### Network Resilience (QUIC Unreliable Datagrams)

For "any grid" with unreliable internet peers:
- Intermediate results use QUIC unreliable datagrams (fire-and-forget)
- Final results use reliable QUIC streams (guaranteed delivery)
- If a guest node hiccups, the pipeline doesn't stall — the Master reassigns the tile
- Timeout: 2× expected compute time → auto-reassign to local hardware

### Unified Performance Stack

| Component | Your Grid (Specialist) | Any Grid (Adaptive) |
|-----------|----------------------|---------------------|
| **Compute** | Native half2 on P100s (~21 TFLOPS) | Generic f32 with naga translation |
| **Memory** | NUMA-pinned allocations to Xeon sockets | Local-affinity first, numanji fallback |
| **Networking** | Fixed-route internal PCIe (15.75 GB/s) | quinn QUIC for cycle stealing |
| **Sharding** | Full tasks to known devices | Atomic tiles by capability tier |
| **Recovery** | Watchdog triggers self-replace | Decentralized K-line / G-line bans |
| **Precision** | FP16 2:1 on P100, FP32 on SM_61 | Probe → select best available |
| **Scheduling** | Topology-aware (socket → GPU affinity) | Capability-based (TFLOPS + bandwidth) |

## Integration with CesarOps Agent

### Boot Phase
1. Binary runs `discover_grid_nodes()`
2. Reports TFLOPS + SM versions to agent via webhook
3. Registers with peer network via mDNS

### Task Phase
1. Agent submits ComputeTask to WarpGrid
2. Scheduler analyzes cost, selects device(s)
3. Kernel Forge selects architecture-specific shader
4. Execution on optimal hardware
5. Result returned to agent

### Update Phase
1. Agent builds better SPIR-V kernel
2. Sends .spv file to binary
3. Binary calls `reload_spirv_shaders()` — hot-swap without restart
4. If binary itself needs update: agent sends new binary, `self-replace` swaps atomically

### Overflow Phase
1. Local P100s at 100% utilization
2. Agent identifies free remote node on quinn grid
3. Triggers `offload_task()` with SPIR-V kernel + input buffer
4. Remote node executes in WASM sandbox
5. Result streams back via QUIC

## Performance Targets

| Operation | Target | Hardware |
|-----------|--------|----------|
| Local GPU dispatch | < 1ms overhead | P100 |
| LAN peer offload | < 50ms round-trip | 1Gbps Ethernet |
| WAN peer offload | < 200ms round-trip | 1Gbps Internet via Tailscale |
| Shader hot-reload | < 100ms | Any |
| Binary self-replace | < 1s | Any |
| Pipeline overlap | 80%+ latency hidden | Multi-device |
| DDR3 compression | 4:1 ratio (FP32→INT8) | Preprocessor nodes |

## GPU Metrics Endpoint (nvml-wrapper)

The Master exposes real-time GPU metrics via `/metrics` for the n8n watchdog to poll. Uses `nvml-wrapper` instead of shelling out to `nvidia-smi`:

```rust
use nvml_wrapper::Nvml;

#[derive(Serialize)]
pub struct GpuMetric {
    pub gpu_index: u32,
    pub utilization_pct: u32,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub temperature_c: u32,
    pub pcie_tx_kbps: u32,
    pub pcie_rx_kbps: u32,
    pub power_draw_w: u32,
}

pub fn get_gpu_metrics() -> Vec<GpuMetric> {
    let nvml = Nvml::init().unwrap();
    let device_count = nvml.device_count().unwrap();
    (0..device_count).map(|i| {
        let dev = nvml.device_by_index(i).unwrap();
        let util = dev.utilization_rates().unwrap();
        let mem = dev.memory_info().unwrap();
        GpuMetric {
            gpu_index: i,
            utilization_pct: util.gpu,
            memory_used_mb: mem.used / (1024 * 1024),
            memory_total_mb: mem.total / (1024 * 1024),
            temperature_c: dev.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu).unwrap(),
            pcie_tx_kbps: dev.pcie_throughput(nvml_wrapper::enum_wrappers::device::PcieUtilCounter::Send).unwrap_or(0),
            pcie_rx_kbps: dev.pcie_throughput(nvml_wrapper::enum_wrappers::device::PcieUtilCounter::Receive).unwrap_or(0),
            power_draw_w: dev.power_usage().unwrap_or(0) / 1000,
        }
    }).collect()
}
```

**n8n Watchdog Decision Logic:**
- If `pcie_tx_kbps` is high but `utilization_pct` is low → NUMA imbalance detected → `POST /repin/:socket_id`
- If `memory_used_mb` > 90% of `memory_total_mb` → alert, consider offloading to remote peer
- If `temperature_c` > 85 → throttle dispatches, alert operator

## Existing Code Reuse Map (for 35B Implementation)

The following modules from `cesarops-hybrid-engine` and `cesarops-aeromagnetic-worker` provide proven patterns that warp-grid MUST reuse (not rewrite):

| Existing Module | Reuse In | What to Keep |
|----------------|----------|--------------|
| `cluster.rs` → `HybridClusterCoordinator` | `pool.rs` | Device enumeration, buffer pre-allocation, zero-copy role flip |
| `scheduler.rs` → `route_task!` macro | `scheduler.rs` | Compile-time task routing by hardware capability |
| `fdct_kernels.rs` → `XeonCpuBackend` | `backends/avx512.rs` | Rayon chunk-8 pattern for AVX-512 vectorization |
| `fdct_kernels.rs` → `P100GpuBackend` | `backends/wgpu_backend.rs` | Pipeline creation, buffer init, dispatch pattern |
| `spatial_engine.rs` → `NauticusPipeline` | `kernel_forge.rs` | Shader module loading, bind group layout, pipeline creation |
| `spatial_engine.rs` → `extract_and_send_anomalies` | `seti/quic.rs` | Async buffer readback + network streaming pattern |
| `nauticus_scanner.wgsl` | `shaders/pascal/` | 32×32 workgroup, atomicAdd sparse reduction, dipole scoring |

### Gaps to Fill (New Code Required)

1. **FP16 Half2 shader variant** — existing shaders are all f32. Need a `shaders/pascal/matmul_half2.wgsl` that uses `f16` types with `enable f16;` WGSL extension for P100 2:1 throughput. The existing dipole scanner should get a half-precision variant for the scoring math (where FP16 precision is sufficient).

2. **NUMA pinning** — existing code uses `wgpu::Instance::default()` which doesn't control thread affinity. Need `hwloc2` integration to pin GPU-feeding threads to the correct socket. The `HybridClusterCoordinator::init()` should be extended with NUMA awareness.

3. **Multi-node networking** — existing code uses raw TCP (`tokio::net::TcpStream`) for supervisor communication. warp-grid needs quinn QUIC for multiplexed, encrypted, low-latency peer-to-peer. The `extract_and_send_anomalies` pattern (length-prefixed frames) maps cleanly to QUIC streams.

4. **Peer discovery** — existing code hardcodes the supervisor address. Need mDNS + Tailscale peer list for auto-discovery.

5. **K-line guard** — no access control exists in current code. Any peer can submit work. Need the `KLineGuard` HashSet check before accepting QUIC connections.

6. **Shader variant selection** — existing code loads ONE shader per pipeline. The Kernel Forge needs to select between pascal/turing/generic variants based on the target device's SM version at dispatch time.

7. **WASM sandbox** — existing code trusts all shader payloads. Remote kernels from untrusted peers need wasmtime sandboxing.

8. **Pipeline parallelism** — existing code dispatches one tile at a time, waits for completion, then sends results. Need double-buffered streaming where Device B starts receiving while Device A is still computing.

9. **nvml-wrapper metrics** — existing code doesn't monitor GPU health. Need the `/metrics` endpoint for n8n watchdog integration.

10. **self-replace** — existing binaries are static. Need atomic hot-swap capability for live updates without service restart.

### BIOS Prerequisite

**CRITICAL**: The T440 BIOS must have "Node Interleaving" DISABLED for NUMA pinning to work. If enabled, the BIOS stripes memory across both sockets (destroying locality). Verify with:
```bash
numactl --hardware  # Should show 2 nodes with separate memory ranges
```
If it shows 1 node with all 94GB, Node Interleaving is ON → enter BIOS and disable it.

## Security Model

### Single Entry Point Architecture

The T440 (Master) is the ONLY node that accepts external QUIC connections. Sub-nodes (cesarops2, cesarops3) are "dumb workers" — they only accept tasks forwarded from the Master's IP. This provides:

- **VRAM Protection**: Older cards (1070, P1000, P106) on slower PCIe/DDR3 buses never see unscreened payloads. The Master validates SPIR-V on a P100 first.
- **Centralized Logging**: One kill log, one place to audit who got banned.
- **Simple Sub-Node Logic**: Workers only trust the Master's Tailscale IP. No K-line logic needed on sub-nodes.

```
External Peers (QUIC) → T440 Master (KLineGuard + SPIR-V validation)
                              │
                              ├── P100 local (validated tasks)
                              ├── cesarops2 (1070/P1000) — Master-forwarded only
                              └── cesarops3 (P106) — Master-forwarded only
```

### K-Line Guard (seti/guard.rs)

IRC-style peer banning with three severity levels, enforced at the Master gate:

```rust
use std::collections::HashSet;
use std::net::IpAddr;
use std::path::Path;
use std::collections::HashMap;

pub enum BanLevel {
    KLine,  // Local ban — Master refuses to forward tasks from this peer
    GLine,  // Global ban — broadcast to all trusted Masters in the WAN grid
    ZLine,  // Kernel-level firewall drop (nftables) — zero CPU cost after set
}

pub struct BanEvent {
    pub peer_id: String,
    pub ip: Option<IpAddr>,
    pub level: BanLevel,
    pub reason: String,
    pub timestamp: u64,
}

pub struct KLineGuard {
    banned_peers: HashSet<String>,      // Peer IDs (K-line)
    banned_ips: HashSet<IpAddr>,        // IP addresses
    trust_scores: HashMap<String, f32>, // Peer ID → trust score (0.0–1.0)
    ban_log: Vec<BanEvent>,             // Audit trail (centralized on Master)
}

impl KLineGuard {
    /// Check BEFORE QUIC handshake completes — banned peers never touch GPU resources
    pub fn is_killed(&self, peer_id: &str, remote_ip: &IpAddr) -> bool {
        self.banned_peers.contains(peer_id) || self.banned_ips.contains(remote_ip)
    }

    /// K-line: Master refuses to forward any further tasks from this peer
    /// Sub-nodes never see the banned peer's payloads
    pub fn add_kline(&mut self, peer_id: String, reason: &str);

    /// G-line: Propagates to ALL peer Masters in the WAN grid within 5 seconds
    /// Used when a peer is confirmed malicious (not just buggy)
    pub async fn add_gline(&mut self, peer_id: String, reason: &str, peers: &PeerNetwork);

    /// Z-line: Kernel-level packet drop via nftables/iptables
    /// Zero CPU cost after set — packets never reach userspace
    pub fn add_zline(&mut self, ip: IpAddr) -> Result<(), std::io::Error>;

    /// Update trust score after task completion/failure
    pub fn record_outcome(&mut self, peer_id: &str, success: bool);

    /// Auto-ban peers below trust threshold (default: 0.2)
    pub fn enforce_trust_threshold(&mut self, threshold: f32);

    /// Persist to disk — survives binary restart and self-replace hot-swap
    pub fn save(&self, path: &Path) -> Result<(), std::io::Error>;
    pub fn load(path: &Path) -> Result<Self, std::io::Error>;
}
```

### Admin CLI Commands (Master only)

```
kline <peer_id> <reason>     — Immediate local ban, drops QUIC stream
gline <peer_id> <reason>     — Global ban, propagates to WAN peers
zline <ip>                   — Kernel firewall drop (nftables rule)
unkline <peer_id>            — Remove local ban
trust <peer_id>              — Show current trust score
banlist                      — Dump all active bans (JSON)
repin <socket_id>            — Re-pin worker threads to target NUMA socket
```

Available via:
- HTTP API: `POST /kline`, `POST /gline`, `GET /banlist`, `POST /repin/:socket_id`
- CLI: `warp-grid kline <peer_id> <reason>`

### Grid Admin HTTP Router (Axum)

The Master exposes a single axum endpoint for remote management. The n8n watchdog agent polls P100 utilization and triggers `/repin` if it detects cross-socket latency spikes (UPI penalty):

```rust
use axum::{routing::{post, get}, extract::Path, Json, Router};

pub fn grid_admin_router() -> Router {
    Router::new()
        .route("/kline", post(handle_kline))
        .route("/gline", post(handle_gline))
        .route("/banlist", get(handle_banlist))
        .route("/repin/:socket_id", post(handle_repin))
        .route("/health", get(handle_health))
        .route("/registry", get(handle_registry))
}

/// n8n calls POST /repin/1 when it detects Socket 1's P100 being fed
/// by threads on Socket 0 (UPI penalty detected via latency spike)
async fn handle_repin(Path(socket_id): Path<u32>) -> &'static str {
    tokio::task::spawn_blocking(move || {
        crate::perf::pin_worker_to_socket(socket_id);
    }).await.unwrap();

    "Socket alignment synchronized."
}

/// n8n calls POST /kline with { "peer_id": "0xABC", "reason": "Fuzzing_Detected" }
async fn handle_kline(Json(req): Json<KLineRequest>) -> &'static str {
    // Updates the KLineGuard HashSet, drops active QUIC stream
    "K-line set."
}
```

**n8n Watchdog Integration:**
1. n8n polls `GET /health` every 10s — checks GPU utilization, memory pressure, socket balance
2. If P100 #1 latency > 1.3× P100 #0 latency → cross-socket detected → `POST /repin/1`
3. If peer failure count hits 3 → n8n calls `POST /kline` with peer ID and reason
4. If peer failure count hits 5 → n8n escalates to `POST /gline` (global ban)
5. All actions logged to tracing + persisted to `~/.warp-grid/admin.log`

### Auto-Ban Triggers (Watchdog Agent)

| Trigger | Action | Escalation |
|---------|--------|------------|
| Malformed SPIR-V payload | Immediate K-line | G-line on 2nd offense |
| Resource hogging (>90% VRAM, >60s, no completion) | K-line + alert | — |
| 3 consecutive task failures | K-line | G-line on 5th failure |
| Latency spike >5× baseline (3 consecutive) | K-line | — |
| Trust score below 0.2 | Auto G-line | Z-line if score hits 0.0 |
| naga panic on shader validation | Immediate K-line | G-line + Z-line |

**Strike Escalation Flow:**
```
Strike 1: Warning logged to tracing (visible to n8n dashboard)
Strike 2: Warning logged, peer trust score decremented by 0.2
Strike 3: K-LINE issued — Master drops QUIC stream, refuses to forward tasks
           Quinn transport rejects peer's handshake at UDP level
Strike 5: G-LINE issued — broadcast to all WAN Masters
Strike 7: Z-LINE issued — nftables rule drops packets at kernel level
```

**SPIR-V Screening Decision:**
- SM 6.0 target (P100): Load shader with `StorageBuffer16BitAccess` + `Float16` capabilities → high-throughput FP16 mode
- Generic/unknown target: Default to branchless f32 path → prevents K-line triggers from numerical precision errors on nodes that can't handle FP16

### SPIR-V Screening (Master Gate)

Before forwarding ANY task to sub-nodes, the Master:
1. Deserializes the SPIR-V payload
2. Runs `naga::valid::Validator` on the module — catches malformed shaders
3. Checks resource limits (workgroup size, buffer bindings) against target device caps
4. Only if validation passes → forwards to the appropriate sub-node

This means a bad shader crashes nothing. The P100 validates, the 1070/P1000 only ever see clean payloads.

### G-Line Propagation (WAN Grid)

For multi-Master deployments (e.g., friend's 2× P100 as a remote Master):
1. Master detects abuse → issues local K-line
2. Broadcasts `GLineEvent { peer_id, reason, timestamp, issuer_id }` via QUIC datagram
3. Remote Masters add the K-line locally within 5 seconds
4. All Masters persist to `~/.warp-grid/bans.json`
5. On binary restart (or self-replace hot-swap), bans reload from disk

### General Security

- **QUIC connections**: Mutual TLS with Tailscale-derived certificates (or `--cluster-key` shared secret)
- **Remote kernels**: WASM sandbox (wasmtime), no filesystem/network access
- **Peer trust**: Score-based — successful completions increase score, failures decrease
- **Sub-node isolation**: Workers only accept connections from Master's Tailscale IP
- **WebGPU peers**: Browser sandbox provides isolation; treated as "best effort" (untrusted tier)
- **Persistence**: All ban state survives restarts via JSON on disk
