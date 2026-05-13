# White Paper: Unified wgpu LLM + Spatial Compute Engine

## CESARops Internal — Future Development Reference

**Date:** 2026-05-07  
**Status:** Contingency design (implement if Cake/external tools prove inadequate)  
**Author:** CESARops Architecture Team

---

## Abstract

This document outlines a path to extend the existing `cesarops-hybrid-engine` — which already manages distributed wgpu compute across multiple GPUs for spatial anomaly detection — to also perform LLM inference using the same devices, shaders, and coordination infrastructure. The result is a single Rust binary that handles both satellite imagery analysis and natural language reasoning on any Vulkan-capable hardware, with zero external dependencies.

---

## Motivation

1. **No suitable off-the-shelf tool exists** that combines distributed LLM inference with distributed spatial compute on the same GPU pool without context-switching overhead.
2. **Field deployment** requires a single portable binary that runs on heterogeneous hardware (laptops, desktops, servers) without CUDA, Python, or framework dependencies.
3. **The infrastructure already exists** — `cesarops-hybrid-engine` manages multi-GPU wgpu devices, zero-copy buffer swaps, and TCP streaming. Adding LLM inference is an incremental extension, not a rewrite.
4. **Resource sharing** — the same P100/1070/1060 GPUs that run anomaly detection shaders can run LLM inference between scan passes, maximizing hardware utilization.

---

## Existing Infrastructure (Already Built)

| Component | Location | What It Does |
|-----------|----------|--------------|
| `HybridClusterCoordinator` | `cesarops-hybrid-engine/src/cluster.rs` | Enumerates GPUs, manages dual P100 devices, allocates buffers, flips between LLM and spatial roles |
| `P100Node` | `cesarops-hybrid-engine/src/cluster.rs` | Per-GPU struct with device, queue, pre-allocated staging buffers, role state |
| `NauticusPipeline` | `cesarops-hybrid-engine/src/spatial_engine.rs` | WGSL shader dispatch, workgroup management, anomaly extraction, TCP streaming |
| `P100GpuBackend` | `cesarops-hybrid-engine/src/fdct_kernels.rs` | Dipole scanner shader dispatch, 32×32 workgroups optimized for P100 SMs |
| Thermal/Optical/Aeromagnetic workers | `cesarops-slicer/src/` | Specialized wgpu compute passes with device enumeration and buffer management |
| `NodeDiscovery` | `sovereign-cloud/src/discovery.rs` | mDNS + Tailscale peer discovery, capability registration |
| `nautivecs` | `nautivecs/` | Vector store for context injection (already integrated) |

---

## What Needs Adding

### 1. WGSL Transformer Shaders (~8 files)

Source: Adapt from [wgpu-llm](https://github.com/Beledarian/wgpu-llm) (MIT/Apache-2.0 licensed, 12 standalone WGSL shaders).

| Shader | Purpose | Complexity |
|--------|---------|------------|
| `gemm.wgsl` | General matrix multiply (the core op) | High — needs tiled implementation for performance |
| `matvec.wgsl` | Matrix-vector multiply (decode phase) | Medium |
| `rmsnorm.wgsl` | RMS layer normalization | Low |
| `rope.wgsl` | Rotary position embedding | Medium |
| `silu.wgsl` | SiLU/SwiGLU activation | Low |
| `softmax.wgsl` | Attention softmax | Medium |
| `embedding.wgsl` | Token embedding lookup | Low |
| `sampling.wgsl` | Top-k/top-p token sampling | Low (can stay on CPU) |

**Estimated effort:** 2-3 days to adapt wgpu-llm's shaders to our buffer layout conventions.

### 2. Model Weight Loader

Parse GGUF or safetensors files into GPU buffers, one per transformer layer.

```rust
/// Load model weights into pre-allocated GPU buffers.
/// Assigns layers to GPUs based on available VRAM.
pub struct ModelLoader {
    /// Layer → GPU assignment (computed at load time)
    layer_assignment: Vec<(usize, usize)>,  // (layer_idx, gpu_idx)
}

impl ModelLoader {
    /// Load a GGUF model, distributing layers across available P100Nodes.
    pub async fn load_gguf(
        path: &Path,
        nodes: &[P100Node],
    ) -> Result<LoadedModel> {
        // 1. Parse GGUF header → get layer count, tensor shapes, quantization
        // 2. Calculate memory per layer
        // 3. Greedy-assign layers to GPUs (fill largest VRAM first)
        // 4. Upload weight tensors to assigned GPU buffers
        // 5. Return LoadedModel with layer→buffer mapping
    }
}
```

**Estimated effort:** 1-2 days. GGUF parsing crates exist (`gguf-rs`). Safetensors parsing is trivial (memory-mapped, zero-copy).

### 3. KV Cache Manager

Paged buffer allocation for key-value attention cache. Reuses the existing `spatial_staging_buffer` pattern.

```rust
/// Paged KV cache — allocates GPU pages lazily as sequence grows.
/// Reuses the same buffer pool as spatial compute (role-flipped).
pub struct KvCache {
    pages: Vec<wgpu::Buffer>,
    page_size: usize,
    sequence_length: usize,
    max_pages: usize,
}
```

**Estimated effort:** 1 day. The buffer management pattern already exists in `P100Node`.

### 4. Inference Orchestrator

Coordinates the forward pass across shaders and GPUs.

```rust
/// Execute one token generation step across distributed GPUs.
pub async fn generate_token(
    model: &LoadedModel,
    kv_cache: &mut KvCache,
    input_ids: &[u32],
    nodes: &[P100Node],
) -> u32 {
    // For each transformer layer:
    //   1. Determine which GPU holds this layer's weights
    //   2. If different from previous layer's GPU, transfer hidden state
    //   3. Dispatch attention shader (Q*K^T, softmax, *V)
    //   4. Dispatch FFN shader (up_proj, gate, down_proj)
    //   5. Dispatch RMSNorm shader
    // Final: dispatch lm_head projection, read back logits, sample
}
```

**Estimated effort:** 2-3 days. The dispatch pattern mirrors `NauticusPipeline::execute_pass`.

### 5. Tokenizer Integration

```rust
// Already available as a crate — zero custom code needed
use tokenizers::Tokenizer;
let tokenizer = Tokenizer::from_file("tokenizer.json")?;
let encoding = tokenizer.encode(prompt, true)?;
```

**Estimated effort:** 30 minutes.

---

## Architecture: Unified Engine

```
┌─────────────────────────────────────────────────────────────┐
│                  cesarops-hybrid-engine                       │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │           HybridClusterCoordinator                    │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │   │
│  │  │  P100 GPU 0 │  │  P100 GPU 1 │  │  1070 (net) │  │   │
│  │  │  Layers 0-15│  │  Layers 16-31│  │  Layers 32+ │  │   │
│  │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  │   │
│  │         │                 │                 │         │   │
│  │  ┌──────┴─────────────────┴─────────────────┴──────┐ │   │
│  │  │              Shared Buffer Pool                   │ │   │
│  │  │  (role-flipped: LLM weights ↔ spatial tiles)     │ │   │
│  │  └──────────────────────────────────────────────────┘ │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                              │
│  ┌────────────────────┐  ┌────────────────────────────────┐ │
│  │  LLM Mode          │  │  Spatial Mode                   │ │
│  │  ─────────         │  │  ────────────                   │ │
│  │  gemm.wgsl         │  │  nauticus_scan.wgsl             │ │
│  │  rmsnorm.wgsl      │  │  curvelet_filter.wgsl           │ │
│  │  rope.wgsl         │  │  dipole_scanner.wgsl            │ │
│  │  silu.wgsl         │  │  thermal_submersion.wgsl        │ │
│  │  softmax.wgsl      │  │  optical_structural.wgsl        │ │
│  │  attention.wgsl    │  │  spectral_analysis.wgsl         │ │
│  └────────────────────┘  └────────────────────────────────┘ │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │              Orchestrator (same binary)                │   │
│  │  - Accepts scan requests                              │   │
│  │  - Flips GPUs to LLM mode for threshold tuning        │   │
│  │  - Flips GPUs to spatial mode for anomaly detection    │   │
│  │  - Streams results over TCP                           │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

---

## Layer Distribution Algorithm

```
Given:
  - N transformer layers in the model
  - K GPUs with VRAM capacities [v0, v1, ..., vK-1]
  - Memory per layer: M bytes (from GGUF header)

Algorithm (greedy proportional):
  1. total_vram = sum(v0..vK-1)
  2. For each GPU i:
       layers_for_i = floor(N * vi / total_vram)
  3. Assign remaining layers to GPU with most free VRAM
  4. Result: layer_ranges = [(0, L0), (L0, L0+L1), ...]

Example (Qwen3-14B Q4, 40 layers):
  - P100 #0 (16GB): 40 * 16/28 = 22 layers → layers 0-21
  - P100 #1 (16GB): 40 * 16/28 = 22 layers → layers 22-39 (gets remainder)
  - (If 1070 8GB added): 40 * 8/40 = 8 layers redistributed

Example (Field: 3 laptops with 4GB each = 12GB):
  - Laptop A (4GB): 40 * 4/12 = 13 layers → layers 0-12
  - Laptop B (4GB): 40 * 4/12 = 13 layers → layers 13-25
  - Laptop C (4GB): 40 * 4/12 = 14 layers → layers 26-39
```

---

## Performance Expectations

### Single-node (P100 16GB, Vulkan)

Based on wgpu-llm benchmarks (26 tok/s on GTX 1070 for 1.1B):
- **1.1B model**: ~30-35 tok/s (P100 has higher memory bandwidth than 1070)
- **7B model**: ~8-12 tok/s (memory-bound, P100's 732 GB/s HBM2 helps)
- **14B model (dual P100)**: ~5-8 tok/s (inter-GPU transfer overhead)

### Distributed (3 laptops over WiFi)

- **14B model (3×4GB)**: ~2-4 tok/s (network latency between layers)
- **7B model (2×4GB)**: ~4-6 tok/s
- Still usable for single-shot structured decisions (threshold tuning takes 10-30s)

### Comparison to KoboldCPP

KoboldCPP achieves 94.7 tok/s on TinyLlama because:
1. CUDA kernels are hand-optimized over years
2. Flash attention reduces memory bandwidth
3. Quantization kernels are CUDA-specific

Our Vulkan path will be 3-4× slower for equivalent models. The tradeoff is portability and unified resource management. For the orchestrator use case (structured decisions, not streaming chat), this is acceptable.

---

## Inter-GPU Transfer Strategy

When a model is split across GPUs (local or networked):

### Same-machine (PCIe)
- Hidden state between layers: 1 × hidden_dim × sizeof(f16) = ~8KB per token for 7B
- PCIe Gen3 x16: 15.75 GB/s → 8KB transfer = 0.5μs (negligible)
- Already implemented in `P100Node` buffer staging

### Cross-machine (Tailscale/TCP)
- Same 8KB per token per layer boundary
- Tailscale LAN: ~1Gbps = 8KB in 0.06ms (negligible for single-token decode)
- WiFi: ~100Mbps = 8KB in 0.6ms (adds ~0.6ms per layer boundary)
- For 3 machines with 2 boundaries: +1.2ms per token → still fine at 5+ tok/s

### Optimization: Pipeline parallelism
- While GPU 1 processes layer N for token T, GPU 0 can process layer N-1 for token T+1
- Hides transfer latency behind compute
- Already conceptually present in `HybridClusterCoordinator`'s role-flip design

---

## Model Support Strategy

### Phase 1: Llama/Qwen family (covers 90% of use cases)
- Llama 2/3, Qwen 2.5/3, TinyLlama, Mistral, Gemma
- All share the same transformer architecture with minor variations
- Differences: RoPE base frequency, activation function (SiLU vs GELU), attention head count
- Parameterize the shaders, don't write separate ones per model

### Phase 2: GGUF quantization support
- Q4_K_M, Q5_K_M, Q8_0 (most common quantizations)
- Dequantization happens in the GEMM shader (fused, no separate pass)
- wgpu-llm already has INT8 block quantization working

### Phase 3: Vision encoders (future)
- For satellite tile classification directly on GPU
- SigLIP/CLIP vision encoder is just more transformer layers
- Same shaders, different weight shapes

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| WGSL GEMM too slow on Pascal | Medium | High | Use tiled GEMM with shared memory; P100 has 48KB shared per SM |
| KV cache exceeds VRAM on small GPUs | Low | Medium | Paged eviction to system RAM (already have CPU↔GPU transfer) |
| Quantized GEMM accuracy loss | Low | Low | Validate against KoboldCPP output for same model/prompt |
| Cross-machine latency kills throughput | Medium | Medium | Pipeline parallelism; batch prompts; accept lower tok/s for field use |
| Maintenance burden of custom engine | High | Medium | Keep shader count minimal; use wgpu-llm as reference; don't over-engineer |

---

## Decision Criteria: Build vs Use Cake

**Use Cake if:**
- Cake's Vulkan backend works reliably on our hardware
- Cake's distributed mode achieves acceptable tok/s (>10 for 8B model)
- Cake's API is stable enough to depend on
- We don't need simultaneous LLM + spatial compute on the same GPUs

**Build our own if:**
- Cake's Vulkan backend is buggy or slow on Pascal GPUs
- We need zero-copy role-flipping between LLM and spatial modes
- We need tighter integration with nautivecs (inject context mid-generation)
- Field deployment requires a single binary with no external model management
- We want to control the full stack for reproducibility and debugging

---

## Estimated Development Timeline

| Phase | Effort | Deliverable |
|-------|--------|-------------|
| Shader adaptation (from wgpu-llm) | 3 days | 8 WGSL files in `shaders/llm/` |
| Weight loader (GGUF) | 2 days | `src/model_loader.rs` |
| KV cache manager | 1 day | `src/kv_cache.rs` |
| Inference orchestrator | 3 days | `src/llm_engine.rs` |
| Integration with HybridClusterCoordinator | 2 days | Role-flip support |
| Distributed layer sharding (TCP) | 2 days | Cross-node inference |
| Testing + optimization | 3 days | Benchmark suite, shader tuning |
| **Total** | **~16 days** | Full custom LLM engine |

---

## References

- [wgpu-llm](https://github.com/Beledarian/wgpu-llm) — WGSL shader reference implementation (MIT/Apache-2.0)
- [Cake](https://github.com/evilsocket/cake) — Distributed LLM inference in Rust (FAIR License)
- [candle](https://github.com/huggingface/candle) — Rust ML framework (MIT/Apache-2.0)
- [GGUF spec](https://github.com/ggerganov/ggml/blob/master/docs/gguf.md) — Model format documentation
- [wgpu v29](https://wgpu.rs/) — WebGPU implementation used by CESARops
- Existing CESARops code: `cesarops-hybrid-engine/`, `cesarops-slicer/`, `sovereign-cloud/`
