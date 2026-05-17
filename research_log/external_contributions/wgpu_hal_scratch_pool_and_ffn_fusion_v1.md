# wgpu_hal Memory-Type-Aware Scratch Pool + Fused FFN — Combined Drop

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-17. Two-message delivery (truncated mid-classify_memory_type
on first send, completed on second send). Combined here.
Status: **REFERENCE — INTEGRATION-READY architecture with TWO ANTI-PATTERN
FLAGS that override our prior decisions.** The classify_memory_type
function and the FFN fusion design are usable. Two specific recommendations
in this drop CONFLICT with what we already locked in (free-list reactivation,
device-name string matching, two-pool model collapse).

## Verdict

The classify_memory_type body is concrete, the FFN fusion design is
the missing piece our queued work needed, and the dispatch-reduction
impact (4 dispatches → 1) lines up with our integration target of
6 dispatches per layer.

But three of their decisions actively conflict with what we already
adopted from prior cluster drops:

1. **Re-introduces the free-list / `offset += size` pattern** without
   forward-pass-scope reset. We rejected this in the
   attention_scratch_pool_v1 stash polish notes. Their `alloc()`
   moves the bump pointer permanently — there's no `reset_each_pass()`,
   only `reset_each_layer()`. This is an architecture regression.

2. **Uses device-name string matching** for classification
   (`device_name.contains("P100")`). We explicitly rejected this in
   the llamacpp_capability_detection_v1_part1 stash polish-1 ("vendor
   ID is for logging only, not kernel selection"). Feature-bitfield
   classification is the agreed approach. The cluster knows this —
   they wrote that polish themselves on the prior drop. They've now
   contradicted it.

3. **Their two-pool model differs from ours.** We adopted
   "Optimized vs Fallback path pools" (one chosen at load time per
   capability profile). They propose "Fast vs Persistent pools"
   (both active simultaneously, different lifecycles). The two
   models are orthogonal — ours is about path selection, theirs is
   about lifetime tiers. Both can coexist; we need to merge them
   carefully.

Adopt the framework, override the three conflicts. Details below.

## What's right (accepted)

### Memory tier abstraction

```rust
enum ScratchMemoryTier {
    DeviceLocalFast,     // VRAM, optimal for KV + FFN intermediates
    DeviceLocalShared,   // fallback GPU local, lower priority
    HostVisibleStaging,  // only for upload / rarely readback
}
```

Adopt as-is. The three-tier model maps cleanly onto Vulkan memory
type flags and our existing pool design.

### ScratchLayout output struct

```rust
struct ScratchLayout {
    device_local_fast_heap: u32,
    device_local_shared_heap: u32,
    host_visible_heap: u32,

    fast_heap_bytes: u64,
    shared_heap_bytes: u64,

    preferred_alignment: u64,
    supports_dedicated_allocation: bool,

    tier: ScratchMemoryTier,
}
```

Useful struct shape. Our `OptimizationProfile` struct from the
capability detection rework gains a `scratch_layout: ScratchLayout`
field.

### Heap classification logic (the real win)

The classifier walks Vulkan memory heaps and buckets them by:
- `MemoryHeapFlags::DEVICE_LOCAL` → device-local (split fast vs shared
  by size threshold)
- otherwise → host-visible

That logic is right. The size threshold (8 GB) is a Pascal-NVIDIA
heuristic — large heaps are typically the dedicated VRAM pool, smaller
heaps are the BAR / shared / fallback pool. P100 16 GB → 1 large
heap → fast tier. GTX 1070 8 GB → either 1 large heap (most cases)
or split into smaller pools (occasionally). Either way the bucket
assignment is sound.

### FFN fusion architecture

```
FFN intermediate must NOT hit global memory:
  x → register
  h → shared memory (optional)
  activated → register
  y → register
Only final output writes global.
```

Correct. Standard fused-FFN pattern. For our Qwen 1.5B with
intermediate_dim=8960, hidden_dim=1536:
- Input x: 1536 fp16 = 3 KB (fits in registers per workgroup)
- W1 output h: 8960 fp16 = 17.5 KB (workgroup memory, fits)
- W2 output y: 1536 fp16 = 3 KB (fits in registers)

Workgroup memory budget on Pascal is 48 KB, so 17.5 KB for h is
comfortable. This eliminates the 17 MB FFN intermediate buffer that
currently dominates our prefill scratch.

### Dispatch reduction

Before FFN fusion: matmul1 + activation + matmul2 + add = 4 dispatches
After FFN fusion: 1 dispatch

Combined with our other queued fusions, total per-layer dispatch budget:
- rmsnorm (1)
- fused_qkv_rope_cache (1, replaces 8)
- fused_attention (1, replaces 12 across heads)
- output_proj_bias (1)
- rmsnorm_post (1)
- fused_ffn (1, replaces 4)
= **6 dispatches per layer**

Matches our target from the lessons_learned ladder. ~168 dispatches
per token vs current ~476.

### SwiGLU support in FFN kernel

Their kernel sketch:
```wgsl
let activated = silu(h) * gelu_gate(h);
```

Wait — that's wrong notation but the right intent. SwiGLU is
`silu(W_gate(x)) * W_up(x)`, NOT `silu(h) * gelu_gate(h)`. They're
mixing GeLU and SiLU and the multiplication structure is for SwiGLU
which uses TWO separate W matrices (gate + up), not one. See polish
note #4.

Their structure shows the right *fusion* concept (gate computation
fused into activation step) but the math expression is sloppy.

### 3-stream memory architecture insight

> "Stream 1: KV cache (bandwidth-bound, persistent)
>  Stream 2: FFN (compute-bound, fused)
>  Stream 3: attention scratch (subgroup windowed)"

Useful framing for the final architecture. Each stream has different
lifetime, different placement preference, different access pattern.
Adopt as the lessons_learned design principle for memory architecture.

## What's wrong / overrides required

### CONFLICT 1: Their `alloc()` is bump-only with NO forward-pass reset

```rust
fn alloc(&mut self, size: u64) -> ScratchSlice {
    let aligned = align(size, self.layout_alignment());
    let start = self.offset;
    self.offset += aligned;
    ScratchSlice { start, size: aligned }
}

fn reset_layer(&mut self) {
    if self.reset_each_layer {
        self.offset = 0;
    }
}
```

There's no `reset_after_pass()` or `reset_after(submission_idx)`.
Once `reset_each_layer = false` the offset just grows forever.

**Override:** Use the version we already approved from the
attention_scratch_pool_v1 stash:
- `alloc()` does bump
- `reset_after(submission_idx, device)` polls submission, then resets
- Forward-pass-scope reset is the canonical cadence
- No free list (YAGNI per prior lessons)

The "reset per layer vs reset per pass" question they're trying to
solve via the boolean flag is the wrong abstraction. Use lifetime
tiers in the *layout*, not lifetimes in the *allocator*. The layer-scope
allocations (FFN intermediates) come from one pool; persistent
allocations (KV cache) come from a separate buffer that's not part
of the scratch pool at all.

### CONFLICT 2: device-name string matching is the rejected pattern

```rust
let is_p100_family = device_name.contains("P100");
let is_pascal = device_name.contains("GTX 1070")
    || device_name.contains("Pascal");
```

We rejected this in `llamacpp_capability_detection_v1_part1.md`
polish note #1:

> "Identity fields (vendor_id, device_id, device_name, driver_version)
>  are useful for logging and debugging, not for kernel selection.
>  Vendor branching is an anti-pattern that will burn us when (a) a
>  new NVIDIA card has different features or (b) an AMD card with
>  the same features as our Pascal targets shows up."

Their classify_memory_type would mis-classify:
- A "Tesla P40" (Pascal sm_61, 24 GB) — `is_p100` is FALSE,
  `is_pascal` is FALSE because "Pascal" not in name. Falls through
  to AMD/unknown branch. Wrong.
- A "Quadro P5000" — same Pascal architecture, doesn't match either
  string. Falls through. Wrong.
- A "Tesla P100-PCIE-16GB" — matches "P100" → goes to P100 branch.
  Correct, but only by accident of string match.

**Override:** Replace device-name matching with feature-bitfield
classification. The `OptimizationProfile.card_class` from the
capability detection rework already buckets to PascalHighEnd /
PascalLowEnd / Maxwell / RDNA / VegaLike / Unknown. Pass the card_class
to classify_memory_type instead of the raw device name.

```rust
fn classify_memory_type(
    mem_props: &vk::PhysicalDeviceMemoryProperties,
    card_class: CardClass,
    capabilities: &InferredCapabilities,
    vram_bytes: u64,
) -> ScratchLayout {
    // Step 1: heap classification (theirs, KEEP)
    [...]

    // Step 2: tier selection by card_class, not name
    match card_class {
        CardClass::PascalHighEnd => {
            // P100 / P40 / similar HBM2 + large VRAM
            ScratchLayout {
                fast_heap_bytes: vram_bytes,
                shared_heap_bytes: vram_bytes / 4,
                preferred_alignment: 128,
                supports_dedicated_allocation: true,
                tier: ScratchMemoryTier::DeviceLocalFast,
                ..
            }
        }
        CardClass::PascalLowEnd => {
            // 1070 / P4 / similar GDDR5 + smaller VRAM
            ScratchLayout {
                fast_heap_bytes: vram_bytes / 2,
                shared_heap_bytes: vram_bytes / 3,
                preferred_alignment: 128,
                supports_dedicated_allocation: false,
                tier: ScratchMemoryTier::DeviceLocalShared,
                ..
            }
        }
        CardClass::RDNA | CardClass::VegaLike => {
            // AMD wave64 alignment preference
            ScratchLayout {
                fast_heap_bytes: vram_bytes / 2,
                shared_heap_bytes: vram_bytes / 2,
                preferred_alignment: 256,
                supports_dedicated_allocation: false,
                tier: ScratchMemoryTier::DeviceLocalShared,
                ..
            }
        }
        _ => /* fallback */
    }
}
```

### CONFLICT 3: their two-pool model is a different axis

Their model:
```
ScratchManager
  ├── fast_pool        // per-layer reuse, reset_each_layer = true
  └── persistent_pool  // across tokens, reset_each_layer = false
```

Our existing model (from attention_scratch_pool_v1):
```
enum ScratchManager {
    Optimized { fa_lite_pool, prefill_pool },
    Fallback  { fallback_pool, prefill_pool },
}
```

These solve different problems:
- Theirs: lifetime tiering (per-layer ephemeral vs cross-token
  persistent)
- Ours: capability-path selection (FA-lite vs 3-kernel fallback)

We need BOTH axes. Merged shape:

```rust
pub struct ScratchManager {
    // Path selection (load-time, fixed for model lifetime)
    path: ExecutionPath,

    // Lifetime tiers (allocation/reset cadences differ)
    fast_pool: ScratchPool,         // FFN intermediates, attention scores - per-pass reset
    persistent_pool: ScratchPool,   // KV cache - never reset (KV cache doesn't actually live here, but cross-pass scratch could)

    layout: ScratchLayout,
}

pub enum ExecutionPath {
    Optimized,
    Fallback,
}
```

The `path` determines which kernels dispatch (FA-lite vs split-by-head).
The `fast_pool` / `persistent_pool` split determines lifetime. The
`layout` determines memory placement.

Three orthogonal axes, all needed. The cluster's collapse to just
"fast vs persistent" misses the path-selection axis.

KV cache does NOT live in either pool — it's a separate persistent
buffer with token-level lifetime, owned by the model context not the
scratch manager. Their note "KV persistent region (never reset)"
inside ScratchManager is wrong — KV cache shouldn't be pool-allocated.

## Polish notes for integration

### 1. The 8 GB heap-size threshold is fragile

```rust
if heap.size > 8 * 1024 * 1024 * 1024 {
    device_local_fast = Some((i, heap.size));
} else {
    device_local_shared = Some((i, heap.size));
}
```

What about an 8 GB GTX 1070? Its single device-local heap is exactly
8 GB → comparison is `8 GB > 8 GB` = false → falls into shared bucket.
That's wrong — for the 1070 the 8 GB heap IS the fast heap.

**Fix:** Use `>=` or change threshold. Better: don't threshold by
size at all. If there's exactly one device-local heap, it's the fast
heap. If there are multiple, the largest is fast and the rest are
shared. If there are zero (integrated graphics), there's no
device-local fast tier and we fall back to host-visible.

```rust
let mut device_local_heaps: Vec<_> = ... collect device-local heaps ...;
device_local_heaps.sort_by(|a, b| b.1.cmp(&a.1));  // largest first

let fast = device_local_heaps.first().copied();
let shared = device_local_heaps.get(1).copied().or(fast);
```

### 2. `unwrap()` on `device_local_fast` will panic on integrated GPUs

```rust
device_local_fast_heap: device_local_fast.unwrap().0 as u32,
```

If a system has only host-visible memory (integrated GPU, no
discrete VRAM), this panics. Replace with proper Result return or
explicit fallback to host-visible.

### 3. SwiGLU math expression is wrong

```wgsl
let activated = silu(h) * gelu_gate(h);
```

SwiGLU is `silu(W_gate · x) ⊙ (W_up · x)`. Two separate weight
matrices, both applied to `x`, then element-wise multiply. Their
expression `silu(h) * gelu_gate(h)` makes no sense — `gelu_gate` is
not a real function and applying both silu and gelu to the same h
isn't what SwiGLU does.

Correct fused FFN for SwiGLU (used by Qwen, Llama, Mistral):

```wgsl
@compute @workgroup_size(SUBGROUP_SIZE)
fn fused_ffn_swiglu() {
    let x = load_input();

    let gate = matmul_w_gate(x);    // W_gate · x
    let up   = matmul_w_up(x);      // W_up · x

    let activated = silu(gate) * up;  // SwiGLU element-wise

    let down = matmul_w_down(activated);  // W_down · activated

    let out = down + x;  // residual

    write_output(out);
}
```

Three matmuls (gate, up, down), one elementwise activation, one
residual. Standard. Their version had two matmuls (matmul_w1,
matmul_w2) which is GeLU FFN, not SwiGLU. Different model
architecture.

For Qwen we need SwiGLU. For older Llama-1 / GPT-J we'd need GeLU.
The kernel needs to know which.

**Fix:** Make activation pluggable via spec constant or push-constant
flag. One shader, runtime branch on `flags & ACTIVATION_MASK`:
- ACTIVATION_GELU → standard GeLU FFN (2 matmuls)
- ACTIVATION_SWIGLU → SwiGLU FFN (3 matmuls)
- ACTIVATION_RELU → minimal (2 matmuls)

### 4. Workgroup memory budget for FFN intermediate

For Qwen 1.5B, `h` (FFN intermediate after gate × up) is 8960 elements.
At fp16 that's 17.5 KB. Pascal workgroup memory limit is 48 KB.

But `gate` and `up` are SEPARATE intermediates that we hold while
computing the elementwise multiply. So we need:
- gate: 8960 fp16 = 17.5 KB
- up:   8960 fp16 = 17.5 KB
= 35 KB before elementwise multiply.

After multiply, `activated` overwrites either gate or up (in-place):
= 17.5 KB ongoing.

35 KB peak fits in 48 KB Pascal limit but leaves only 13 KB for other
shared-memory uses. Tight. Their kernel design needs explicit
shared-memory layout planning, NOT just "shared memory (optional)"
hand-waving.

**Fix:** Use chunked computation — compute gate × up element-by-element
in workgroup-sized chunks, never holding both full vectors in shared
memory. For chunk size 128:
- gate_chunk: 128 fp16 = 256 B
- up_chunk:   128 fp16 = 256 B
- multiply, accumulate into matmul_w_down
= ~512 B working set instead of 35 KB.

This is the standard "streaming SwiGLU" pattern. ~50 LOC more shader
code, ~70× less shared-memory pressure.

### 5. Their performance projection table is consistent with ours

| State | t/s (P100) |
|-------|------------|
| Current | 2.2 |
| + KV + dispatch fixes | 8-14 |
| + FFN fusion + scratch | 12-20 |

Matches our internal ladder almost exactly (we had 12-25 t/s for
"+ wgpu_hal submit path"; their +20 t/s for FFN+scratch is close).
The "FFN+scratch" combined gain ~+50% on top of KV+dispatch is
realistic — FFN is the next-largest compute consumer after attention.

### 6. The "next bottleneck class" is dual-kernel pipelined execution

Their final note: "dual-kernel pipelined execution (overlap FFN and
attention across warps)". Pascal supports concurrent kernel execution
on the same SM IF the kernels are dispatched on independent streams
AND there's enough register/shared-memory headroom. WGSL/Vulkan
exposes this via async compute queues, which we explicitly rejected
in the wgpu_hal_vulkan_port_scoping stash ("Avoid timeline semaphore
complexity, async compute queue dependence, multi-queue assumptions").

So the offered "next upgrade" is on the rejected side of our
architecture. Decline if offered explicitly. The dispatch overhead
reduction we get from wgpu_hal + the kernel fusion we already have
gets us to the same Pascal ceiling (~18-25 t/s) without the multi-queue
complexity.

### 7. KV cache placement guidance is missing

The drop talks about FFN scratch placement (DeviceLocalFast for HBM2,
DeviceLocalShared for GDDR5) but doesn't address KV cache placement.
KV cache is the single largest device-local allocation we have
(~234 MB at 4096 ctx for our model in fp16). It should always be
DeviceLocalFast — it's accessed every layer every token.

Polish: explicit `kv_cache_tier: ScratchMemoryTier::DeviceLocalFast`
in the layout, used by tensor_loader_safe.rs to allocate the KV cache
in the right heap.

### 8. `supports_dedicated_allocation` flag

Their P100 layout: `supports_dedicated_allocation: true`. Their 1070
layout: `supports_dedicated_allocation: false`.

Vulkan dedicated allocations (`VK_KHR_dedicated_allocation`) tell the
driver this buffer is large enough to deserve its own VkDeviceMemory.
Both Pascal cards support the extension; the difference here is
"should we use it" not "can we." The drop's heuristic of using
dedicated allocation only on the larger-VRAM card is reasonable but
unmotivated — track as a perf-tuning iteration after the basic pool
lands.

## Integration sequence (when this lands)

Slot in the queue after attention scratch pool and capability detection
but before prefill batching:

1. Multi-model registry (in-progress)
2. `--bench` mode
3. Q6_K K/V/Q proj+RoPE+cache fusion (V kernel ready)
4. Attention scratch pool (with overrides 1+3 above)
5. Capability detection + OptimizationProfile
6. KV cache fp16 + 128B prefetch (Stage #11)
7. **classify_memory_type + ScratchLayout extension** ← from this drop
8. **Fused SwiGLU FFN kernel** ← from this drop
9. Prefill batching mode
10. wgpu_hal::vulkan submit path

Steps 7 and 8 land together — FFN fusion is the consumer for the
memory-type-aware pool placement.

## Where this drop closes our gap list

Two known gaps closed:
- ✓ wgpu_hal-friendly memory-type-aware pool variant (with overrides)
- ✓ Fused FFN kernel design (with SwiGLU correction)

Remaining cluster-prompt gaps:
- None outstanding. All 6 original prompts answered.
- Implicit follow-ups (KV prefetch #11, this drop) also delivered.
- Stage #12 (megakernel) explicitly rejected.

The cluster has now delivered everything we asked for. Next prompts
when the operator re-engages would be:
- Refine SwiGLU FFN kernel to chunked-streaming pattern (per polish #4)
- Validate classify_memory_type override fits actual heap layouts on
  P100 + 1070 (we'd run a probe and check)
- Reference implementation for `reset_after(submission_idx)` pattern
  in pure wgpu (before wgpu_hal port lands)

---

## Verbatim source — combined drop, polish-pending

### ScratchLayout struct

```rust
struct ScratchLayout {
    device_local_fast_heap: u32,
    device_local_shared_heap: u32,
    host_visible_heap: u32,

    fast_heap_bytes: u64,
    shared_heap_bytes: u64,

    preferred_alignment: u64,
    supports_dedicated_allocation: bool,

    tier: ScratchMemoryTier,
}
```

### ScratchMemoryTier enum

```rust
enum ScratchMemoryTier {
    DeviceLocalFast,     // VRAM, optimal for KV + FFN intermediates
    DeviceLocalShared,   // fallback GPU local, lower priority
    HostVisibleStaging,  // only for upload / rarely readback
}
```

### classify_memory_type (NEEDS POLISH 1+2 + CONFLICT-2 OVERRIDE)

```rust
fn classify_memory_type(
    mem_props: &vk::PhysicalDeviceMemoryProperties,
    device_name: &str,
    vram_bytes: u64,
) -> ScratchLayout {

    let mut device_local_fast = None;
    let mut device_local_shared = None;
    let mut host_visible = None;

    // Step 1: classify memory heaps
    for (i, heap) in mem_props.memory_heaps.iter().enumerate() {
        let is_device_local = heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL);

        if is_device_local {
            // Heuristic split: fast vs shared GPU local
            if heap.size > 8 * 1024 * 1024 * 1024 {
                device_local_fast = Some((i, heap.size));
            } else {
                device_local_shared = Some((i, heap.size));
            }
        } else {
            host_visible = Some((i, heap.size));
        }
    }

    // Step 2: enforce platform heuristics
    let is_p100_family = device_name.contains("P100");
    let is_pascal = device_name.contains("GTX 1070")
        || device_name.contains("Pascal");

    // P100: HBM2, massive bandwidth, prefers large contiguous device-local
    if is_p100_family {
        return ScratchLayout {
            device_local_fast_heap: device_local_fast.unwrap().0 as u32,
            device_local_shared_heap: device_local_fast.unwrap().0 as u32,
            host_visible_heap: host_visible.unwrap().0 as u32,

            fast_heap_bytes: vram_bytes,
            shared_heap_bytes: vram_bytes / 4,

            preferred_alignment: 128,
            supports_dedicated_allocation: true,

            tier: ScratchMemoryTier::DeviceLocalFast,
        };
    }

    // GTX 1070: split VRAM pressure + weaker L2 + display sharing
    if is_pascal {
        return ScratchLayout {
            device_local_fast_heap: device_local_fast.unwrap().0 as u32,
            device_local_shared_heap: device_local_shared.unwrap_or(device_local_fast.unwrap()).0 as u32,
            host_visible_heap: host_visible.unwrap().0 as u32,

            fast_heap_bytes: vram_bytes / 2,
            shared_heap_bytes: vram_bytes / 3,

            preferred_alignment: 128,
            supports_dedicated_allocation: false,

            tier: ScratchMemoryTier::DeviceLocalShared,
        };
    }

    // fallback (AMD / unknown)
    ScratchLayout {
        device_local_fast_heap: device_local_fast.unwrap_or((0, 0)).0 as u32,
        device_local_shared_heap: device_local_shared.unwrap_or((0, 0)).0 as u32,
        host_visible_heap: host_visible.unwrap_or((0, 0)).0 as u32,

        fast_heap_bytes: vram_bytes / 2,
        shared_heap_bytes: vram_bytes / 2,

        preferred_alignment: 256,
        supports_dedicated_allocation: false,

        tier: ScratchMemoryTier::DeviceLocalShared,
    }
}
```

### ScratchPool + ScratchManager (NEEDS CONFLICT-1+3 OVERRIDE)

```rust
struct ScratchManager {
    fast_pool: ScratchPool,        // per-layer reuse
    persistent_pool: ScratchPool,  // across tokens
    layout: ScratchLayout,
}

struct ScratchPool {
    buffer: Buffer,
    offset: u64,
    size: u64,
    reset_each_layer: bool,
}

impl ScratchPool {
    fn alloc(&mut self, size: u64) -> ScratchSlice {
        let aligned = align(size, self.layout_alignment());
        let start = self.offset;
        self.offset += aligned;
        ScratchSlice { start, size: aligned }
    }

    fn reset_layer(&mut self) {
        if self.reset_each_layer {
            self.offset = 0;
        }
    }
}
```

### Fused FFN kernel sketch (NEEDS POLISH 3+4 — SwiGLU correction + chunked streaming)

```wgsl
@compute @workgroup_size(SUBGROUP_SIZE)
fn fused_ffn() {
    let lane = subgroup_invocation_id();

    // load input activation
    let x = load_input(lane);

    // GEMM 1 (W1)
    let h = matmul_w1(x);

    // activation fused (NOTE: their notation is wrong - see polish #3)
    let activated = silu(h) * gelu_gate(h);

    // GEMM 2 (W2)
    let y = matmul_w2(activated);

    // residual add fused
    let out = y + x;

    write_output(out);
}
```

### Forward pass integration

```rust
fn forward_pass(token: Token, scratch: &mut ScratchManager) {
    scratch.fast_pool.reset_layer();

    qkv_rope(token, scratch);
    attention_kv(token, scratch);
    fused_ffn(token, scratch);  // NEW
    output_proj(token, scratch);
}
```

---

Filed under research_log because the architecture is integration-ready
after applying the three conflict overrides (free-list YAGNI + forward-pass
reset, feature-bitfield over device-name, three-axis manager merge)
and the four polish notes (heap threshold, panic on integrated, SwiGLU
math, chunked FFN streaming). Closes our two remaining cluster-prompt
gaps. All original prompts now answered.
