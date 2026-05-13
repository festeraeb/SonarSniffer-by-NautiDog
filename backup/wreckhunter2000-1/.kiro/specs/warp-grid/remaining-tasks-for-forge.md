# Remaining Tasks for Autonomous Forge Execution

## Priority 1: Forge Self-Improvement (must complete first)

- [ ] Fix forge project awareness: instead of writing to temp dir, detect existing Cargo.toml in target-dir and write INTO the existing crate structure. Run cargo check against the full project, not an isolated temp dir.
- [ ] Add the web serve mode: axum server on :9100 with embedded HTML, plan/spec/monitor endpoints. (cesarops-forge-web is the prototype — merge it into the main forge binary)
- [ ] Add spec mode state machine: Describe -> GenRequirements -> Approve -> GenDesign -> Approve -> GenTasks -> Done. Write output to .kiro/specs/<name>/ directory.
- [ ] Add credentials subcommand: `secrets audit` runs the inventory scanner, `secrets encrypt` encrypts credentials.sh with user passphrase (age or AES-256-GCM).

## Priority 2: Warp-Grid Implementation (Phase 2-5 from tasks.md)

- [ ] Task 3: Implement warp-grid/src/scheduler.rs - GridScheduler with NUMA-aware device selection, DispatchTier routing, multi-device sharding for large tasks
- [ ] Task 5: Implement warp-grid/src/kernel_forge.rs - JIT shader selection by SM version (SM 6.0 -> pascal half2, SM 6.1 -> generic f32, SM 7.5 -> turing tensor core)
- [ ] Task 5.3: Create warp-grid/shaders/pascal/matmul_half2.wgsl - FP16 shader with `enable f16;` for P100 2:1 throughput
- [ ] Task 5.4: Create warp-grid/shaders/generic/matmul_f32.wgsl - Standard f32 fallback for SM 6.1 cards
- [ ] Task 6: Implement warp-grid/src/backends/mod.rs + wgpu_backend.rs - GPU execution via wgpu 29, pipeline creation, buffer management, dispatch, readback
- [ ] Task 7: Implement warp-grid/src/backends/avx512.rs - CPU compute with 8-thread rayon pool cap (AVX-512 throttle guard), FP16<->FP32 conversion
- [ ] Task 9: Implement warp-grid/src/seti/quic.rs - Quinn QUIC transport with 0-RTT session resumption, multiplexed streams, cluster-key auth
- [ ] Task 9.4: Implement warp-grid/src/seti/discovery.rs - mDNS peer discovery + Tailscale status parsing
- [ ] Task 10: Implement warp-grid/src/seti/guard.rs - K-line/G-line/Z-line peer banning with trust scores, JSON persistence, strike escalation
- [ ] Task 13: Implement warp-grid/src/admin.rs - Axum admin API: /kline, /gline, /repin, /metrics, /health, /registry, /banlist
- [ ] Task 16: Implement warp-grid/src/pipeline.rs - Pipeline parallelism with double-buffered streaming between devices

## Priority 3: Self-Healing Supervisor (cesarops-watchdog)

- [ ] Implement cesarops-watchdog as a tiny Rust binary that manages process lifecycle on T440
- [ ] Manages: drive mounts, cloudflared, tailscale, samba, KoboldCPP, nautivecs, forge-web, code-server
- [ ] Uses self-replace for live binary updates
- [ ] Health checks every 30s, auto-restart on failure
- [ ] Exposes /health endpoint for the forge monitor mode to poll

## Priority 4: Detection Pipeline Integration

- [ ] Wire the triple-lock detection pipeline (scout -> validator -> TPU jitter -> reasoner) into warp-grid dispatch
- [ ] Integrate nauticuvs f64 curvelet forward/inverse into the warp-grid scheduler (route to Xeon AVX-512)
- [ ] Connect the Argo validation tiles (Lake Erie 2015) as test data for the full pipeline
- [ ] Implement drift correction via sub-pixel slice-stitch-replace on P100s using the half2 shaders

## Priority 5: Overnight Research Automation

- [ ] Implement research mode in the forge: search nautivecs + WSO for a topic, synthesize findings, store in research_log/
- [ ] Weather-driven scan scheduling: check NOAA buoy data, identify post-storm windows, queue tile downloads
- [ ] Autonomous tile download: when weather window detected, download Landsat/Sentinel tiles for target areas
- [ ] Temporal stack builder: accumulate 20+ days of tiles per target, tag with weather condition

---

## Execution Notes for the Forge

- Each task should follow: Research (nautivecs + WSO) -> Implement (35B) -> Review (8B) -> Fix -> cargo check
- Golden style guide: inject cesarops-hybrid-engine patterns before each task
- Hardware constraints: inject .kiro/steering/warp-grid-hardware-constraints.md
- All code writes to /codebase/wreckhunter2000-1/ on the RAID
- f64 is now the default precision for nauticuvs — curvelet math stays on Xeons
- P100 shaders use f16 (half2) for the parallel pixel sweep only
- AVX-512 limited to 8 threads max (frequency throttle guard)
- NUMA pinning: Socket 0 feeds P100 #0, Socket 1 feeds P100 #1
