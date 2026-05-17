# llama.cpp / koboldcpp Capability Detection (Part 1) — Reality Check

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-17. Response to Prompt 1 (per-card profile dump +
capability detection from koboldcpp / llama.cpp). Part 2 still pending.
Status: **REFERENCE — REFRAMING ACCEPTED.** Contributor explicitly
rejected the prompt's premise of "per-card kernel tables" and showed
that llama.cpp / ggml uses a feature-detection matrix instead. This
correction is RIGHT and changes our OptimizationProfile design.

## The reframing (operator-relevant, accept)

Operator's Prompt 1 asked for:
> "the actual per-card optimization profile that koboldcpp uses
> internally for at least these cards: P100 / P40 / P4 / M40 / GTX
> 1070 / K80 / RX cards"

with an expected output struct:
```rust
struct OptimizationProfile {
    card_class: CardClass,
    kernel_set: KernelSet,
    workgroup_sizes: WorkgroupConfig,
    tile_sizes: TileConfig,
    chunk_size: u32,
    kv_dtype: KvDtype,
    fp16_compute: bool,
    ...
}
```

Contributor's correction (paraphrased):
> "There is no per-device kernel table like
> `GTX1070 -> use kernel X with tile 64x128`.
> Instead llama.cpp does:
>   1. Detect GPU features (CUDA or Vulkan)
>   2. Select backend
>   3. Backend internally picks kernels based on:
>      - compile-time macros
>      - tensor layout
>      - a few runtime thresholds (VRAM, SM count, sometimes
>        heuristics like fast fp16)"

**Accepted.** The honest mapping is feature-matrix → kernel-selection,
not GPU-name → profile. This matches what we'd discover anyway when
we hit the second card and realize the profile-table approach
explodes combinatorially.

## What the corrected `OptimizationProfile` should look like

Replace the per-card-name approach with a feature-bitfield + a small
set of derived heuristics:

```rust
pub struct DeviceCapabilities {
    // Identity (for logging / leaderboard / debug only,
    // NOT for kernel selection)
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_name: String,
    pub driver_version: u32,

    // Feature bitfield — this drives kernel selection
    pub fp16_storage: bool,       // shaderFloat16 + storage_buffer_16bit_access
    pub fp16_fast_compute: bool,  // tensor cores OR vendor fp16 acceleration
    pub int8_dp4a: bool,          // DP4A on Pascal sm_61+, Vulkan shaderInt8
    pub subgroup_size: u32,       // 32 (NVIDIA) / 64 (AMD wave64) / variable (RDNA)
    pub subgroup_ops_supported: SubgroupFlags,
    pub max_workgroup_size: u32,
    pub min_storage_buffer_offset_alignment: u32,
    pub max_compute_workgroup_storage_size: u32,

    // Numeric capacity — drives sizing decisions
    pub vram_bytes: u64,
    pub sm_count: u32,            // multiprocessor count (NVIDIA) / CU count (AMD)
    pub shared_mem_per_block: u32,
}

pub struct OptimizationProfile {
    pub kernel_set: KernelSet,    // derived from feature bitfield
    pub kv_dtype: KvDtype,        // fp16 if fp16_storage, fp32 fallback
    pub chunk_size: u32,          // derived from VRAM bucket + model size
    pub scratch_pool_bytes: u64,  // min(20% VRAM, 2.5 GB cap)
    pub tile_size: TileConfig,    // mostly fixed, occasionally subgroup-size-derived
}

impl OptimizationProfile {
    pub fn derive(caps: &DeviceCapabilities, model: &ModelMeta) -> Self {
        // Pure function. Reproducible. Loggable. Testable.
    }
}
```

The key shift: `OptimizationProfile` is **derived** from
`DeviceCapabilities`, not looked up from a table. We can still log a
human-readable "card class" string for the UI, but it's a label not a
driver of behavior.

This collapses the combinatorial explosion of a per-card table into a
~10-feature bitfield. New cards get supported automatically as long
as their feature set falls within recognized buckets.

## The actual feature matrix (theirs, accepted)

For our targets, here's the feature set:

| Feature | P100 | P40/P4/1070 | M40 | K80 | Vega | RDNA |
|---------|------|-------------|-----|-----|------|------|
| fp16 storage | yes | yes | partial | no | yes | yes |
| fp16 fast compute | partial* | partial* | no | no | yes | very fast |
| int8 DP4A | partial† | yes | no | no | varies | good |
| subgroup size | 32 | 32 | 32 | 32 | 64 | 32 or 64 |
| best quant kernel | q4_K, f16 | q4_0, q8_0 | f32 fallback | f32 fallback | fp16 compute | fp16 + wave ops |

Notes:
- *Pascal fp16 compute exists but is bandwidth-optimization only,
  no tensor cores. Storage benefit is real (halved KV cache), compute
  benefit is marginal.
- †P100 sm_60 actually lacks full DP4A — this was the subtle
  correction in their writeup. Pascal CONSUMER cards (sm_61+) have
  DP4A; the Tesla P100 (sm_60) does not. Our cluster has the P100,
  so DP4A is OFF for us. The GTX 1070 is sm_61 so DP4A is ON for it.

This split between P100 and 1070 is exactly the kind of detail a
per-card-name table would get wrong if not maintained perfectly. The
feature-matrix approach catches it for free.

## Their key takeaway (confirmed)

> "Don't build: per-GPU kernel databases.
> Do build: feature detection matrix:
>   FP16_FAST
>   FP16_STORAGE_ONLY
>   INT8_DP4A
>   SUBGROUP_SIZE_32/64
>   SHADER_INT8
>   SHADER_FLOAT16
>   VRAM_BUCKET
> Then: profile = f(features)"

Adopted. This becomes the architecture for our load-time detection.

## Implementation plan for our load-time detection

1. Probe Vulkan (via wgpu's `Adapter::get_info()` plus the
   `Limits` and `Features` queries we can already run today)
2. Construct `DeviceCapabilities` from the probe
3. Derive `OptimizationProfile` via pure function
4. Log the derived profile + the underlying capability bitfield so
   debugging "why did it pick this kernel set?" is one log line away
5. Pick `KernelSet::Optimized` vs `KernelSet::Fallback` from the
   profile's feature gates

For our specific fleet:

**T440 P100** (sm_60):
- fp16_storage: yes
- fp16_fast_compute: no (no tensor cores, sm_60 also no DP4A)
- int8_dp4a: NO (sm_60 specifically)
- subgroup_size: 32
- vram: 16 GB
- → KernelSet::Optimized (Pascal-tuned f16 storage path), KvDtype::Fp16,
  chunk_size: 512, scratch_pool: 2.0 GB

**cesarops2 GTX 1070** (sm_61):
- fp16_storage: yes
- fp16_fast_compute: partial (no tensor cores)
- int8_dp4a: YES (sm_61+)
- subgroup_size: 32
- vram: 8 GB
- → KernelSet::Optimized (Pascal-tuned with DP4A int8 path enabled),
  KvDtype::Fp16, chunk_size: 512, scratch_pool: 1.0-1.2 GB

So even our two boxes have different profiles. The DP4A difference
specifically might matter for q8_0 quantization paths if we ever
add them — the 1070 would unlock a faster int8 dot-product path
that the P100 cannot.

## Source map (anchors for our porting work)

Their source citations are the most useful operational content:

| File | Purpose |
|------|---------|
| `ggml/src/ggml-cuda.cu` | CUDA kernels + compute capability branching |
| `ggml/src/ggml-vulkan.cpp` | Vulkan backend + feature probing |
| `ggml/src/ggml.c` | tensor ops abstraction |
| `llama.cpp` | model runtime orchestration + GPU offload logic |
| `examples/main` | runtime selection behavior |

For our wgpu work the relevant reference is `ggml-vulkan.cpp` since
that's the closest analog to what we're building. The CUDA file is
useful as a sanity check on which features matter (DP4A, fp16 fast
compute, subgroup sizes) but the API surface to query them in wgpu
is different.

When we wire the detection code, the workflow is:
1. Read `ggml-vulkan.cpp`'s `ggml_vk_check_features` (or equivalent)
   to see what they probe
2. Map each probe to the wgpu equivalent (some are
   `Adapter::features()`, some are `Adapter::limits()`, some require
   raw Vulkan via wgpu_hal once that's wired)
3. Implement `DeviceCapabilities::probe(adapter: &wgpu::Adapter)`
4. Implement `OptimizationProfile::derive(caps, model)` as the
   pure-function classifier

## Polish notes for integration

### 1. Don't conflate "vendor ID" with "kernel selection"

Their note: identity fields (vendor_id, device_id, device_name,
driver_version) are useful for **logging and debugging**, not for
kernel selection. The temptation is real — see "vendor=NVIDIA" and
branch on it. Don't. The feature bitfield captures everything that
actually matters for kernel choice. Vendor branching is an anti-pattern
that will burn us when (a) a new NVIDIA card has different features
or (b) an AMD card with the same features as our Pascal targets shows
up.

Use vendor ID for log lines and the user-facing card label only.

### 2. Subgroup size variability on RDNA

RDNA is not a clean wave64 — it can dispatch in wave32 or wave64
depending on the workgroup configuration. Their writeup glosses this
with "subgroupSize = 32 or 64" for RDNA. When we eventually add RDNA
support, the subgroup size becomes per-pipeline rather than
per-device. wgpu exposes this via `subgroup_min_size` and
`subgroup_max_size`. For Pascal both are 32; we won't hit the
variability until later.

### 3. VRAM bucket is the wrong unit

Their note treats `VRAM_BUCKET` as a feature flag. It's not — it's
a continuous variable. Better to use the raw VRAM bytes and let the
profile derivation function compute the scratch pool size as
`min(20% VRAM, 2.5 GB cap)` directly. Bucketing introduces cliff
behavior at bucket boundaries. Already handled correctly in the
attention scratch pool design.

### 4. The P100 sm_60 DP4A gotcha

The contributor flagged it as "subtle." Worth promoting to a polish
note: **P100 (sm_60) does NOT have DP4A.** Pascal CONSUMER cards
(sm_61, GP104/GP102/GP106) have DP4A. The P100 (GP100, sm_60) was
the datacenter card that traded DP4A for HBM2 bandwidth and faster
fp16. This is the kind of detail that breaks per-card name-tables —
"Pascal" isn't a uniform tier.

For our T440 with the P100: any q8_0 / DP4A-accelerated path is OFF.
We rely on the f16 path for quantized matvec. Already aligned with our
shipped Q6_K path which uses f32 accumulation + f16 storage where
beneficial.

### 5. Workgroup sizes are NOT in the profile

Their writeup: "workgroup_sizes are NOT per-GPU." Confirmed — they're
fixed per-kernel at compile time in CUDA, and shader-specialization-
constant-derived in Vulkan. Our shaders should:
- Default to a Pascal-friendly fixed size (typically 128 or 256)
- Optionally accept a workgroup size override via shader spec
  constants for cards that profile better at a different size
- NOT branch in Rust dispatch code based on the profile's "tile size"
  field

Drop `tile_sizes: TileConfig` from `OptimizationProfile` unless we
add a runtime-tunable variant later (which would require either
shader recompilation per-card or pipeline cache variant management —
both expensive). For now: single shipped tile size per kernel,
chosen for Pascal optimality, fallback path uses smaller boring tiles.

### 6. KV dtype is the simplest to get right

Their position: fp16 default unless memory constrained, fp32
fallback on weak GPUs. Concrete:
```rust
fn select_kv_dtype(caps: &DeviceCapabilities) -> KvDtype {
    if caps.fp16_storage { KvDtype::Fp16 } else { KvDtype::Fp32 }
}
```

That's the whole logic. Halves KV cache memory on every card we care
about. Already promoted in the attention_scratch_pool stash analysis.

### 7. Fallback behavior — they basically never refuse to run

Their note: "Does it refuse to run? Rarely. Only if no compatible
backend compiled in / GPU lacks required Vulkan features entirely /
CUDA compute capability too old. Otherwise it runs slow."

Adopt the same posture. Our fallback path:
- If `KernelSet::Optimized` features not met, fall back to
  `KernelSet::Fallback`
- If fallback features not met, refuse load with a clear error
- Never silently fall back to CPU (we don't have a CPU path; this
  isn't llama.cpp — it's a wgpu-only engine)

The "refuse to run" boundary for us is "no Vulkan-capable adapter
found" — that's the only hard refuse. Everything else gets the
fallback profile.

### 8. Logging behavior

Their reference outputs:
- "using CUDA backend"
- "GGML CUDA: device 0: GTX 1070"
- "KV cache size …"
- "offloading X layers to GPU"
- koboldcpp: "GPU detected, using automatic offload"

For us, log at model-load:
```
[load] Adapter: NVIDIA Tesla P100-PCIE-16GB (vendor=10de device=15f8)
[load] Capabilities: fp16_storage=yes fp16_fast=no dp4a=no subgroup=32 vram=16 GB
[load] Profile: KernelSet::Optimized chunk=512 kv_dtype=fp16 scratch_pool=2.0 GB
[load] Loaded 28 layers (1.5 GB Q6_K weights)
```

One log line for adapter, one for raw capabilities, one for derived
profile, one for the model. Three lines = full observability into
"why did it pick this." Worth its weight in gold during debugging.

## What's still missing (Part 2 expected)

Their writeup ends with "stand by for part 2." Things they have NOT
delivered yet:

- The actual probing **code** — they described the structs and APIs
  but didn't give us a copy-paste-able probe function
- The decision matrix from feature bitfield to KernelSet choice in
  table form (they sketched it but didn't enumerate all the
  combinations)
- The Vulkan-specific extension/feature names we need to query in
  the order we should query them (they referenced
  `VkPhysicalDeviceVulkan12Features` / `VkPhysicalDeviceSubgroupProperties`
  but didn't give us the equivalent wgpu API calls)
- Concrete fallback behavior code for "feature missing → which
  kernel?"
- The specific `ggml-vulkan.cpp` line numbers / function names for
  the probe code

Hopefully Part 2 covers the missing implementation details. If it
doesn't, those are the items for a follow-up prompt.

## Where this slots vs other queued work

Updated priority order with this drop's information factored in:

1. **Multi-model loading registry** (in-progress)
2. **`--bench` mode + real EngineBenchmarker** (queued)
3. **Q6_K K/V/Q proj+RoPE+cache fusion** (V kernel ready)
4. **Attention scratch pool** (skeleton ready, polish notes ready)
5. **Capability detection + OptimizationProfile** ← this drop unblocks
   the load-time detection that powers the two-prong-route compromise
6. **Prefill batching mode** (depends on scratch pool + profile)
7. **wgpu_hal::vulkan submit-path port** (architecture approved)

Capability detection moves up to slot 5 because it's a prerequisite
for the two-prong architecture the operator locked in. Without it we
have the pieces but no runtime selector.

The detection itself is small — maybe ~150 LOC for the
`DeviceCapabilities::probe` function and ~50 LOC for the
`OptimizationProfile::derive` function. We can land it as a
standalone change once Part 2 arrives or once we've cross-referenced
ggml-vulkan.cpp ourselves to fill the gaps.

## Final mapping — accept these 7 corrections to our prompt 1

1. **Per-card kernel database is wrong abstraction** — use feature
   matrix
2. **Tile sizes are NOT runtime-selected** — fixed per kernel, not
   per profile
3. **Workgroup sizes are NOT per-GPU** — backend defaults, occasionally
   subgroup-size-derived
4. **Chunk size is model-driven not GPU-driven** — although we
   refine with VRAM bucket as cap (our prefill design already does)
5. **KV dtype defaults to fp16** unless storage isn't supported
6. **Vendor ID is for logging only**, not kernel selection
7. **Refuse-to-run is rare** — fall back to slower path almost
   always

These corrections all go into the `OptimizationProfile::derive`
implementation plan.

---

Filed under research_log because the part 1 reframing is operationally
useful TODAY (it changes the OptimizationProfile struct shape from
per-card lookup table to feature-derivation function), but the
implementation details (probe code, decision matrix, Vulkan extension
queries, ggml-vulkan.cpp line citations) are still pending in part 2.
Stash will be updated when part 2 arrives.

## Verbatim source — captured below for reference

[Full text of "Reality model: how llama.cpp / koboldcpp actually do
this" document, sections 1-7 + final mapping. Captured 2026-05-17.

Section headings:
1. What "profiles" actually exist (implicit, not explicit)
2. Runtime detection logic (ACTUAL SYSTEM USED)
3. Your requested per-card "profiles" (reconstructed reality model)
4. Fallback behavior (important for your design)
5. Source map (where to look)
6. Direct mapping to YOUR desired struct
7. Key takeaway for your wgpu design

Critical structural points already captured in analysis above.
Recovery from cluster chat history if full verbatim ever needed.]
