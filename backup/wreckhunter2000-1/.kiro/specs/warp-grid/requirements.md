# Warp-Grid: Distributed Heterogeneous Compute Runtime

## Vision

A Rust crate that treats an entire heterogeneous cluster as ONE virtual compute device. Automatic work-stealing, JIT shader specialization, and QUIC-based remote cycle stealing. Replaces the need for CUDA-specific code by abstracting all hardware behind a unified dispatch interface.

## Hardware Inventory (Target Devices)

| Device | Arch | Compute | Memory | Location | Role |
|--------|------|---------|--------|----------|------|
| 2× P100 16GB | Pascal SM_60 | FP16 2:1, FP64 | 32GB HBM2 (732 GB/s each) | T440 local | Heavy compute |
| GTX 1070 8GB | Pascal SM_61 | FP16 2:1 | 8GB GDDR5 | cesarops2 | Preprocessor |
| P106 6GB | Pascal SM_61 | FP16 2:1 | 6GB GDDR5 | cesarops3 | Preprocessor |
| P1000 4GB | Pascal SM_61 | FP16 | 4GB GDDR5 | cesarops2 | Light tasks |
| 2× Xeon 4110 | Skylake-SP | AVX-512 | 94GB DDR4 6-ch | T440 local | CPU compute |
| Coral Edge TPU | — | INT8 4 TOPS | — | T440 VM | Classification |
| Friend's 2× P100 | Pascal SM_60 | FP16 2:1 | 32GB HBM2 | Remote (QUIC) | Overflow |
| Future Turing cards | SM_75 | Tensor Cores | — | WebGPU peers | Donated cycles |

## Requirements

### R1: Virtual Compute Pool

**User Story:** As a developer, I want to submit a compute task and have it automatically dispatched to the best available hardware without knowing which device will execute it.

**Acceptance Criteria:**
1. `WarpGrid::new()` SHALL enumerate all local GPUs via `wgpu::Instance::enumerate_adapters()` and all local CPUs via `std::thread::available_parallelism()`
2. Each device SHALL be profiled at startup: TFLOPS (FP16/FP32/INT8), memory bandwidth, available VRAM, architecture features
3. `WarpGrid::dispatch(task)` SHALL analyze the task's compute cost and select the optimal device(s) based on: data size, precision requirements, current device utilization
4. Tasks that exceed a single device's VRAM SHALL be automatically sharded across multiple devices
5. The pool SHALL track device utilization in real-time and load-balance across idle devices

### R2: JIT Shader Specialization

**User Story:** As a compute engine, I want shaders to be specialized for each GPU architecture at runtime so that Pascal gets FP16 2:1 paths and Turing gets Tensor Core paths.

**Acceptance Criteria:**
1. The Kernel Forge SHALL pre-compile SPIR-V shader packs for each detected architecture (SM_60, SM_61, SM_75)
2. Pascal devices SHALL receive shaders with FP16 vectorized operations (2:1 throughput hack)
3. Turing/Volta devices SHALL receive shaders with Tensor Core wmma intrinsics via naga injection
4. Xeon devices SHALL receive AVX-512 kernels compiled via `std::arch::x86_64` intrinsics
5. The Coral TPU SHALL receive INT8-quantized TFLite models
6. Shader selection SHALL happen at dispatch time based on the target device, not at compile time

### R3: SETI Protocol (Remote Cycle Stealing via QUIC)

**User Story:** As a cluster operator, I want idle GPUs on remote machines to automatically join the compute pool and accept work over the network with sub-50ms latency.

**Acceptance Criteria:**
1. Remote nodes SHALL connect via `quinn` (QUIC) with TLS 1.3 encryption and zero-config certificate management
2. The master node SHALL broadcast available tasks; idle nodes SHALL "steal" tasks from the queue
3. Task payload SHALL be: SPIR-V kernel + serialized input buffer + execution parameters
4. Remote execution SHALL be sandboxed (WASM or process isolation) to prevent malicious kernels
5. Results SHALL stream back via QUIC multiplexed streams (tensor data on one stream, metadata on another)
6. Latency between task dispatch and result receipt SHALL be under 50ms for LAN peers and under 200ms for internet peers (1Gbps)
7. Nodes SHALL auto-discover via mDNS on LAN and via Tailscale peer list for WAN
8. A `--cluster-key` shared secret SHALL authenticate peers (same pattern as Cake)

### R4: Grid-Level Pipeline Parallelism

**User Story:** As a pipeline operator, I want Layer N+1 to begin processing on a remote device while Layer N is still executing locally, hiding network latency behind compute.

**Acceptance Criteria:**
1. Multi-layer tasks SHALL be decomposed into a pipeline where each stage can execute on a different device
2. While Device A processes Stage N, the output buffer SHALL be compressed (bf16/fp16) and streamed to Device B for Stage N+1
3. Pipeline scheduling SHALL overlap compute and transfer: Device B begins receiving while Device A is still computing
4. The scheduler SHALL calculate the optimal pipeline depth based on: compute time per stage, transfer time between devices, device count
5. PCIe transfers (same machine) SHALL use zero-copy staging buffers
6. Network transfers SHALL use QUIC unreliable datagrams for intermediate results where packet loss is acceptable

### R5: Low-Memory Mode (DDR3 Bridge)

**User Story:** As a heterogeneous cluster, I want older DDR3 machines to participate by preprocessing and compressing data before sending to high-bandwidth nodes.

**Acceptance Criteria:**
1. The crate SHALL detect DDR3 vs DDR4 memory bandwidth at startup
2. DDR3 nodes SHALL automatically operate in "preprocessor" mode: compress tensors to fp16/int8 before network transfer
3. GPUs on DDR3 nodes (1060/1070) SHALL handle data reduction tasks: downsampling, quantization, feature extraction
4. The scheduler SHALL NEVER send full-precision large tensors to DDR3 nodes (bandwidth bottleneck)
5. DDR3 nodes SHALL advertise their role as "preprocessor" in the peer registry

### R6: Hardware Registry and TFLOPS Broadcasting

**User Story:** As a cluster node, I want to broadcast my available compute capacity so the scheduler can make optimal placement decisions.

**Acceptance Criteria:**
1. Each node SHALL maintain a `HardwareRegistry` with: device name, architecture, VRAM total/free, TFLOPS (measured at startup via microbenchmark), current utilization %
2. The registry SHALL be broadcast to all peers every 10 seconds via QUIC datagram
3. The master scheduler SHALL maintain a global view of all cluster capacity
4. New nodes joining SHALL immediately receive the full registry and begin accepting work within 5 seconds
5. Nodes going offline SHALL be detected within 30 seconds and their tasks redistributed

### R7: WebGPU Peer Integration

**User Story:** As a web browser user, I want to donate my GPU cycles to the CesarOps grid via WebGPU, receiving SPIR-V kernels and executing them in a sandboxed environment.

**Acceptance Criteria:**
1. A WebGPU peer SHALL connect to the grid via WebSocket (QUIC not available in browsers)
2. The peer SHALL receive WGSL shaders (not SPIR-V — browser limitation) and input buffers
3. Execution SHALL be sandboxed by the browser's WebGPU implementation
4. Results SHALL stream back via the same WebSocket connection
5. The scheduler SHALL treat WebGPU peers as "best effort" (may disconnect at any time)
6. Turing GPUs accessed via WebGPU SHALL still benefit from Tensor Core paths in WGSL

### R8: K-Line Peer Banning (IRC-Style Access Control)

**User Story:** As a cluster operator, I want to instantly ban abusive or malfunctioning peers from my compute grid using IRC-style K-line/G-line semantics, so that malicious or broken nodes cannot waste my hardware resources.

**Acceptance Criteria:**
1. Each node SHALL maintain a local `KLineGuard` with a HashSet of banned Peer IDs and banned IP addresses
2. K-line checks SHALL execute BEFORE the QUIC handshake completes — banned peers never touch GPU resources
3. A K-line SHALL be local to the issuing node only (single-machine ban)
4. A G-line (Global line) SHALL propagate to ALL peers in the grid within 5 seconds via QUIC datagram broadcast
5. A Z-line SHALL trigger kernel-level packet drop (nftables/iptables) for the banned IP — zero CPU cost after set
6. The watchdog agent SHALL auto-issue K-lines when it detects: malformed SPIR-V payloads, resource hogging (>90% VRAM for >60s without completing), repeated task failures (3 consecutive), or latency spikes (>5× baseline)
7. K-line/G-line state SHALL persist to disk (JSON) and reload on binary restart
8. A `--trust-score` system SHALL track peer reliability: successful completions increase score, failures decrease it; peers below threshold get auto-K-lined
9. Manual K-line/G-line commands SHALL be available via the agent's HTTP API (`POST /kline`, `POST /gline`)

### R9: Hardware-Aware Performance Tiering

**User Story:** As a compute scheduler, I want to automatically select optimized execution paths for known hardware (P100 FP16 2:1, Xeon NUMA pinning) while gracefully falling back to generic paths for unknown guest nodes.

**Acceptance Criteria:**
1. P100 (SM 6.0) tasks SHALL use `f16vec2` vectorized WGSL shaders exploiting the 2:1 FP16 throughput ratio (~21 TFLOPS effective)
2. SM 6.1 devices (1070, P1000, P106) SHALL NOT use FP16 paths (1:64 ratio makes it slower than FP32) — fall back to FP32 shaders
3. On dual-socket Xeon systems, GPU-feeding threads SHALL be pinned to the NUMA node physically closest to the target GPU's PCIe slot
4. Staging buffers SHALL be allocated on the NUMA-local memory domain (eliminate UPI cross-socket penalty)
5. Unknown/guest nodes SHALL be probed at join time: FP16 ratio, memory bandwidth, NUMA topology, AVX-512 support
6. Nodes with measured bandwidth < 100 GB/s SHALL be auto-classified as "preprocessor" tier (compression/reduction only)
7. Work SHALL be sharded into atomic tiles: fast nodes get compute-dense center tiles, slow nodes get latency-tolerant edge tiles
8. WebGPU/guest tiles SHALL be duplicated on a local fast node as backup — if guest doesn't return within 2× expected time, backup result is used
9. The scheduler SHALL maintain a `DispatchTier` enum: `Specialist` (optimized paths), `Adaptive` (generic), `Preprocessor` (compression only)

## Crate Structure

```
warp-grid/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Public API: WarpGrid, dispatch(), ComputeTask
│   ├── pool.rs             # VirtualComputePool: device enumeration, profiling
│   ├── scheduler.rs        # Task analysis, device selection, load balancing
│   ├── kernel_forge.rs     # JIT shader compilation, architecture detection
│   ├── pipeline.rs         # Pipeline parallelism, stage decomposition
│   ├── seti/
│   │   ├── mod.rs          # SETI protocol coordinator
│   │   ├── quic.rs         # Quinn-based QUIC transport
│   │   ├── discovery.rs    # mDNS + Tailscale peer discovery
│   │   ├── sandbox.rs      # WASM sandboxed execution for remote kernels
│   │   └── guard.rs        # K-line/G-line/Z-line peer banning
│   ├── backends/
│   │   ├── mod.rs          # Backend trait
│   │   ├── wgpu_backend.rs # GPU execution via wgpu
│   │   ├── avx512.rs       # CPU execution via AVX-512 intrinsics
│   │   ├── tpu.rs          # Coral TPU execution via TFLite
│   │   └── webgpu.rs       # WebGPU peer execution
│   ├── registry.rs         # Hardware registry, TFLOPS broadcasting
│   └── types.rs            # ComputeTask, Tensor, DeviceProfile, etc.
├── shaders/
│   ├── pascal/             # SM_60/61 optimized WGSL
│   ├── turing/             # SM_75 with tensor core hints
│   └── generic/            # Fallback WGSL
└── examples/
    ├── local_dispatch.rs   # Single-machine heterogeneous dispatch
    ├── cluster_join.rs     # Join as a SETI worker node
    └── benchmark.rs        # Measure TFLOPS across all devices
```

## Dependencies

```toml
[dependencies]
wgpu = "29"
naga = "29"                    # Shader translation + injection
quinn = "0.11"                 # QUIC transport
axum = "0.8"                   # Admin HTTP API (repin, kline, health)
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"               # K-line persistence (bans.json)
bincode = "1"                  # Fast tensor serialization
ndarray = "0.16"               # Tensor operations
wasmtime = "25"                # WASM sandbox for remote kernels
mdns-sd = "0.11"               # mDNS peer discovery
hwloc2 = "0.5"                 # NUMA topology discovery + thread/memory pinning
nvml-wrapper = "0.10"          # GPU metrics without nvidia-smi overhead
tracing = "0.1"
uuid = { version = "1", features = ["v4"] }
half = "2"                     # FP16 type support for half2 packing
bytemuck = { version = "1", features = ["derive"] }  # Zero-copy buffer casting
rayon = "1.10"                 # CPU parallelism (AVX-512 chunk processing)
self-replace = "1"             # Atomic binary hot-swap
```

## Implementation Order (Plan-Execute Chunks)

1. **Task 1**: `types.rs` + `pool.rs` — Define ComputeTask, Tensor, DeviceProfile. Enumerate local devices.
2. **Task 2**: `scheduler.rs` — Analyze tasks, select devices, basic round-robin dispatch.
3. **Task 3**: `backends/wgpu_backend.rs` — Execute WGSL shaders on local GPUs via wgpu.
4. **Task 4**: `backends/avx512.rs` — Execute compute on Xeons via AVX-512.
5. **Task 5**: `kernel_forge.rs` — Detect architecture, select shader variant.
6. **Task 6**: `registry.rs` — Hardware profiling, TFLOPS measurement, broadcast.
7. **Task 7**: `seti/quic.rs` + `seti/discovery.rs` — QUIC transport, peer discovery.
8. **Task 8**: `seti/sandbox.rs` — WASM sandboxed remote execution.
9. **Task 9**: `pipeline.rs` — Pipeline parallelism across devices.
10. **Task 10**: `backends/webgpu.rs` — WebGPU peer integration.

## Success Criteria

- A single `warp_grid.dispatch(task)` call automatically selects the best hardware
- Local heterogeneous dispatch (P100 + Xeon + TPU) works without manual device selection
- Remote nodes join via QUIC and accept work within 5 seconds
- Pipeline parallelism hides 80%+ of network latency between stages
- The crate compiles and runs on the existing CESAROPS cluster without CUDA dependency
