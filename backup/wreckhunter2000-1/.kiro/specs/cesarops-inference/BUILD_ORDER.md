# cesarops-inference — Build Order Spec

## For Review by: Gemini + Captain + Kiro
## Then handed to: 35B MoE (writer) + Strand BF16 (auditor)

---

## Phase 1: Foundation (Get it compiling)

### File 1: `Cargo.toml`
- **Deps**: burn (0.16, features=["wgpu"]), burn-wgpu, half, bytemuck, memmap2, serde, serde_json, tokio (full), axum (0.8), tracing, thiserror
- **Path deps**: warp-grid = { path = "../warp-grid" }
- **Complexity**: Simple
- **Notes**: Pin exact versions. No llama-gguf yet (evaluate first). Start minimal.

### File 2: `src/lib.rs`
- **Exposes**: mod declarations for all modules
- **Deps**: None
- **Complexity**: Simple

### File 3: `src/hardware.rs` — IronProfile
- **Exposes**: `IronProfile`, `audit_system()`, `GpuNode`, `CpuNode`
- **Deps**: warp-grid (pool, numa, types)
- **What it does**: Enumerate GPUs via wgpu, detect AVX-512, read NUMA topology, classify devices
- **Complexity**: Medium
- **Builds on**: warp-grid/src/pool.rs, warp-grid/src/numa.rs

---

## Phase 2: Memory Layer (Load model weights)

### File 4: `src/loader.rs` — GGUF Parser + GridBuffer Mapping
- **Exposes**: `GgufLoader`, `ModelWeights`, `LayerWeights`
- **Deps**: memmap2, warp-grid (GridBuffer, DeviceLocation, Precision)
- **What it does**: mmap GGUF file from RAID, parse tensor metadata, map each tensor into a GridBuffer, shard MoE experts across GPUs
- **Complexity**: Hard
- **Key decision**: Use memmap2 for zero-copy file access. Parse GGUF header manually (format is well-documented). Don't load weights into RAM — mmap them and let the OS page-fault on demand.
- **Reference**: llama-gguf crate for format parsing, but we write our own GridBuffer integration

### File 5: `src/bridge.rs` — GridBuffer ↔ Burn Tensor
- **Exposes**: `GridBuffer::as_burn_tensor()`, `BurnTensor::to_grid_buffer()`
- **Deps**: burn, burn-wgpu, warp-grid (GridBuffer)
- **What it does**: Zero-copy conversion between GridBuffer (our memory abstraction) and Burn tensors. No re-allocation on HBM2.
- **Complexity**: Hard (requires understanding Burn's internal tensor primitives)
- **Key risk**: Burn may not expose low-level buffer access. May need unsafe. Check burn-wgpu source.

---

## Phase 3: Compute (Forward pass)

### File 6: `src/attention.rs` — Multi-Head Attention + RoPE
- **Exposes**: `multi_head_attention()`, `apply_rope()`
- **Deps**: burn, half
- **What it does**: Standard MHA with rotary positional embeddings. Uses matmul_half2.wgsl on P100, matmul_f32.wgsl on 1070.
- **Complexity**: Medium
- **Notes**: Register-heavy — keep 16x16 tiles in registers. Flash attention pattern if possible.

### File 7: `src/transformer.rs` — Universal Forward Pass
- **Exposes**: `UniversalTransformer`, `forward()`
- **Deps**: attention.rs, moe.rs, kv_cache.rs, bridge.rs, loader.rs
- **What it does**: Layer-by-layer forward pass. Embed → (Attention + KV Cache + MoE FFN) × N → Norm → LM Head → Logits
- **Complexity**: Hard
- **Notes**: This is the main loop. Each layer: attention (with KV), then FFN (with MoE routing).

### File 8: `src/moe.rs` — Mixture of Experts Routing
- **Exposes**: `MoeRouter`, `route_experts()`, `dispatch_cross_gpu()`
- **Deps**: burn, warp-grid (GridBuffer::migrate)
- **What it does**: Top-k expert selection per token. If expert is on other GPU, migrate via GridBuffer. Round-robin expert placement at load time.
- **Complexity**: Hard
- **Notes**: Qwen3.6 uses 128 experts, activates 8 per token. Shard 64 experts per P100.

---

## Phase 4: Intelligence (KV + Sampling)

### File 9: `src/kv_cache.rs` — Tiered KV Cache
- **Exposes**: `KvCacheManager`, `push()`, `get()`, `overflow_to_host()`, `prefetch_from_raid()`
- **Deps**: warp-grid (GridBuffer, DeviceLocation), memmap2
- **What it does**: Ring buffer in HBM2 (Tier 0). When full, oldest heads migrate to DDR4 (Tier 1). Cold entries mmap'd from RAID (Tier 3). NUMA-pinned: Socket 0 DDR4 for P100 #0's overflow.
- **Complexity**: Hard
- **Key insight**: DDR4 is the prefetch buffer. Background thread pre-loads next-needed KV from RAID into DDR4 before the GPU asks for it.

### File 10: `src/sampling.rs` — Sampler + Logit Control
- **Exposes**: `sample()`, `apply_temperature()`, `apply_top_p()`, `apply_rep_pen()`, `apply_logit_bias()`, `grammar_constrain()`
- **Deps**: None (pure Rust math)
- **What it does**: Temperature scaling, nucleus sampling, repetition penalty, stop sequences. CRITICAL: logit bias sets <think>/<\/think> to -infinity during action phase. Grammar constraint forces valid JSON for tool calls.
- **Complexity**: Medium
- **Notes**: This is where the "Think Killer" lives. No more nudges — the model physically cannot emit think tokens.

---

## Phase 5: Interface (Tokenizer + Server)

### File 11: `src/tokenizer.rs` — Qwen Tokenizer
- **Exposes**: `encode()`, `decode()`, `special_tokens()`
- **Deps**: tokenizers crate (HuggingFace) or tiktoken-rs
- **What it does**: String → token IDs, token IDs → String. Handles <|im_start|>, <|im_end|>, <tool_call>, </tool_call>.
- **Complexity**: Simple (use existing crate)

### File 12: `src/server.rs` — HTTP API (KoboldCPP Drop-in)
- **Exposes**: Axum routes: POST /api/v1/generate, GET /api/v1/model, GET /health
- **Deps**: axum, tokio, serde_json
- **What it does**: Same JSON format as KoboldCPP. forge-v2 doesn't need ANY changes. Drop-in replacement.
- **Complexity**: Simple
- **Notes**: Request: {"prompt", "max_length", "temperature", "top_p", "rep_pen", "stop_sequence"}. Response: {"results": [{"text": "..."}]}

---

## Build Dependencies (What blocks what)

```
lib.rs ← everything
hardware.rs ← loader.rs, transformer.rs
loader.rs ← bridge.rs, transformer.rs
bridge.rs ← attention.rs, transformer.rs, moe.rs
attention.rs ← transformer.rs
moe.rs ← transformer.rs
kv_cache.rs ← transformer.rs
sampling.rs ← server.rs
tokenizer.rs ← server.rs
transformer.rs ← server.rs
```

## Critical Path: hardware → loader → bridge → attention → transformer → server

---

## Speculative Decoding (Phase 6 — after basic inference works)

### File 13: `src/speculative.rs` — Draft + Verify
- **What it does**: 1070 runs a 1.5B draft model, proposes N tokens. P100s verify all N in one forward pass. Accept matching tokens, reject divergent ones. 2-3x speedup.
- **Complexity**: Medium (once basic inference works)
- **Deps**: A second model loaded on the 1070

---

## Success Criteria

1. `cargo check` passes on all files
2. Can load Qwen3.6-35B-A3B MXFP4 GGUF across both P100s
3. Single forward pass produces logits
4. Sampling produces tokens
5. Server responds to same API as KoboldCPP
6. forge-v2 works without changes (drop-in)
7. KV cache overflows to DDR4 at 32K+ context without crash
8. Generates at >= 15 tok/s (matching KoboldCPP baseline)

---

## Swap Procedure (When ready to test)

1. Stop KoboldCPP: `sudo systemctl stop koboldcpp`
2. Start cesarops-inference: `./target/release/cesarops-inference --model /codebase/models/Qwen3.6-35B-A3B-MXFP4_MOE.gguf --port 5001`
3. Test: `curl http://127.0.0.1:5001/health`
4. If works: forge-v2 automatically uses it (same port, same API)
5. If fails: restart KoboldCPP, iterate
