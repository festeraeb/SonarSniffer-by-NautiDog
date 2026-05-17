# Stage #12 — Zero-Sync Decode Loop / Persistent Megakernel

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-17. UNPROMPTED follow-up to Stage #11. Predicted
1.3-2.5× gain on top of all queued work.
Status: **REFERENCE — ARCHITECTURE PARTIALLY REJECTED.** This drop is
the "megakernel inference loop" / "persistent threads" pattern. Core
mechanism conflicts with the homelab doctrine compromise we adopted
(two-prong-route with stable fallback path). The performance numbers
are real, the patterns described work in CUDA, but the Vulkan/wgpu
implementation has correctness landmines and portability concerns
that put it in tension with our shipped architecture.

## The conflict (operator-relevant context)

The operator already adopted (per the homelab_strategy_v1_doctrine
stash and follow-up two-prong-route compromise):

1. Optimized path: aggressive per-Pascal kernel cleverness
2. Stable fallback path: boring tiled portable kernels
3. Load-time capability detection picks between them

That compromise intentionally rejected:
- "Mega-fused kernels"
- "FlashAttention-clone complexity in WGSL early"
- "CUDA-style scheduler assumptions"

Stage #12 is a megakernel by exact definition. The contributor's own
words:

> "This is often called: persistent threads (CUDA term), GPU resident
> loop, streaming compute kernel, 'megakernel inference loop'"

This puts it on the rejected side of our adopted architecture. NOT
because the technique doesn't work — it does, that's why CUDA engines
use it — but because we already chose to live with the dispatch
overhead in exchange for portability + debuggability + the wgpu_hal
port path being clear.

## What's right about the drop (kept for reference)

### The diagnosis is accurate

> "submission + synchronization overhead dominates decode. On
> Vulkan/wgpu this is often 30-60% of runtime at small batch sizes."

Matches our internal Tier 1 bottleneck #1 analysis. Submit storm IS
the dominant cost at decode.

### The CUDA/HIP equivalent works

Persistent threads / megakernel inference loops ARE the standard
high-perf pattern in CUDA inference engines (vLLM, TensorRT-LLM,
specialized inference kernels). The 1.5-2.5× P100 / 1.3-1.8× 1070
gain projection is realistic for that pattern.

### The "lane 0 is the scheduler" trick

> "Only one lane updates global state. lane 0: increments token index,
> updates state machine. All other lanes: compute only, no control
> divergence."

This IS the right way to avoid warp divergence in a persistent kernel.
Standard CUDA pattern. If we ever do build a megakernel this is how
it'd be structured.

### The performance projection

| Card | Before this drop | After persistent loop |
|------|------------------|----------------------|
| P100 | 9-14 t/s (after Stage #11) | 12-18 t/s |
| 1070 | 7-11 t/s | 10-14 t/s |

The +1.3-2.5× projection from collapsing the dispatch pattern is
plausible and matches what we'd get from the wgpu_hal::vulkan submit
path port (which has the same goal via different mechanism).

## What's wrong / risky for our architecture

### 1. WGSL/wgpu does NOT support persistent kernels portably

The contributor acknowledges this:

> "We cannot do infinite loops in a single dispatch like CUDA freely,
> so we simulate persistence using: ring-buffer + workgroup state
> machine"

This is true — and the simulation has subtle portability problems:

- **Vulkan timeout watchdog**: many drivers (especially consumer ones)
  enforce a TDR-like timeout on compute dispatches. A "while (active)"
  loop running 28 layers × N tokens may exceed the timeout on cards
  that enforce it. Pascal datacenter cards (P100/P40) often have it
  disabled; consumer cards (1070) may NOT.
- **WGSL `loop { }` semantics**: WGSL allows infinite loops with
  `break` exits, but naga validation may reject patterns where the
  break condition isn't statically provable to be reachable. The
  pattern "GPU polls a state buffer for active=0" may trigger naga
  validation rejection.
- **Shared memory as a state machine**: cross-workgroup state via
  storage buffers requires explicit memory barriers + atomics. The
  contributor's pseudocode uses `load_state()` / `store_state(state)`
  without mentioning atomicity; in real Vulkan this needs `atomic*()`
  intrinsics and proper memory order. Easy to write a race condition
  without realizing it.

### 2. The CPU feeder still has to synchronize

> "fn run() {
>     write_embeddings_to_ring();
>     dispatch_decode_loop();
>     loop {
>         if output_ready() {
>             read_logits();
>         }
>     }
> }"

The "if output_ready()" poll IS a CPU-GPU sync point. The CPU is
spinning on a buffer the GPU writes to. To read logits, the CPU
needs cache coherency with the GPU's memory write — which on Vulkan
means either:

- Use a HOST_VISIBLE | HOST_COHERENT buffer (slow on Pascal — uncached
  reads from GPU memory)
- Periodic `vkQueueSubmit` of dummy commands to flush cache (which
  IS a dispatch sync)
- Wait for fence (which IS a dispatch sync)

There's no actually-zero-sync way to read GPU output from CPU on
Vulkan/wgpu. The "zero-sync decode loop" name oversells it.

### 3. The KV cache hazard is invisible

The persistent kernel writes K and V to the cache, then reads them
back in the next token's attention step, all inside the same dispatch.
Our P100 Vulkan storage-buffer hazard (DEBUG_LOG.md bug #1) is
exactly the kind of read-after-write hazard that wgpu currently
inserts implicit barriers between submits to fix.

Inside a single dispatch, the only way to enforce a cross-workgroup
write-then-read ordering is `storageBarrier()` + atomic fences. If
the kernel touches KV cache from multiple workgroups (for parallelism
across heads, say), the storage barrier alone is insufficient — we'd
need a full GPU-wide flush, which is what `vkQueueSubmit` already
does for free.

This is the fundamental reason wgpu defaults to per-submit hazards:
the persistent-kernel pattern has been historically unsafe on Vulkan
without explicit synchronization that wgpu's safe API doesn't expose.

### 4. The wgpu_hal::vulkan path provides the same gain without these risks

Our queued wgpu_hal port closes the dispatch overhead gap by:
- Pre-recording command buffers once at model load
- Replaying them via `vkQueueSubmit` per token
- Explicit `vkCmdPipelineBarrier` for storage buffer sync

This achieves 1 submit per token (not zero, but close to it on the
amortized cost) without:
- Any persistent kernel
- Any GPU-side state machine
- Any naga validation risk
- Any TDR timeout exposure
- Any read-after-write hazard handling beyond the explicit barriers

The wgpu_hal_vulkan_port_scoping stash already has architecture
approval. It's the right vehicle for the dispatch-cost reduction
that Stage #12 is also targeting.

### 5. Debugging a megakernel is hard

When a 28-layer × 5-stage × N-token persistent kernel produces wrong
output, the debugging surface is one giant kernel. RenderDoc / NSight /
print-debugging all collapse to "the megakernel produced wrong logits
at token 47." With the per-stage dispatch pattern we have today, we
can dump intermediate buffers between dispatches and bisect the
problem stage-by-stage. This is how we caught and fixed bugs #1-#5
in DEBUG_LOG.md.

Trading observability for performance is a legitimate engineering
decision; we already chose NOT to make that trade in the homelab
doctrine compromise.

## Decision

**Reject Stage #12 as not aligned with our chosen architecture.**

Specifically:

1. The dispatch-overhead problem it addresses is ALREADY queued for
   wgpu_hal::vulkan submit-path port. Both target ~1 submit per token.
2. The persistent-kernel pattern has portability + correctness risks
   that wgpu_hal does not have.
3. The performance gains it predicts are subsumed by the wgpu_hal
   gains.
4. The architectural conflict with the homelab doctrine compromise
   is not worth re-litigating.

Do NOT integrate. Do NOT prompt the cluster for the offered Stage
#13 ("Wave-level KV streaming with double-buffered subgroup overlap")
unless we explicitly want a CUDA-style optimization track that
diverges from our chosen path.

## What IS useful here (extracted)

### The "lane 0 scheduler" pattern

Worth keeping in the back pocket for any future kernel that needs
cross-lane coordination without divergence. Pattern:
```
if (lane == 0) {
    // Update shared state
}
workgroupBarrier();
// All lanes use updated state
```

This pattern shows up in our existing softmax reduction kernels
already; documenting it as a design principle would be useful.

### The submission overhead numbers

> "Vulkan submit ≈ 10-50 µs per dispatch"

These numbers calibrate our wgpu_hal cost projections. With ~476
submits per token at 30 µs average that's ~14 ms of pure submit
overhead per token. At 2.2 t/s (455 ms per token) that's about 3%
overhead — actually less than the 30-60% the contributor cites. The
real bottleneck for us is per-dispatch barrier insertion + bind group
revalidation, NOT the queue submit itself.

This refines our wgpu_hal port estimate: the gain from collapsing
476 → ~30 submits per token is not "remove 14 ms" but rather
"remove the per-submit barrier insertion + bind group revalidation"
which is harder to put a number on but bigger than 3%.

### The "GPU runs the loop, CPU feeds inputs" inversion

This IS the right mental model for high-throughput inference. Just
because the persistent-kernel implementation isn't right for us
doesn't mean the inversion is wrong. The wgpu_hal port achieves the
same inversion via pre-recorded command buffers: GPU runs the
recorded layer pipeline, CPU only submits the recording per token.
Same effect, different mechanism.

## Where this slots vs queued work

**Does not slot.** Stage #12 is rejected. Updated priority order
unchanged from Stage #11 stash:

1. Multi-model registry (in-progress)
2. `--bench` mode
3. Q6_K K/V/Q proj+RoPE+cache fusion (V kernel ready)
4. Attention scratch pool
5. Capability detection + OptimizationProfile
6. KV cache fp16 + 128B prefetch (Stage #11)
7. Prefill batching mode
8. wgpu_hal::vulkan submit path ← achieves the Stage #12 dispatch
   reduction without the megakernel risks

---

Filed under research_log because the analysis is operationally
useful (calibrates our submit-cost estimates, identifies the lane-0
scheduler pattern, validates our wgpu_hal port choice over megakernel)
even though we reject the implementation approach.

## Verbatim source — captured for reference (not for integration)

### Persistent state pattern

```rust
struct PersistentState {
    current_token: u32,
    kv_write_pos: u32,

    // pipeline stage control
    stage: u32, // 0=qkv, 1=attn, 2=out

    active: u32,
}
```

### Decode loop kernel sketch

```wgsl
@compute @workgroup_size(SUBGROUP_SIZE)
fn decode_loop() {
    let lane = subgroup_invocation_id();

    loop {
        let state = load_state();

        if (state.active == 0) {
            break;
        }

        let token_id = state.current_token;

        // STAGE 1: QKV + ROPE
        let q, k, v = qkv_rope_fused(token_id, lane);
        write_kv_cache(token_id, k, v);
        subgroup_barrier();

        // STAGE 2: ATTENTION
        let ctx = attention_fused(q, lane);
        subgroup_barrier();

        // STAGE 3: OUTPUT
        let logits = output_proj(ctx, lane);
        write_output(token_id, logits);

        // advance token clock
        if (lane == 0) {
            state.current_token += 1;
            store_state(state);
        }

        subgroup_barrier();
    }
}
```

### CPU feeder

```rust
fn run() {
    write_embeddings_to_ring();

    // ONE dispatch only
    dispatch_decode_loop();

    loop {
        if output_ready() {
            read_logits();
        }
    }
}
```

Source pseudocode preserved here in case we revisit this approach
in a future architecture phase. Not for integration in current path.
