# Warp-Grid: Implementation Tasks

## Execution Model

Each task follows a three-phase loop for the MoE (Qwen3.6-35B on P100s):

1. **Research** — nautivecs injects relevant code context + MoE searches for specific technical details
2. **Implement** — MoE writes code grounded in existing `cesarops-hybrid-engine` style
3. **Validate** — `cargo check` must pass before proceeding to next task

**Golden Style Guide** (inject via nautivecs before each task):
- `cesarops-hybrid-engine/src/cluster.rs` — buffer management, device init
- `cesarops-hybrid-engine/src/scheduler.rs` — route_task! macro, enum dispatch
- `cesarops-hybrid-engine/src/fdct_kernels.rs` — dual-backend trait pattern
- `cesarops-hybrid-engine/src/spatial_engine.rs` — pipeline creation, dispatch, readback
- `.kiro/steering/rust-codegen-corrections.md` — LLM error patterns to avoid
- `.kiro/steering/warp-grid-hardware-constraints.md` — P100/Xeon/NUMA constraints

**Rule**: Each task MUST compile before the next begins. No forward references.

---

## Phase 1: Hardware-Affinity & NUMA (The Foundation)

Everything else is pointless if memory is crossing the UPI interconnect.

- [ ] 1. Core types and Cargo.toml

  - [ ] 1.1 **Implement**: Create `warp-grid/Cargo.toml` with all dependencies pinned:
    - wgpu 29, naga 29, quinn 0.11, axum 0.8, tokio 1, serde 1, serde_json 1, bincode 1, ndarray 0.16, wasmtime 25, mdns-sd 0.11, hwloc2 0.5, nvml-wrapper 0.10, half 2, bytemuck 1, rayon 1.10, self-replace 1, tracing 0.1, uuid 1
    - _Requirements: all_

  - [ ] 1.2 **Implement**: Create `warp-grid/src/types.rs`
    - `ComputeTask` — data size, precision (FP16/FP32/FP64/INT8), shader ref, execution params
    - `Tensor` — ndarray wrapper with dtype tag
    - `DeviceProfile` — name, arch, VRAM total/free, TFLOPS measured, bandwidth, utilization
    - `NvidiaArch` enum — Pascal(String), Volta(String), Turing(String), Unknown(String)
    - `GpuNode` — name, arch, sm_version: (u32, u32), features: Vec<&'static str>, vram_mb, bandwidth_gbps
    - `Hardware` enum — Pascal, Turing, Volta, Xeon, TPU, RemoteNode, WebGpu (enum dispatch, NO trait objects)
    - `BanLevel` enum — KLine, GLine, ZLine
    - `DispatchTier` enum — Specialist, Adaptive, Preprocessor
    - `Error` enum — DeviceNotFound, ShaderCompileFailed, QuicTimeout, KLined, VramExhausted, NumaMismatch
    - _Requirements: R1.1, R1.2, R8.1, R9.9_

  - [ ] 1.3 **Validate**: `cargo check` passes with types.rs and empty lib.rs

- [ ] 2. NUMA topology mapping

  - [ ] 2.1 **Research**: Search for "Skylake-SP UPI latency", PCIe bus-to-socket mapping via hwloc2. Run `numactl --hardware` via Command wrapper to verify 2 nodes visible.

  - [ ] 2.2 **Implement**: Create `warp-grid/src/numa.rs`
    - `NumaTopology::detect()` — use hwloc2 to discover socket count, core-to-socket mapping
    - Map PCIe bus addresses → socket via hwloc2 PCI object traversal (find which P100 is on which socket)
    - `pin_worker_to_socket(socket_id)` — bind current thread's CPU affinity + memory policy to target NUMA node (MEMBIND_STRICT)
    - `alloc_numa_local(gpu, size)` — allocate staging buffer on NUMA node closest to target GPU
    - Health check: verify "Node Interleaving" is DISABLED (if only 1 NUMA node detected → error + log)
    - Fallback: if hwloc2 unavailable or single-socket → no-op (graceful degradation)
    - _Requirements: R9.3, R9.4_

  - [ ] 2.3 **Implement**: Create `warp-grid/src/pool.rs`
    - `VirtualComputePool::new()` — enumerate adapters via `wgpu::Instance`, classify by SM version
    - Feature mapping: P100 → `["2:1_FP16", "HBM2"]`, 1070/P1000/P106 → `["Simulated_BF16"]`
    - CPU detection via `std::thread::available_parallelism()`
    - Wire NUMA: after enumeration, call `NumaTopology::detect()` and map each GPU to its socket
    - Pin GPU-feeding thread pools to correct sockets at startup
    - Real-time utilization tracking via `Arc<AtomicU64>` per device
    - Pattern: follow `HybridClusterCoordinator::init()` from cluster.rs
    - _Requirements: R1.1, R1.2, R1.5, R9.3_

  - [ ] 2.4 **Implement**: Create `warp-grid/src/lib.rs`
    - Public `WarpGrid` struct holding pool, scheduler (stub), kernel_forge (stub), peers (stub)
    - `WarpGrid::new()` async constructor — calls pool init + NUMA detect
    - `WarpGrid::dispatch(task)` signature (stub returns Error for now)
    - Module declarations for all planned modules
    - _Requirements: R1.1, R1.3_

  - [ ] 2.5 **Validate**: `cargo check`. Verify NUMA detection compiles. If on T440, run and confirm 2 sockets detected.

- [ ] 3. Scheduler with NUMA-aware dispatch

  - [ ] 3.1 **Implement**: Create `warp-grid/src/scheduler.rs`
    - `GridScheduler` struct with pool reference + NUMA topology
    - `analyze(task)` → compute cost: data_size × precision → estimated FLOPS
    - `best_fit()` → select device: match precision requirements, VRAM, utilization, NUMA locality
    - NUMA rule: prefer device on same socket as the calling thread's memory
    - Multi-device sharding: if task VRAM > single device, split across devices
    - `DispatchTier` selection: known hardware → Specialist, unknown → Adaptive
    - Pattern: extend `route_task!` macro with new Hardware variants
    - _Requirements: R1.3, R1.4, R1.5, R9.9_

  - [ ] 3.2 **Validate**: `cargo check`. Unit test: scheduler selects P100 for large FP16, Xeon for sequential FP64.

- [ ] 4. **CHECKPOINT** — Foundation verified
  - `cargo build` clean
  - NUMA topology detected (or graceful fallback on non-T440)
  - Pool enumerates local GPUs with correct SM classification

---

## Phase 2: Pascal "Half2" Specialization (The Speed)

This is where we beat native CUDA — architecture-aware shader selection.

- [ ] 5. Kernel Forge — JIT shader specialization

  - [ ] 5.1 **Research**: Find SPIR-V opcodes for 16-bit storage on SM 6.0 (`OpCapability Float16`, `SPV_KHR_16bit_storage`). Research naga 29 API for `enable f16;` WGSL support and shader validation.

  - [ ] 5.2 **Implement**: Create `warp-grid/src/kernel_forge.rs`
    - `KernelForge` struct — shader_cache: HashMap<(u64, NvidiaArch), CompiledShader>
    - `select_shader_variant(target: &DeviceProfile)` → ShaderPath:
      - SM 6.0 (P100): `shaders/pascal/` with Float16 + StorageBuffer16BitAccess
      - SM 6.1 (1070/P1000): `shaders/generic/` branchless f32 (NEVER FP16 — 1:64 ratio)
      - SM 7.5 (Turing): `shaders/turing/` with Tensor Core hints
    - `validate_p100_shader(wgsl_source)` — check for `enable f16;`, estimate register pressure (>20 vars → reject)
    - Cache: HashMap keyed by (shader_hash, target_arch), invalidate on file change
    - _Requirements: R2.1, R2.2, R2.3, R2.6_

  - [ ] 5.3 **Implement**: Create `warp-grid/shaders/pascal/matmul_half2.wgsl`
    - `enable f16;` header
    - Storage buffers typed as `array<vec2<f16>>`
    - Pack adjacent tensor elements into vec2<f16> for HFMA2
    - Workgroup size 64 (32 threads × 2 for Pascal warp)
    - FMA: `packed[id] = fma(packed[id], factor, packed[id])`
    - Keep local variables ≤ 20 (register pressure rule)
    - _Requirements: R2.2, R9.1_

  - [ ] 5.4 **Implement**: Create `warp-grid/shaders/generic/matmul_f32.wgsl`
    - Standard f32, branchless, no precision-dependent behavior
    - Workgroup size 64
    - _Requirements: R2.6, R9.2_

  - [ ] 5.5 **Validate**: `cargo check`. Test: SM 6.0 → pascal path, SM 6.1 → generic path.

- [ ] 6. GPU backend (wgpu)

  - [ ] 6.1 **Implement**: Create `warp-grid/src/backends/mod.rs`
    - `Backend` enum — Wgpu(WgpuBackend), Avx512(Avx512Backend), WebGpu(WebGpuBackend)
    - `Backend::execute(task)` via match arms (enum dispatch, NO trait objects)
    - _Requirements: R1.3, R2.6_

  - [ ] 6.2 **Implement**: Create `warp-grid/src/backends/wgpu_backend.rs`
    - `WgpuBackend` — device, queue, pipeline_cache
    - `execute(task)` — load shader via kernel_forge, create pipeline, buffers, dispatch, readback
    - Staging buffer pattern from `aeromagnetic-worker/main.rs` (MAP_READ + COPY_DST)
    - Async buffer mapping with flume channel
    - NUMA-aware: allocate staging on correct socket via `numa.rs`
    - _Requirements: R1.3, R2.1, R2.2_

  - [ ] 6.3 **Validate**: `cargo check`. Test: dispatch simple addition shader on local GPU.

- [ ] 7. CPU backend (AVX-512) with throttle guard

  - [ ] 7.1 **Research**: Search for `_mm512_fmadd_ps`, `_mm512_cvtph_ps` (FP16↔FP32). Research AVX-512 frequency penalty on Xeon Silver (9-core threshold).

  - [ ] 7.2 **Implement**: Create `warp-grid/src/backends/avx512.rs`
    - `Avx512Backend` struct
    - Runtime detection: `is_x86_feature_detected!("avx512f")`
    - **Throttle guard**: dedicated rayon pool with `num_threads(8)` — NEVER exceed 8 cores doing AVX-512
    - Vectorized FP32: `_mm512_fmadd_ps` (16 floats per cycle)
    - FP16↔FP32 conversion: `_mm512_cvtph_ps` / `_mm512_cvtps_ph`
    - Chunk size 16 (512 bits / 32 bits = 16 elements per vector)
    - Drift correction path: use AVX2 `_mm256_fmadd_ps` (no frequency penalty)
    - Fallback: scalar if AVX-512 not detected
    - _Requirements: R2.4, steering (AVX-512 throttle rules)_

  - [ ] 7.3 **Validate**: `cargo check`. Test: FP32 vector ops correct. Verify rayon pool capped at 8.

- [ ] 8. **CHECKPOINT** — Local dispatch working end-to-end
  - Submit ComputeTask → scheduler selects device → kernel forge picks shader → backend executes → result returned
  - P100 gets half2 shader, 1070 gets f32, Xeon gets AVX-512 (8 threads max)

---

## Phase 3: Quinn & Cycle Stealing (The Distributive Flow)

- [ ] 9. QUIC transport and peer discovery

  - [ ] 9.1 **Research**: Search for "QUIC 0-RTT session resumption" in quinn. Goal: cesarops3 reconnects instantly without full handshake. Search quinn API for multiplexed streams + unreliable datagrams.

  - [ ] 9.2 **Implement**: Create `warp-grid/src/seti/mod.rs`
    - `PeerNetwork` struct — active connections, peer registry
    - `SetiConfig` — cluster_key, listen_addr, discovery_mode
    - Module declarations: quic, discovery, guard, sandbox
    - _Requirements: R3.1, R3.8_

  - [ ] 9.3 **Implement**: Create `warp-grid/src/seti/quic.rs`
    - `GridPeer` with quinn Endpoint
    - `start_node(addr)` — TLS 1.3, self-signed certs, 0-RTT session tickets
    - `offload_task(peer, task)` — serialize via bincode, send on bi-directional stream
    - Multiplexed: tensor data stream 0, control/metadata stream 1
    - Unreliable datagrams for intermediate aeromagnetic tile previews (fire-and-forget)
    - 0-RTT: store session tickets so cesarops3 resumes without full handshake
    - `--cluster-key` shared secret auth
    - Pattern: length-prefixed frames like `extract_and_send_anomalies`
    - _Requirements: R3.1, R3.2, R3.3, R3.5, R3.6, R3.8_

  - [ ] 9.4 **Implement**: Create `warp-grid/src/seti/discovery.rs`
    - mDNS: register `_warp-grid._udp.local` with TFLOPS metadata
    - Tailscale: parse `tailscale status --json` for peer IPs
    - Auto-connect within 5 seconds
    - _Requirements: R3.7, R6.4_

  - [ ] 9.5 **Validate**: `cargo check`. Test: loopback endpoint connects to itself.

- [ ] 10. Hardware registry and metrics

  - [ ] 10.1 **Implement**: Create `warp-grid/src/registry.rs`
    - `HardwareRegistry` — local + remote device profiles
    - TFLOPS microbenchmark at startup (short shader dispatch, measure wall time)
    - DDR3 detection: bandwidth < 30 GB/s → preprocessor role
    - Serialize as bincode for QUIC broadcast (every 10s)
    - Offline detection: 30s timeout → redistribute tasks
    - _Requirements: R6.1–R6.5, R5.1, R5.5_

  - [ ] 10.2 **Implement**: Create `warp-grid/src/metrics.rs`
    - `get_gpu_metrics()` via nvml-wrapper — utilization, VRAM, temp, PCIe TX/RX, power
    - n8n signal: high PCIe TX + low GPU util → NUMA imbalance
    - _Requirements: R9 (watchdog)_

  - [ ] 10.3 **Validate**: `cargo check`. Test: registry serialization round-trip.

- [ ] 11. **CHECKPOINT** — Networked cluster operational
  - Two nodes connect via QUIC
  - Registry broadcasts every 10s
  - Metrics endpoint returns real GPU data

---

## Phase 4: The Supervisory Layer (The Guard)

- [ ] 12. K-line/G-line peer banning

  - [ ] 12.1 **Implement**: Create `warp-grid/src/seti/guard.rs`
    - `KLineGuard` — banned_peers: HashSet, banned_ips: HashSet, trust_scores: HashMap, ban_log: Vec
    - `is_killed(peer_id, ip)` — O(1) check BEFORE QUIC handshake
    - `add_kline` / `add_gline` / `add_zline` (nftables shell-out)
    - `record_outcome(peer_id, success)` — trust score increment/decrement
    - `enforce_trust_threshold(0.2)` — auto-K-line below threshold
    - Strike escalation: 1-2 warn, 3 K-line, 5 G-line, 7 Z-line
    - `save`/`load` — JSON persistence to `~/.warp-grid/bans.json`
    - _Requirements: R8.1–R8.9_

  - [ ] 12.2 **Implement**: Integrate K-line into QUIC accept path
    - Call `guard.is_killed()` before accepting connection
    - Reject at UDP level — banned peers never touch GPU
    - Wire: malformed SPIR-V detection → strike increment
    - _Requirements: R8.2, R8.6_

  - [ ] 12.3 **Validate**: `cargo check`. Test: K-line blocks peer. Trust decay triggers auto-ban.

- [ ] 13. Admin HTTP API (Axum)

  - [ ] 13.1 **Implement**: Create `warp-grid/src/admin.rs`
    - Axum router: `POST /kline`, `POST /gline`, `GET /banlist`, `POST /repin/:socket_id`, `GET /health`, `GET /registry`, `GET /metrics`
    - `/repin/:socket_id` → calls `numa::pin_worker_to_socket()` (n8n triggers on imbalance)
    - `/kline` → calls `guard.add_kline()`, drops active QUIC stream
    - `/metrics` → calls `metrics::get_gpu_metrics()`
    - _Requirements: R8.9_

  - [ ] 13.2 **Validate**: `cargo check`. Test: curl endpoints respond.

- [ ] 14. WASM sandbox

  - [ ] 14.1 **Implement**: Create `warp-grid/src/seti/sandbox.rs`
    - wasmtime execution: no filesystem, no network, memory cap
    - Timeout: kill after 30s (configurable)
    - Failure → increment peer strike count in KLineGuard
    - _Requirements: R3.4_

  - [ ] 14.2 **Validate**: `cargo check`. Test: sandbox rejects filesystem access.

- [ ] 15. **CHECKPOINT** — Secure networked cluster
  - K-line blocks banned peers before handshake
  - Admin API responds to curl
  - WASM sandbox isolates remote kernels

---

## Phase 5: Aeromagnetic Synthesis (The Finish)

- [ ] 16. Pipeline parallelism and nautivecs bridge

  - [ ] 16.1 **Research**: Search for double-buffering patterns. Inject `cluster.rs` → `page_out_llm_and_stage_spatial` as reference.

  - [ ] 16.2 **Implement**: Create `warp-grid/src/pipeline.rs`
    - Decompose multi-stage tasks into pipeline graph
    - Double-buffer: Device A computes Stage N while streaming bf16 to Device B for Stage N+1
    - PCIe: wgpu staging buffers (zero-copy, NUMA-local)
    - Network: QUIC unreliable datagrams for intermediate results
    - Optimal depth: `min(device_count, stages) where compute_time > transfer_time`
    - Target: hide 80%+ of network latency
    - _Requirements: R4.1–R4.6_

  - [ ] 16.3 **Implement**: nautivecs → warp-grid bridge
    - Ensure `route_task!` macro correctly routes:
      - `curvelet_forward()` → Xeon AVX-512 (FP64, sequential, 8-thread pool)
      - Dipole pixel sweep → P100 (f16 half2 shader)
      - `curvelet_inverse()` → Xeon AVX-512 (FP64, sequential)
    - NautiBuffer → wgpu::Buffer conversion with COPY_SRC flag
    - The P100 NEVER does curvelet math (no f64 in WGSL) — only parallel pixel sweep between forward/inverse
    - _Requirements: R2.4, R9 (nautivecs integration)_

  - [ ] 16.4 **Validate**: `cargo check`. Test: curvelet forward → P100 dipole → curvelet inverse pipeline compiles.

- [ ] 17. Self-replace and final wiring

  - [ ] 17.1 **Implement**: Add self-replace capability
    - `deploy_new_binary(path)` — atomic hot-swap via `self-replace` crate
    - `reload_shaders(arch)` — invalidate kernel forge cache, reload from disk
    - Triggered by agent via `POST /update` admin endpoint
    - _Requirements: design (Self-Healing Live Update)_

  - [ ] 17.2 **Implement**: Wire all modules into `WarpGrid::dispatch()` in lib.rs
    - Scheduler → Kernel Forge → Backend → Execution
    - Overflow to remote peers when local at capacity
    - Pipeline parallelism for multi-stage tasks
    - K-Line guard in peer acceptance path
    - Registry broadcast loop (tokio::spawn, every 10s)
    - _Requirements: R1.3, R1.4, R3.2, R4.3, R8.2_

  - [ ] 17.3 **Implement**: Low-memory DDR3 bridge
    - DDR3 detection from registry bandwidth (< 30 GB/s)
    - Auto "preprocessor" mode: compress to fp16/int8 before transfer
    - Scheduler NEVER sends large FP32 to DDR3 nodes
    - _Requirements: R5.1–R5.5_

  - [ ] 17.4 **Implement**: WebGPU peer backend
    - WebSocket via axum, SPIR-V → WGSL via naga
    - "Best effort" scheduling, backup tiles on local fast node
    - _Requirements: R7.1–R7.6_

  - [ ] 17.5 **Validate**: `cargo build --release` clean. Integration test: full dispatch path works.

- [ ] 18. **FINAL CHECKPOINT**
  - Full `cargo test` passes
  - Binary runs on T440 with both P100s detected, NUMA-pinned, profiled
  - Admin API live, K-line functional, metrics streaming
  - Ready for remote node connection

---

## Deployment Checklist

- [ ] Verify T440 BIOS: "Node Interleaving" DISABLED (`numactl --hardware` → 2 nodes)
- [ ] Deploy to T440 via scp + self-replace hot-swap
- [ ] Start: `./warp-grid --cluster-key <secret> --listen 0.0.0.0:9100`
- [ ] Verify P100 half2 shader loads (tracing output: "pascal/matmul_half2.wgsl")
- [ ] Verify NUMA pinning (htop: GPU-feeding threads on correct cores)
- [ ] Connect cesarops2: `./warp-grid --cluster-key <secret> --master 100.72.182.77:9100`
- [ ] Test K-line: `curl -X POST localhost:9100/kline -d '{"peer_id":"test","reason":"manual"}'`
- [ ] Wire n8n watchdog: poll GET /metrics every 60s, trigger /repin on imbalance
- [ ] Load 70B on Xeons (CPU-only Q4_K_M) for math verification pass on FP16 shaders

---

## MoE Cheat Sheet (inject into every prompt)

| Key | Value |
|-----|-------|
| P100 SM Version | 6.0 (requires Float16 + StorageBuffer16BitAccess) |
| 1070/P1000/P106 SM | 6.1 (NEVER use FP16 — 1:64 ratio, slower than f32) |
| Xeon 4110 ISA | AVX-512F (no native bf16; use Shift-16 hack) |
| AVX-512 core limit | 8 threads max (9+ drops chip to 1.4GHz) |
| P100 register limit | ≤32 per thread at full occupancy (≤20 named vars) |
| Latency target | <50ms LAN, <200ms WAN |
| K-line threshold | 3 strikes → K-line, 5 → G-line, 7 → Z-line |
| NUMA rule | Socket 0 feeds P100 #0, Socket 1 feeds P100 #1 |
| Curvelet math | ALWAYS on Xeon (f64), NEVER on GPU (no f64 in WGSL) |
| Style guide | enum dispatch, borrow-before-move, no trait objects for async |
