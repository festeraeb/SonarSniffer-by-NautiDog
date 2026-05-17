# Stage #11 — KV Cache Prefetch + 128B-Aligned Subgroup Streaming

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-17. Response to greenlight on item #11 from the
fusion_rewrite_architecture drop. Predicted +40-80% Pascal gain on
top of fusion work.
Status: **REFERENCE — INTEGRATION-READY in concept, needs concrete
shader source and KV layout migration. The architecture is sound;
the WGSL sketches are not yet drop-in.**

## Verdict

The architectural shift is right and addresses a real Pascal pain
point. Reframing:

> "KV cache stops being 'a buffer problem' and becomes a memory
> topology problem (L2 + coalescing + prefetch scheduling)."

Their solution: **convert random head-strided KV reads into predictable
128B-aligned subgroup streaming** via a 2-stage prefetch pipeline.

This is a real Pascal win because:
- P100 HBM2 has high raw bandwidth but weak L2 predictability — it
  loves contiguous streaming
- GTX 1070 GDDR5 has lower raw bandwidth but better cache locality
  per warp — it loves smaller windows with more reuse
- Both prefer 128B-aligned cache-line access
- Both punish strided multi-head access patterns

The drop's per-card tuning (P100: window=32 + prefetch_depth=2;
1070: window=16 + prefetch_depth=3 + DP4A) is exactly the kind of
divergence the OptimizationProfile derivation should produce. Slots
into the capability detection v2 architecture as a concrete kernel
strategy variant.

## What's right (accepted)

### KV memory layout: 128B-aligned packed fp16 blocks

Current: `[pos][n_kv_heads][head_dim]` row-major fp32 = 256 bytes
per (pos, kv_head) for head_dim=128 fp32.

Proposed: same layout, but fp16 (halves footprint) AND aligned to
128B blocks (= 64 fp16 values per block).

For our model:
- head_dim=128 in fp16 = 256 bytes per kv_head per position
- That's 2 × 128B blocks per kv_head per position
- n_kv_heads=2 → 4 × 128B blocks per position
- Each block boundary perfectly aligned to L2 cache line

This unlocks 100% L2 cache line utilization on KV reads.

### Two-stage pipeline: PREFETCH → COMPUTE

Stage A (prefetch): each subgroup reads KV in coalesced 128B chunks
into shared/workgroup memory. NO compute happens here.

Stage B (compute): attention dot/softmax/AV operates exclusively on
shared memory. NO global KV reads happen here.

This is the standard FlashAttention K/V tile pattern but with explicit
focus on the cache-line boundary. The win is twofold:

1. Coalesced loads → predictable bandwidth
2. Compute decoupled from memory → enables latency hiding via
   pipelined prefetch (next-tile loaded while current-tile computes)

### Per-card window tuning

Their `select_kv_strategy`:

| Card | Window | Block align | Prefetch depth | DP4A | Bandwidth-vs-cache |
|------|--------|-------------|----------------|------|---------------------|
| P100 sm_60 | 32 | 128B | 2 | NO | bandwidth |
| 1070 sm_61 | 16 | 128B | 3 | YES | cache |
| Fallback | 8 | 128B | 1 | NO | conservative |

The window/depth/DP4A settings differ per card, validating our
feature-bitfield approach from the capability detection drops. This
table becomes part of the `OptimizationProfile::kv_strategy` field
once we land both this drop and the capability detection rework.

### Performance projection

Their predicted ladder for our hardware:

| Card | Before | After fusion | After KV prefetch | Optimized submit |
|------|--------|--------------|-------------------|------------------|
| P100 | 2.2 t/s | 5-7 t/s | **9-14 t/s** | 12-18 t/s |
| GTX 1070 | ~2 t/s | 4-6 t/s | **7-11 t/s** | 9-14 t/s |

The KV prefetch step is the +40-80% gain they promised. P100 gains
more than 1070 because it's bandwidth-rich and HBM2 loves the
streaming pattern. This matches our internal projection ladder; the
"after KV prefetch" row pulls forward what we had at 8-14 t/s on P100
into the same range.

## Polish notes for integration

### 1. WGSL sketches are not drop-in

The provided `kv_prefetch` and `attention_compute` WGSL kernels are
illustrative pseudo-code. Issues that need fixing before they
compile under naga:

- `load_aligned_128b` and `subgroup_invocation_id()` and
  `subgroup_barrier()` are sketched as if they're standard WGSL
  builtins. Real names:
  - `subgroupBarrier()` (note camelCase, no underscore in WGSL)
  - `local_invocation_id` for thread-within-workgroup ID
  - No subgroup intrinsic for "lane within subgroup" exists in
    portable WGSL until subgroup ops feature lands; for Pascal
    Vulkan we can use `local_invocation_id.x % subgroupSize`
- `vec4<f32>` accumulator in `attention_compute` mixes with f16
  shared memory; need explicit conversions
- `shared.k_block[lane][b]` syntax: WGSL workgroup-memory arrays use
  index-then-index, fine, but lane needs to be u32 not signed
- `subgroup_barrier()` must be `workgroupBarrier()` for cross-lane
  sync within a workgroup

### 2. Naga subgroup feature gating

WGSL subgroup ops (`subgroupBroadcast`, `subgroupBallot`, etc.) are
behind a feature flag in wgpu. We need to enable
`Features::SUBGROUP` and check support before compiling the optimized
kernel. The fallback path (per the homelab doctrine compromise) does
NOT use subgroup ops and runs on any Vulkan adapter.

### 3. The KV layout migration affects existing code

We currently store KV in `[pos][n_kv_heads][head_dim]` fp32. Migrating
to fp16 + 128B-aligned blocks affects:

- `tensor_loader_safe.rs` — KV cache initialization
- `forward_pass.rs::execute_layer` — KV write site (currently
  fp32, needs fp16 cast)
- `attention_dispatch.rs` — KV read site
- The fused K/V/Q proj+RoPE+cache shaders from the q6k_kv stash —
  they currently write fp32, need fp16 output variant
- Any tooling that reads KV cache for diagnostics

The migration is a bounded change but touches several files. Plan
as a single integration step labeled "kv-fp16-promotion" with smoke
tests on both gates after.

### 4. Window size as runtime parameter, not compile-time constant

Their `WINDOW_BLOCKS` and `SUBGROUP_SIZE` are presented as compile-time
constants. For our two-prong-route architecture, these need to be
push-constants or pipeline specialization constants because the same
kernel gets dispatched on P100 (window=32) and 1070 (window=16) with
different values. WGSL pipeline override constants handle this:

```wgsl
override WINDOW_BLOCKS: u32 = 16u;
override SUBGROUP_SIZE: u32 = 32u;
```

Set at pipeline creation time from the OptimizationProfile.

### 5. Prefetch depth = 2 means double-buffered shared memory

P100's `prefetch_depth: 2` means: while subgroup is computing on tile
N, prefetch tile N+1 in parallel. This doubles shared memory usage
per workgroup:

- 64 fp16 per K + 64 fp16 per V per slot = 256 bytes per slot
- × 32 lanes per subgroup = 8 KB per slot
- × 2 (prefetch depth) = 16 KB per workgroup just for the staging
  buffer

Pascal's max shared memory per workgroup is 48 KB. 16 KB is fine
budget-wise. 1070 with `prefetch_depth: 3` uses 24 KB. Both within
limits.

But: when we extend to multi-head simultaneous, multiplied by
n_heads, the budget can explode. The kernel needs an explicit check
against `max_compute_workgroup_storage_size` from the capability
profile, with fallback to depth=1 if budget is tight.

### 6. Subgroup window vs workgroup size

Their kernel uses workgroup_size = SUBGROUP_SIZE (typically 32 on
Pascal, 64 on AMD). This means each workgroup IS one subgroup. Fine
for prefetch where we're streaming KV blocks. For attention compute
we may want larger workgroups (multiple subgroups cooperating on
one Q row). Track as a kernel-tuning iteration after the basic
pattern lands.

### 7. The "exp(score)" softmax is naive — same trap as before

Their attention_compute has:
```
let weight = exp(score);
acc += weight * v;
norm += weight;
```

This is the same overflow-prone softmax we already flagged in the
prefill_batching stash polish notes (item #2) and locked into
lessons_learned.md. Need online-softmax max-subtraction:
```
m_new = max(m_old, score)
p     = exp(score - m_new)
acc   = acc * exp(m_old - m_new) + p * v
norm  = norm * exp(m_old - m_new) + p
m     = m_new
```

This is the THIRD time the cluster has shipped the naive softmax
form. Worth reaffirming the lessons_learned entry on it.

### 8. Causal masking absent

Their kernel iterates `for b in 0..WINDOW_BLOCKS` without bounds
checking against the current token's position. For decode mode (M=1)
this is fine because we only read KV positions ≤ current pos.
For prefill mode (M>1) we need an explicit mask test against
(q_pos < k_pos). Track as a polish for prefill batching integration.

### 9. The "k_block[lane][b]" indexing is per-lane scratch

Re-reading their shared memory layout:
```rust
struct KvPrefetchBuffer {
    k_block: [[f16; 64]; SUBGROUP_SIZE],
    v_block: [[f16; 64]; SUBGROUP_SIZE],
    slot: u32,
}
```

`[[f16; 64]; SUBGROUP_SIZE]` is per-lane storage of 64 fp16 = 128B
each. So each lane has its own 128B slot. This means each subgroup
holds 32 × 128B = 4 KB of K + 4 KB of V = 8 KB per workgroup.

This is the "each lane gets one block" mapping. Different from the
"all lanes cooperate on one block" pattern. Both work; the first is
simpler to reason about, the second is potentially better for sharing
across heads.

For our integration, start with the per-lane mapping (theirs).
Optimize to cooperative tiles later if profiling shows it matters.

### 10. Item #12 is offered next

Their final paragraph offers Stage #12: "Zero-sync decode loop"
(persistent megakernel). That arrived in the next message and is
stashed separately with significant pushback notes — it conflicts
with the homelab doctrine compromise we already adopted.

Stage #11 here does NOT have those concerns. It's a kernel-level
optimization with no architectural conflicts.

## Integration sequence (when this lands)

1. **KV layout migration** — fp32 → fp16 + 128B alignment. Standalone
   change, parity test against current implementation. ~150 LOC across
   tensor_loader / forward_pass / attention_dispatch.
2. **Prefetch kernel** — implement per polish notes (1, 2, 4, 5).
   Standalone shader file `kv_prefetch.wgsl`. Compiles and validates
   against naga.
3. **Attention compute kernel using prefetch** — implement per polish
   notes (7, 8). Read shared memory only, write attention output.
4. **Capability-aware dispatch wiring** — pick window/depth/DP4A
   per device profile. The OptimizationProfile.kv_strategy field
   from the capability detection rework drives this.
5. **Smoke test through both gates** — T440 P100, cesarops2 GTX 1070.
   Parity vs current implementation, then perf measurement.
6. **Tune window sizes** — they suggested P100=32 / 1070=16; verify
   with the bench mode if it's landed by then.

Total estimate: ~1.5-2 days for steps 1-5.

Depends on: capability detection landing (provides the strategy
dispatch hook), attention scratch pool landing (provides shared
infrastructure), KV fp16 promotion (the layout migration step itself
is the migration).

## Where this slots vs queued work

Updated priority order:

1. Multi-model registry (in-progress)
2. `--bench` mode
3. Q6_K K/V/Q proj+RoPE+cache fusion (V kernel ready)
4. Attention scratch pool
5. Capability detection + OptimizationProfile
6. **KV cache fp16 + 128B prefetch** ← this drop, consolidates the
   "KV layout migration" + "KV prefetch kernel" work into one
   integration phase
7. Prefill batching mode
8. wgpu_hal::vulkan submit path

Slot 6 here makes sense because KV prefetch needs the capability
profile to drive the strategy selection, and the fp16 layout
migration is naturally bundled with the prefetch kernel work.

---

Filed under research_log because the architecture is integration-ready
but the WGSL sketches need polish-1 fixes before compilation. The
+40-80% Pascal gain projection is realistic and stacks cleanly with
the other queued fusion work.

## Verbatim source — design pseudocode

Captured below.

### KV strategy dispatch

```rust
fn select_kv_strategy(gpu: &DeviceFeatures) -> KvStrategy {
    match gpu {
        // P100 (SM60) - bandwidth rich, weaker cache locality
        g if g.sm == 60 => KvStrategy {
            window_size: 32,
            block_align: 128,
            prefetch_depth: 2,
            use_dp4a: false,
            prefer_bandwidth_over_cache: true,
        },

        // GTX 1070 (SM61) - better cache behavior, DP4A exists
        g if g.sm == 61 => KvStrategy {
            window_size: 16,
            block_align: 128,
            prefetch_depth: 3,
            use_dp4a: true,
            prefer_bandwidth_over_cache: false,
        },

        // fallback
        _ => KvStrategy {
            window_size: 8,
            block_align: 128,
            prefetch_depth: 1,
            use_dp4a: false,
            prefer_bandwidth_over_cache: false,
        }
    }
}
```

### Prefetch kernel sketch (NEEDS POLISH)

```wgsl
@compute @workgroup_size(SUBGROUP_SIZE)
fn kv_prefetch(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let lane = lid.x;
    let token = gid.x;

    // Each subgroup preloads a contiguous KV window
    let base_block = token * KV_BLOCKS_PER_TOKEN;

    // 128B aligned loads (critical)
    for (i in 0..WINDOW_BLOCKS) {
        let block_id = base_block + i;

        // Coalesced load: each lane grabs contiguous f16 vec4
        let kv = load_aligned_128b(block_id, lane);

        shared.k_block[lane][i] = kv.k;
        shared.v_block[lane][i] = kv.v;
    }

    subgroup_barrier();
}
```

### Attention compute kernel sketch (NEEDS POLISH + ONLINE SOFTMAX)

```wgsl
@compute @workgroup_size(SUBGROUP_SIZE)
fn attention_compute() {
    let lane = subgroup_invocation_id();

    let q = load_q(lane);

    var acc = vec4<f32>(0.0);
    var norm = 0.0;

    // Iterate ONLY over shared memory blocks
    for (b in 0..WINDOW_BLOCKS) {
        // fully local memory (fast path)
        let k = shared.k_block[lane][b];
        let v = shared.v_block[lane][b];

        let score = dot(q, k);

        let weight = exp(score);  // BUG: needs online max-subtract

        acc += weight * v;
        norm += weight;
    }

    write_output(acc / norm);
}
```
