# Pascal Ceiling Analysis + Dual-Kernel Pipeline + Speculative Pitch

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-17. Three-part drop: dual-kernel pipelined execution
("Stage #12 take 2"), hard-truth on P100 ceiling, pitch for speculative
decoding as the real lever.
Status: **REFERENCE — HARD TRUTH ACCEPTED, DUAL-KERNEL PIPELINE
PARTIALLY REJECTED, SPECULATIVE DECODING PITCH ACCEPTED for separate
deep-dive (Drop 2 in this batch).**

## The hard truth section (most operationally important content)

This is the single most useful piece of the drop:

> "You cannot reach 60-70 tok/s on a P100 with a Gemma-class model
> via Vulkan alone unless you change at least one of: model size,
> precision, or execution regime."

The cluster has been promising perf ladders that creep toward
60+ t/s through pure kernel optimization. They've now admitted what
we'd been suspecting:

> "60-70 tok/s is not achievable in pure autoregressive decode on P100"

Their realistic single-stream bound for our class:

| Setup | P100 t/s |
|-------|----------|
| Current (our state) | 2-3 |
| Fully fused + KV + FFN | 10-18 |
| + pipeline overlap | 12-22 |
| + speculative decoding (2-model) | 25-45 effective |

So the **single-stream pure-decode ceiling on P100 is ~22 t/s.**

That's important because:
- Our existing ladder topped out at 35 t/s; that was optimistic. **22 t/s is the real ceiling.**
- Reaching koboldcpp's 60+ t/s requires speculative decoding,
  batching, or smaller model — kernel optimization alone cannot
  close the gap.
- This calibrates how much further we should chase per-card
  Pascal optimization beyond the queued work. Diminishing returns
  past the +pipeline-overlap point.

> "How koboldcpp/llama.cpp actually hit high tok/s numbers (spoiler:
> they usually don't on P100+big models)"

Three explanations they offer:
- Option A: 2B-or-smaller models, Q4/Q3, short context (<512 tokens)
- Option B: batching + speculative decoding (real engineering levers)
- Option C: prefill-not-decode benchmarks, or cached-prompt reuse

For our reference benchmarks (kobold P100 60 t/s on Qwen MoE) — we
should re-examine what was actually being measured. Likely Option C
(prefill) or Option A (smaller effective model since MoE only
activates a fraction of params per token).

**Adopt** as a lessons entry: do not chase 60+ t/s through pure
kernel optimization on P100 + dense 1.5B+ model. The ceiling is ~22 t/s
single-stream. Batching/speculative is the path past that.

## Dual-kernel pipeline (PARTIALLY REJECTED)

Their proposal:

> "Token N      → FFN stage
>  Token N+1    → Attention stage
>  Token N+2    → QKV stage
>  All simultaneously."

Three pipelines running in lockstep, ring-buffered communication
between them. Pipeline depth 3-6 tokens in flight.

### What's right

- The principle of overlapping pipeline stages to hide latency is
  sound. CPUs do it (instruction pipelining), modern GPUs do it
  (concurrent kernel execution).
- The "Kernel A produces, Kernel B consumes via ring buffer" pattern
  is the right architectural shape for cross-stage overlap.
- The performance projection (+10-30% over fully fused) is realistic
  IF the implementation works.

### What's wrong / risky

This is largely a re-skin of the rejected Stage #12 megakernel
pattern with a coat of paint:

- **Cross-kernel data hazards**: Kernel B reads from `qkv_ring`
  while Kernel A is still writing to it. Inside a single submit
  this needs explicit `vkCmdPipelineBarrier` between kernel
  invocations — which means the kernels are NOT actually overlapped
  in execution, they're just submitted in sequence with synchronization
  points. We pay the barrier cost between every stage.
- **Ring buffer state corruption**: Kernel A advances `state.qkv_stage`,
  Kernel B reads `state.attn_stage`. Cross-workgroup atomics required.
  The drop's pseudocode uses `state.qkv_stage` without atomicity
  annotations.
- **Vulkan async compute queues**: True overlap requires submitting
  to independent queues with semaphore synchronization. We
  explicitly rejected this in the wgpu_hal_vulkan_port_scoping
  stash ("Avoid timeline semaphore complexity, async compute queue
  dependence, multi-queue assumptions").
- **wgpu doesn't expose multi-queue submission** in its safe API.
  We'd need wgpu_hal::vulkan AND multi-queue work — both are
  separate large projects.

### Where this might land

If we ever do the multi-queue compute work (post-wgpu_hal port,
post-prefill-batching), the dual-kernel pattern could be the right
architecture for prefill mode where M>1 tokens are batched. For
decode mode (M=1) the pipeline depth doesn't help — you still have
sequential dependency across tokens.

**Decision:** decline as currently scoped. Revisit if/when we have
both wgpu_hal port AND prefill batching landed AND we still see
overlap headroom. ~6 months out, optimistically.

## Speculative decoding pitch (separate stash)

The drop concludes with a pitch for speculative decoding as "the
real 60+ lever." That's the same content as Drop 2 in this batch —
a full speculative decoding architecture. Stashed separately at
`speculative_decoding_v1_design.md`.

Short version: legitimate technique, used by vLLM/TensorRT-LLM/llama.cpp
draft mode, real 2-3× multiplier. Requires multi-model loading
(already queued) + draft model + commit-or-revert KV cache. See
the dedicated stash for analysis.

## Polish notes (for the dual-kernel pipeline if we ever revisit)

### 1. Ring buffer depth must be empirically tuned

Their suggested depths (P100=4, 1070=2-3) are educated guesses.
Real tuning would require the `--bench` mode running with depth
swept across [1, 2, 4, 6, 8] tokens and recording per-depth t/s.
Not worth doing until the basic pipeline works.

### 2. Causal dependency restricts pipeline depth

Token N+2's QKV depends on token N+1's output (autoregressive
decoding). Their pseudocode shows N+2 in the QKV stage while N is
still in FFN — but if N hasn't produced its output yet, N+1 doesn't
have its input embedding, so N+2 can't have run QKV either.

The pipeline only works if we use **draft tokens** for N+1 and
N+2 — which is exactly speculative decoding. The pipeline-overlap
argument and the speculative-decoding argument are the same argument
under different names. The "+pipeline overlap" 12-22 t/s row in
their table likely already assumes some form of token speculation.

This is a key insight the drop doesn't make explicit. Lock into
lessons.

### 3. The acknowledged constraint

Their note:

> "We cannot do true CUDA-style async streams, so we emulate it"

Honest. The "emulation" via "1 dispatch per stage covering MANY
tokens" is exactly the prefill-batching pattern we already have
queued. Same architecture, different framing.

## Where this slots vs queued work

Doesn't add new work for the dual-kernel pipeline — that's deferred
indefinitely. Speculative decoding (Drop 2) is a separate item that
DOES slot in as new work, after multi-model loading lands.

Updated priority order:

1. Multi-model registry (in-progress)
2. `--bench` mode + EngineBenchmarker
3. Q6_K K/V/Q proj+RoPE+cache fusion
4. Attention scratch pool
5. Capability detection + OptimizationProfile
6. KV cache fp16 + 128B prefetch
7. classify_memory_type + ScratchLayout extension
8. Fused SwiGLU FFN kernel
9. Prefill batching mode
10. wgpu_hal::vulkan submit path
11. **Speculative decoding** ← new (after multi-model + bench land)

Slot 11 is where speculative decoding fits. It's a full system, not
a kernel optimization, and requires both multi-model loading
(slot 1) and benchmarking (slot 2) as prerequisites. Once those
land, speculative becomes the highest-leverage remaining item.

## Realistic post-everything ceiling for our hardware

Updated final projection:

| Phase | P100 t/s | Notes |
|-------|----------|-------|
| Today | 2.2 | baseline |
| All kernel + scratch + KV work landed | 12-18 | single-stream pure decode |
| + pipeline overlap (deferred) | 14-22 | ceiling for pure decode |
| + speculative decoding | 25-45 effective | "perceived" t/s |

The 60+ t/s line is reachable only with speculative on a smaller
model (Qwen 0.5B as draft, Qwen 1.5B as main, both on the P100).
Gemma-class with 4B+ effective parameters would need ~70-80 t/s
draft model speeds to even pretend at 60+ t/s effective — which is
itself unreachable.

**Realistic target for cesarops-inference v1**: 25-45 effective t/s
with speculative on small dense models. Beyond that requires either
smaller models, smaller context, or quantization tradeoffs.

---

Filed under research_log because the hard-truth analysis is
important calibration content, the dual-kernel pipeline is deferred,
and speculative decoding gets its own stash.

## Verbatim source — dual-kernel pipeline (NOT for integration)

```wgsl
// Kernel A: QKV producer
@compute @workgroup_size(SUBGROUP_SIZE)
fn qkv_stage() {
    let token = global_id.x + state.qkv_stage;
    let qkv = fused_qkv_rope(token);
    write_qkv_ring(token, qkv);
}

// Kernel B: Attention consumer
@compute @workgroup_size(SUBGROUP_SIZE)
fn attention_stage() {
    let token = global_id.x + state.attn_stage;
    let qkv = read_qkv_ring(token);
    let ctx = attention_fused(qkv);
    write_ctx_ring(token, ctx);
}

// Kernel C: FFN consumer (overlaps attention)
@compute @workgroup_size(SUBGROUP_SIZE)
fn ffn_stage() {
    let token = global_id.x + state.ffn_stage;
    let ctx = read_ctx_ring(token);
    let out = fused_ffn(ctx);
    write_output(token, out);
}
```

Preserved for reference if we revisit dual-kernel pipelining
post-wgpu_hal + post-prefill-batching landing.
