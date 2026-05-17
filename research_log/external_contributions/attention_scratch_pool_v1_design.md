# Attention Scratch Pool + Capability-Aware Sizing — Design Response

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-17. Response to Prompt 5 (attention scratch pool with
dual-path support: optimized FA-lite + stable 3-kernel fallback).
Status: **REFERENCE — INTEGRATION-READY skeleton with polish notes.
Decisions defended, sizing tables concrete, correctness flags called
out by the contributor themselves.**

## Verdict

**Adopt the recommendations as-is for v1.** The contributor confirmed
all four of the operator's leanings and gave concrete defenses:

| Decision | Operator leaning | Contributor recommendation | Match |
|----------|------------------|---------------------------|-------|
| Static vs dynamic | static | static | ✓ |
| Allocation pattern | one big buffer, suballocate | "1-3 large device buffers + region allocator inside" | ✓ |
| Reuse strategy | per-token reset | "per forward pass reset" | ⚠ refined |
| Dual-path | Option A (two pools) | Option A explicitly recommended over B and C | ✓ |
| Pool sizing | per-card capability table | concrete table provided | ✓ |
| Failure mode | refuse / reduce chunk | "reject at load OR reduce chunk OR disable FA-lite" | ✓ |

The one refinement: reset cadence should be **per forward pass**, not
per token. Operator's prompt offered "per layer / per token /
persistent" as the choice menu. Contributor's argument for forward-pass
scope is correct — both FA-lite and fallback have lifetimes that
collapse cleanly to the forward-pass boundary, the KV cache already
defines the token-level persistence boundary so scratch doesn't need
to live across tokens, and per-layer reset would underutilize reuse
opportunities within a layer (e.g., tiled attention reusing the K-tile
buffer across multiple Q tiles).

**Adopt: forward-pass scope reset.**

## Concrete sizing tables (theirs, accepted)

### Per-layer fallback scratch (M=512, 12 heads, head_dim=128, fp32)

| Item | Formula | Size |
|------|---------|------|
| scores per head | 512 × 512 × 4B | 1.0 MB |
| 12 heads | × 12 | 12.0 MB |
| **Total per forward pass** | | **~12 MB** |
| With safety margin | | **16 MB reserved** |

Because layers reuse the same scratch (forward-pass scope), this is
**12 MB total** not 12 MB × 28 layers = 336 MB. Operator caught this
correctly in the prompt; contributor confirmed.

For our actual model — Qwen 1.5B with GQA n_kv_heads=2, n_heads=12 —
the scores buffer dimensions are still M×M (the score matrix is
indexed by Q-position × K-position, not by KV-head). 12 MB scratch
budget is correct as-stated. K/V access uses GQA indexing (recorded
in lessons_learned.md from prior pass) but scratch sizing doesn't
change.

### Prefill batching v1 scratch (M=512)

| Component | Size |
|-----------|------|
| QKV output (M × hidden = 512 × 1536 × fp32) | ~3.0 MB |
| Attention output | ~3.0 MB |
| FFN intermediate (512 × 8960 × fp32) | ~17.3 MB |
| KV slice working buffer | ~0.25 MB |
| **Prefill peak** | **~24-25 MB** |
| **With staging + alignment** | **32 MB** |

The FFN intermediate at 17.3 MB dominates. Confirmed our prefill
batching design holds at chunk M=512 — even with the FFN scratch
included, total stays under 32 MB and well under the smallest card's
1 GB pool budget.

### FA-lite optimized path scratch

| Component | Size |
|-----------|------|
| Reduction buffer (optional) | 1-4 MB |
| Softmax scratch scalars | negligible |
| K/V tiled staging | workgroup memory only |
| **Device scratch** | **4-8 MB max** |

FA-lite's working set lives almost entirely in workgroup memory which
is NOT pool-allocated. Device-memory scratch is tiny. Pool sizing
should be dominated by the prefill + fallback requirements, not by
FA-lite.

### Per-card pool sizing

Policy: `scratch_pool = min(20% VRAM, 2.5 GB cap)`

| GPU | VRAM | Suggested pool |
|-----|------|----------------|
| P100 | 16 GB | 2.0 GB |
| P40 | 24 GB | 3.0 GB |
| M40 | 24 GB | 3.0 GB |
| GTX 1070 | 8 GB | 1.0-1.2 GB |
| P4 | 8 GB | 0.8-1.0 GB |
| Unknown | assume 8 GB | 0.8 GB |

Hard floor: 128 MB minimum viable pool (covers fallback + prefill
+ staging + fragmentation buffer).

For the cesarops fleet:
- T440 P100 16 GB: 2.0 GB pool
- cesarops2 GTX 1070 8 GB: 1.0-1.2 GB pool

Both well above the hard floor. No card on our fleet is at risk.

## Polish notes for integration

### 1. Buffer usage flags need adjustment

Their skeleton uses:
```
usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC
```

Missing `INDIRECT` and `UNIFORM` which we may want for future
indirect-dispatch and uniform-via-pool patterns. Even if we don't
need them today, allocating with restrictive flags forces a re-allocation
when we add features. Recommendation:
```
usage: BufferUsages::STORAGE
     | BufferUsages::COPY_DST
     | BufferUsages::COPY_SRC
     | BufferUsages::UNIFORM
     | BufferUsages::INDIRECT
```

`UNIFORM` is mostly free on Pascal Vulkan but the validation layer
will block uniform-binding views into a STORAGE-only buffer. Worth
including up front.

### 2. AtomicUsize ordering

Their skeleton uses `Ordering::Relaxed` for the bump pointer. This is
correct for single-threaded use (one forward pass at a time per pool)
but if we ever parallelize forward passes (multi-model concurrent
inference, which IS in our roadmap) we need `Ordering::AcqRel` or
explicit per-pool locking. For now Relaxed is fine, but flag for
multi-model integration:

> When multi-model registry lands and we have concurrent forward
> passes against different models on the same GPU, each model's
> forward pass needs its own scratch pool OR the bump pointer needs
> upgraded ordering + a guard that ensures forward passes don't
> interleave their alloc/reset cycles.

Cleanest fix: per-model scratch pools (matches the multi-model design
where each loaded model owns its execution context). Pool sizing
divides 20% VRAM budget across the loaded models.

### 3. The free list never gets used in practice

The contributor included a free list for "fallback for out-of-order
lifetimes," but with forward-pass-scope reset, every allocation has
the same lifetime (until the next reset). The free list path is
dead code unless we have mid-forward-pass deallocation, which we
don't.

**Recommendation:** Drop the free list entirely. Pure bump allocator
with reset. ~30 LOC simpler. If we ever need free-then-reuse mid-pass
later, add it then. Current design is YAGNI.

The contributor's own note says "free-list fallback (only for
out-of-order lifetimes if needed)" — they flagged it as conditional.
Our condition is "no out-of-order lifetimes," so drop it.

### 4. parking_lot::Mutex dependency

Their skeleton imports `parking_lot::Mutex`. We're already on
`parking_lot` elsewhere in the engine, so this is fine — but if the
free list is dropped per polish note 3, the mutex goes with it and
the pool becomes lock-free.

### 5. Alignment handling

Their `alloc()` uses caller-supplied alignment. For Vulkan this
should default to `minStorageBufferOffsetAlignment` from
`VkPhysicalDeviceLimits`. On Pascal this is typically 256 bytes.
On AMD it can be larger (some cards report 64 bytes, some 256).

**Recommendation:** Cache the device's `min_storage_buffer_offset_alignment`
in the pool at construction. Default `alloc()` align to that value.
Allow caller override only when they know the resource needs tighter
alignment (e.g., uniform buffer slices need
`minUniformBufferOffsetAlignment` which is often higher).

### 6. Synchronization correctness — the buried correctness flag

Their final correctness flag is the most important one and easy to
miss:

> "You need: fence or encoder completion before `reset()`. Otherwise:
> reuse during in-flight dispatch = silent corruption."

Our current submit pattern submits a command buffer then immediately
proceeds. wgpu's submit returns a `SubmissionIndex` that can be passed
to `device.poll(Maintain::WaitForSubmissionIndex(idx))` to wait for
GPU completion. The pool's `reset()` MUST happen AFTER the previous
forward pass's submission completes.

**Integration plan:**
- `ScratchPool::reset()` becomes private
- New `ScratchPool::reset_after(submission_idx, device)` that polls
  for completion before resetting
- Forward pass returns its `SubmissionIndex` to the caller
- Caller calls `pool.reset_after(idx, device)` before starting the
  next forward pass

This is the same pattern wgpu_hal will let us upgrade to fence-based
later. Today, polling submission completion via `Maintain::WaitForSubmissionIndex`
is the correct wgpu-safe approach.

### 7. ScratchManager wrapping two pools

Their ScratchManager pseudocode is one line:
```
ScratchManager
  ├── optimized_pool (FA-lite)
  └── fallback_pool (3-kernel)
```

with "only one is instantiated per GPU/model selection."

Concrete shape:
```rust
pub enum ScratchManager {
    Optimized { fa_lite_pool: ScratchPool, prefill_pool: ScratchPool },
    Fallback  { fallback_pool: ScratchPool, prefill_pool: ScratchPool },
}
```

The `prefill_pool` is shared structure (same sizing on both paths),
the attention pool differs. Forward-pass code dispatches based on
the variant.

This shape integrates cleanly with the OptimizationProfile struct
from prompt #1 (still outstanding). When the profile is selected at
load time, the matching ScratchManager variant is constructed.

### 8. Failure mode wiring

Their three-tier fallback strategy:
1. Reject forward pass at model load time (preferred)
2. Reduce prefill chunk size (M: 512 → 256) (acceptable)
3. Disable FA-lite path → fallback path only (last resort)

Implementation needs a budget calculator at model load:
```rust
fn calculate_required_scratch(
    model: &ModelMeta,
    chunk_size: u32,
    profile: &OptimizationProfile,
) -> Result<u64, ScratchOverrun>;
```

Caller decides: try chunk_size=512, if it overruns try 256, if 256
overruns try 128, if 128 overruns refuse model load with a clear
"VRAM insufficient for this model on this GPU" error. The auto-step-down
wraps this nicely as `attempt_load_with_chunk_search()`.

This is also the right surface for the `--bench` mode to measure:
once benchmarking is wired, recording the highest chunk size that
fit per (GPU, model) combination becomes part of the leaderboard.

## Integration sequence (when this lands)

1. Drop `ScratchPool` (without free list, with cached alignment)
   into `cesarops-inference/src/memory/scratch_pool.rs`. ~80 LOC
   after dropping the free list.
2. Drop `ScratchManager` enum into the same module. ~30 LOC.
3. Wire `forward_pass.rs::execute_layer` to allocate fallback scores
   buffer from the manager instead of the current per-head buffer
   creation pattern (which is one of the Tier 1 bottlenecks identified
   in the bottleneck analysis).
4. Wire decode-mode prefill scratch through the manager.
5. Add the budget calculator + auto-step-down chunk search to
   model load.
6. Smoke test through both gates.
7. Multi-model integration: per-model ScratchManager, sized as a
   fraction of the GPU's pool budget.

Estimate: ~1 day for steps 1-4 once we're ready to integrate. Steps
5-7 chain into the multi-model + bench mode work.

## Where this slots vs other queued work

Updated priority order:

1. **Multi-model loading registry** (in-progress)
2. **`--bench` mode + real EngineBenchmarker** (queued)
3. **Q6_K K/V/Q proj+RoPE+cache fusion** (V kernel ready, K/Q after partner_row check)
4. **Prefill batching mode** (v1 design stashed)
5. **Attention scratch pool ← THIS DROP** (skeleton ready, blocks the per-head buffer Tier 1 bottleneck fix and prefill batching)
6. **wgpu_hal::vulkan submit-path port** (architecture approved)

Note the move: attention scratch pool slots in BEFORE prefill batching
because prefill batching depends on the scratch pool being there.
The per-head buffer allocation in current attention dispatch is also
a Tier 1 bottleneck the pool fixes regardless of prefill — so
this drop has standalone value too.

## What this drop unblocks

- Tier 1 bottleneck #2 (per-head fresh buffer allocation in attention
  dispatch) — fixed by allocating from the pool instead of fresh
  per-head buffers per dispatch
- Prefill batching — gives prefill mode a place to put its FFN
  intermediate + QKV output + attention output buffers
- Multi-model concurrent inference — clean per-model scratch
  isolation
- FA-lite kernel landing — provides the small device-memory
  reduction buffer FA-lite needs

That's a lot of integrations enabled by ~110 LOC of allocator code.
This is the highest-leverage drop the cluster has produced so far.

## Their offered next step

> "If you want next step, I can map this directly onto:
> - Vulkan VkDeviceMemory heap segmentation
> - or a wgpu_hal::vulkan-friendly version with explicit memory types
>   + bindless offsets"

Both are useful for the wgpu_hal port. The "explicit memory types +
bindless offsets" version is the one we want — bindless offsets
mean we bind the pool buffer once per pipeline and pass the offset
via push constants per dispatch, eliminating bind-group churn (which
is one of the dispatch-overhead components our submit-storm pays
for today).

**Recommended next prompt to cluster:** map this allocator design to
the wgpu_hal::vulkan version with explicit memory type selection
(device-local heap for scratch, host-visible for staging) and
bindless offsets for per-dispatch buffer slicing. Tee up alongside
prompt #1 (capability detection profiles) so they integrate cleanly
when the wgpu_hal port lands.

---

## Verbatim source — pool skeleton

Their Rust skeleton, captured verbatim. **Apply polish notes 1-7
before integration** (drop free list, expanded usage flags, cached
alignment, sync-aware reset).

```rust
use std::sync::Arc;

pub struct ScratchPool {
    device: Arc<wgpu::Device>,
    buffer: wgpu::Buffer,

    size: usize,
    offset: std::sync::atomic::AtomicUsize,

    // optional free list for fallback reuse
    free_list: parking_lot::Mutex<Vec<(usize, usize)>>, // (offset, size)
}

#[derive(Clone, Copy, Debug)]
pub struct ScratchAlloc {
    pub offset: u64,
    pub size: u64,
}

impl ScratchPool {
    pub fn new(device: Arc<wgpu::Device>, size: usize, label: &str) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: size as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        Self {
            device,
            buffer,
            size,
            offset: std::sync::atomic::AtomicUsize::new(0),
            free_list: parking_lot::Mutex::new(Vec::new()),
        }
    }

    pub fn reset(&self) {
        self.offset.store(0, std::sync::atomic::Ordering::Relaxed);
        self.free_list.lock().clear();
    }

    pub fn alloc(&self, size: usize, align: usize) -> Option<ScratchAlloc> {
        let mut free = self.free_list.lock();

        // simple first-fit reuse
        if let Some(pos) = free.iter().position(|&(_, s)| s >= size) {
            let (off, _) = free.swap_remove(pos);
            return Some(ScratchAlloc {
                offset: off as u64,
                size: size as u64,
            });
        }

        let mut off = self.offset.load(std::sync::atomic::Ordering::Relaxed);

        let align_off = (off + align - 1) & !(align - 1);

        if align_off + size > self.size {
            return None;
        }

        self.offset.store(align_off + size, std::sync::atomic::Ordering::Relaxed);

        Some(ScratchAlloc {
            offset: align_off as u64,
            size: size as u64,
        })
    }

    pub fn free(&self, alloc: ScratchAlloc) {
        self.free_list
            .lock()
            .push((alloc.offset as usize, alloc.size as usize));
    }

    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }
}
```

---

Filed under research_log because the design is integration-ready
after polish notes 1-7 are applied (~30 LOC simpler than the verbatim
skeleton). Sizing tables are concrete and operationally usable.
Decisions are defended with sound rationale. This is the highest-quality
drop the cluster has produced so far.
