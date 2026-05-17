# Capability Detection (Part 2) — Vulkan Feature → Profile Pipeline

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-17. Continuation of Prompt 1 (capability detection).
Status: **REFERENCE — INTEGRATION-READY for the load-time profile
derivation, with polish notes. The 3-layer architecture (Vulkan probe
→ inference → kernel routing graph → optimization compiler) is the
right shape.**

## Verdict

Part 2 delivers the architecture and code skeletons that Part 1 only
sketched. Three concrete layers:

1. **Layer 1: Vulkan capability matrix** — raw probe → normalized
   `InferredCapabilities` bitfield. ~50 LOC.
2. **Layer 2: Kernel routing graph** — `InferredCapabilities + CardClass`
   → `KernelGraph` (matmul + attention + rope + kv_cache nodes). ~60 LOC.
3. **Layer 3: Optimization compiler** — graph optimizer with passes
   (FuseMatmulAndBias, FuseRopeIntoQKV, FuseAttentionKVLoad,
   QuantizeWeights, ReorderMemoryLayout, SelectTilingStrategy). ~100 LOC.

The Layer 3 framing — "ggml + TensorRT graph compiler + Vulkan
subgroup optimizer" — is the correct architectural endpoint. It
formalizes what llama.cpp does informally and what TensorRT does
opaquely. Adopt the structure.

## What's right (accepted)

### The feature-vector → profile derivation pipeline

```
DeviceFeatures (Vulkan probe)
    ↓
InferredCapabilities (normalized bitfield)
    ↓
CardClass (bucket: PascalHighEnd / PascalLowEnd / Maxwell / RDNA / VegaLike / Unknown)
    ↓
KernelGraph (per-stage kernel selection: matmul, attention, rope, kv_cache)
    ↓
Optimization passes applied
    ↓
ExecutableGraph (compiled, ready to dispatch)
    ↓
OptimizationProfile (final shipped output)
```

This is the architecture our load-time path should implement. Each
arrow is a pure function. The pipeline is reproducible and loggable.

### Subgroup-locked workgroup sizing

Their Layer-1 sizing rule:
- subgroup=32 → workgroup=128 (NVIDIA, "warp efficient")
- subgroup=64 → workgroup=256 (AMD wave64)
- otherwise → workgroup=64 (conservative)

Concrete and right. Pascal hits the first branch. This becomes our
default workgroup shape derivation; existing kernels that hardcode
256 should be reviewed against this.

### Critical rule: NEVER launch partial warps

> "For P100 / 1070: NEVER launch partial warps (avoid < 32 active
> lanes)"

This is one of those Pascal gotchas that's easy to miss. Anywhere we
dispatch a workgroup with `(out_dim + 127) / 128` and `out_dim < 128`,
we're firing a workgroup with mostly inactive lanes. Worth a one-pass
audit of the existing dispatch code before this lands.

### Tile sizing from memory bandwidth class

Their derivation:
- High bandwidth (>16 GB VRAM, HBM2): 64×64×16
- Medium (8-16 GB, GDDR5/5X): 32×64×16
- Low (<8 GB): 16×32×8

P100 16 GB HBM2 → High → 64×64×16 tile. GTX 1070 8 GB GDDR5 → Medium
→ 32×64×16. Matches our prior allocator's fp32-fallback-tile sizing
within rounding.

### KV cache layout: `[seq-major][head-major][packed fp16]`

> "FAST layout (llama.cpp optimized behavior):
>  [seq-major][head-major][packed fp16]
>  - continuous writes
>  - append-only
>  - no transpose"

This matches our existing KV cache layout (which is already
`[pos][n_kv_heads][head_dim]` row-major). Confirmation we got this
right. The "packed fp16" is the upgrade we still need (KV dtype
fp16 promotion).

> "SLOW layout (common Vulkan mistake):
>  [head-major][seq-major]  ❌
>  per-token buffer allocation  ❌
>  per-layer buffers  ❌"

We do NONE of these. Already aligned.

### Layer-3 optimization passes

Their pass list:
- FuseMatmulAndBias — already shipped (round 6)
- FuseRopeIntoQKV — q6k_kv stash implements this (Q6_K specific)
- FuseAttentionKVLoad — FA-lite kernel target (not yet integrated)
- QuantizeWeights — Q6_K already loaded as Q6_K, but we DEQUANT to
  fp32 at load (Tier 1 bottleneck — we should keep packed Q6_K)
- ReorderMemoryLayout — KV cache fp16 promotion fits here
- SelectTilingStrategy — paired with the bandwidth-class tile
  derivation above

Each pass maps to an existing TODO. Adopt the pass model as our
optimization-pass framework when the load path lands.

## Polish notes for integration

### 1. wgpu probe layer is missing pieces

Their `probe_wgpu` skeleton uses `limits.shader_float16` which is
NOT a real wgpu Limits field. wgpu exposes fp16 support via
`Features::SHADER_F16`, not via Limits. Correction:

```rust
// WRONG (their version):
fp16_storage: limits.shader_float16,

// RIGHT:
fp16_storage: features.contains(wgpu::Features::SHADER_F16),
```

Also: `subgroup_min_size`/`subgroup_max_size` arrived in wgpu 0.20+.
For older versions we fall back to `infer_wgpu_subgroup` heuristics
(NVIDIA=32, AMD=64 by vendor ID). Track the wgpu version we're on
when we wire this — don't blindly copy their probe code.

### 2. P100 has fp16 storage but NOT shaderFloat16 fast compute

Their `infer_caps` rule:
```rust
let fp16_compute_fast =
    v.shader_float16 &&
    (v.vendor_id == 0x10DE || v.vendor_id == 0x1002);
```

This is wrong for P100. P100 reports `shaderFloat16=true` (it has
fp16 storage AND can compute fp16, just at f16-rate-equal-to-f32 on
sm_60). The flag should distinguish fp16-storage from fp16-fast-compute.
Pascal sm_60 P100 has fp16 storage AND fp16 compute, but **no fp16
speedup** vs fp32. Volta/Turing have speedup. RDNA/Vega have speedup.

Fix:
```rust
let fp16_compute_fast =
    v.shader_float16 &&
    !is_pascal_p100(&v) &&  // sm_60 specific exclusion
    (v.vendor_id == 0x10DE || v.vendor_id == 0x1002);
```

Or better: add a `fp16_speedup_class` enum (`Slow / Same / Fast`)
instead of a bool. P100 = Same. Volta+ = Fast. Maxwell = Slow (or
unsupported). RDNA = Fast.

### 3. The CardClass→KernelGraph table needs Pascal-specific tuning

Their PascalHighEnd kernel graph:
```rust
KernelGraph {
    matmul: KernelNode::Q6KOptimized,
    attention: KernelNode::F16SubgroupFused,
    rope: KernelNode::Fused,
    kv_cache: KernelNode::Fp16Tiled,
}
```

For P100 the `F16SubgroupFused` attention is what we want, but as
of right now the FA-lite kernel doesn't exist yet. Until it lands,
the P100 attention node falls back to our existing per-head split
attention. The graph's `KernelNode` enum needs a runtime check
"is this kernel actually compiled and validated?" with auto-fallback
to a less-fused version. Same pattern we already use elsewhere
(behind feature flags).

### 4. CardClass classifier has a Maxwell/Pascal edge case

Their classifier:
```rust
match f.vendor {
    Vendor::Nvidia => match f.fp16_compute {
        false => CardClass::Maxwell,
        true => {
            if f.int8 {
                if f.vram_mb >= 10000 { CardClass::PascalHighEnd }
                else { CardClass::PascalLowEnd }
            } else { CardClass::Maxwell }
        }
    },
    ...
}
```

Problem: P100 sm_60 has `int8=false` (no DP4A). Per their own table,
P100 should classify as PascalHighEnd. With this classifier, P100
falls through to `CardClass::Maxwell` because `int8=false`.

Fix: P100 needs a separate path. Either bucket by VRAM tier first
(>=16 GB + fp16 → PascalHighEnd) OR add an explicit `is_p100`
heuristic check. We have a P100 on T440; this WILL trip on first run
if we ship it as-written.

```rust
Vendor::Nvidia => {
    if !f.fp16_compute { CardClass::Maxwell }
    else if f.vram_mb >= 12000 && f.memory_bandwidth_class >= BandwidthClass::High {
        CardClass::PascalHighEnd  // P100 (HBM2 high BW path)
    } else if f.int8 {
        CardClass::PascalLowEnd  // 1070 / P40 / P4 (DP4A)
    } else {
        CardClass::Maxwell  // M40 / older
    }
}
```

### 5. ExecutableGraph compilation needs to be lazy

Their Layer-3 `ExecutableGraph::from(g)` is presented as eager
compilation at startup. For our use case (multi-model concurrent
inference), each (model, GPU) pair gets its own executable graph
on demand at load time. Lazy compile, cache the result, evict on
model unload. Track for the multi-model registry integration.

### 6. The `int4_supported` flag depends on `buffer_device_address`

Their inference:
```rust
let int4_supported = v.shader_int8 && v.buffer_device_address;
```

This isn't quite right — int4 support is more about packed-format
shader paths than buffer_device_address (which is a Vulkan 1.2
addressing mode). The actual gate for int4 is whether we have a
working int4 dequant path in the shader. P100 can do it via
`shaderInt8 + bit unpack`, no `buffer_device_address` needed. Track
as a polish on the inference rule when we eventually add Q4_K
support; not relevant for our Q6_K-first path.

### 7. Logging the full pipeline at load-time

Each layer's output should land in the load log:
```
[load] Adapter: NVIDIA Tesla P100-PCIE-16GB (vendor=10de device=15f8)
[load] Vulkan raw: subgroup=32 max_wg=1024 fp16_storage=true int8=false vram=16384 MB
[load] Inferred: warp_model=Warp32 fp16_compute_fast=Same int8_dot_fast=false
[load] Class: PascalHighEnd
[load] KernelGraph: matmul=Q6KOptimized attn=F16SubgroupFused rope=Fused kv=Fp16Tiled
[load] Optimization passes applied: FuseMatmulAndBias, FuseRopeIntoQKV, ReorderMemoryLayout
[load] Profile: workgroup=128 tile=64x64x16 chunk=512 kv_dtype=fp16 scratch=2.0GB fusion=Aggressive
```

Six log lines for full observability. When something goes wrong this
is the first thing we read.

## Integration sequence (when this lands)

1. Drop `DeviceFeatures` + probe code into
   `cesarops-inference/src/profile/probe.rs`. ~100 LOC.
2. Drop `InferredCapabilities` + inference rules into
   `cesarops-inference/src/profile/infer.rs`. ~80 LOC with the polish-2
   fix.
3. Drop `CardClass` + classifier with polish-4 fix. ~50 LOC.
4. Drop `KernelGraph` + selector with polish-3 runtime fallback. ~80 LOC.
5. Drop optimization passes incrementally — start with the ones that
   match shipped features (FuseMatmulAndBias). ~60 LOC per pass.
6. Wire `OptimizationProfile::derive(adapter, model)` as the entry
   point. Returns `Result<OptimizationProfile, ProfileError>` with
   ProfileError on no compatible Vulkan adapter found.
7. Replace existing kernel-selection branches with profile-driven
   routing.
8. Smoke test through both gates.

Total estimate: ~3-4 days for steps 1-7. Feature-flag the new path
so we can A/B against the old hardcoded selection.

## Where this slots vs queued work

Confirmed slot 5 in priority order:

1. Multi-model registry (in-progress)
2. `--bench` mode + EngineBenchmarker
3. Q6_K K/V/Q proj+RoPE+cache fusion (V kernel ready)
4. Attention scratch pool
5. **Capability detection + OptimizationProfile** ← this drop
6. Prefill batching mode
7. wgpu_hal::vulkan submit path

Capability detection landing here unblocks the two-prong-route
compromise architecture and provides the runtime selector for
KernelSet::Optimized vs KernelSet::Fallback.

---

Filed under research_log because the architecture is integration-ready
once the four polish fixes land (wgpu probe API correction, fp16
speedup tier instead of bool, PascalHighEnd classifier fix, lazy
compilation for multi-model). The 3-layer pipeline shape is the
right architectural endpoint.
