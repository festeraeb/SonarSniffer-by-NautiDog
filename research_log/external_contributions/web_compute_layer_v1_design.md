# Web Compute Layer v1 — WebGPU Worker Pool Architecture

Source: cluster, 2026-05-17. Response to operator's "browser remote
compute" framing.
Status: **REFERENCE — ARCHITECTURE ACCEPTED, deferred until Tier 1+2
land.** Cluster correctly self-flagged this as utilization amplifier
not latency reducer. Slot 11+ in queue, NOT tonight.

## Key correctness points (accepted)

- Server owns: KV cache, weights, decode state, scheduler
- Workers own: WebGPU device + transient buffers ONLY. No state.
- Workers compute: matvec, attention QK^T, reductions. NEVER KV access.
- Same WGSL ships to both native (Vulkan) and worker (WebGPU) — wgpu's
  cross-target compile is the lever here.
- WebSocket RPC, capability handshake, retry-on-native-GPU on disconnect.

## What the cluster got right

- "Kernel-level opportunistic offload, NOT model distribution" — exactly
  the niche I identified. Petals/cake/distributed-llama all do model
  sharding; this is different.
- Realistic perf claim: +10-25%, not 2×. Network-bound, useful only when
  native GPU is saturated (multi-model serving, batched workloads).
- Honest priority ranking they offered: KV prefix > speculative >
  diagnostics+UBO > queue scheduler > web compute. Matches our queue.

## What's missing / polish

- No mention of authentication. Anyone connecting to the WebSocket
  becomes a worker. For homelab: trust LAN. For internet exposure:
  shared-secret + signed jobs.
- Job result verification absent. A malicious worker could return
  wrong logits. Mitigation: server periodically reruns 1% of jobs on
  native GPU and compares; permaban workers with mismatch rate > N%.
- Bandwidth math missing. Q vector at hidden_dim=1536 fp16 = 3 KB
  per dispatch. Attention QK at head_dim=128 × pos=4096 × n_heads=12
  fp16 = ~12 MB per token. Worker WebSocket better do binary frames
  not base64 JSON, or this is dead before it starts.
- "matvec" in the kernel offload list is wrong target. Each matvec
  needs its weight slice — that's MB shipped per dispatch. Bandwidth
  kills it. Better targets: attention QK^T (compute-heavy, small
  inputs/outputs), softmax row reductions (tiny inputs), AV matmul
  (already-cached V), all per-head not per-layer.

## Cluster's offered next step

> "bindless descriptor + persistent pipeline graph runtime — removes
> 20-40% remaining Vulkan overhead"

This IS our wgpu_hal port direction. Same goal (collapse per-dispatch
overhead via pre-recorded command buffers + bindless), different
framing. Slot 10 in queue. Don't double-prompt for it.

## Decision

**Accept architecture, defer integration.** Web Compute Layer slot 11
in queue, after wgpu_hal port. Multi-week project minimum. Tonight's
prompts focus on Tier 1 (UBO + KV prefix + speculative) where the
real per-token gains live.

## Verbatim source preserved in chat history. End.
