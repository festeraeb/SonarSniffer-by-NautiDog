# Bottleneck Diagnosis @ 2.1 t/s — Cluster Diagnostic

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-17. UNPROMPTED diagnostic — contributor saw our
2.1 t/s number and built a diagnostic framework around it.
Status: **REFERENCE — VALIDATES OUR EXISTING BOTTLENECK ANALYSIS.**
Mostly confirms what we already documented in the bottleneck analysis
chat thread, with sharper framing in places.

## Verdict

This drop reformulates our internal bottleneck analysis as a 4-axis
diagnostic model:

> "(A) matmul efficiency (compute bound)
>  (B) KV cache bandwidth (usually the real limiter)
>  (C) kernel launch / dispatch overhead (Vulkan/wgpu killer)
>  (D) subgroup occupancy (silent killer)
>
>  Your symptom (2.1 t/s) usually = B + C combined"

This matches our internal analysis exactly:
- Tier 1 dispatch storm = their (C)
- Tier 1 pre-dequant of weights to f32 = their (A) inefficiency
- Tier 1 per-head buffer alloc = their (B) memory traffic
- Tier 3 polling instead of fences = their (C)
- Pascal subgroup underfill = their (D) — flagged but not yet
  audited in our codebase

## What the diagnostic asks for (and what we'd answer)

Their diagnostic question list:

> "If you want, I can diagnose this to near-exact root cause in one
>  pass. Send:
>  - GPU model (P100 / 1070 / etc)
>  - tokens/sec broken down: prefill t/s, decode t/s
>  - dispatch count per token step
>  - KV cache layout (even pseudo)
>  - whether you do: fused QKV? (yes/no), fused attention? (yes/no)
>  - buffer strategy: persistent or per-token allocation?"

Our answers (for forwarding back if we choose to engage):

| Question | Answer |
|----------|--------|
| GPU model | Tesla P100 16 GB (T440), GTX 1070 8 GB (cesarops2) |
| Prefill t/s | N/A — no prefill mode yet, prefill currently dispatches as M independent decode passes |
| Decode t/s peak | 2.2 t/s on Qwen 1.5B Q6_K, P100 |
| Dispatch count per token | ~17 per layer × 28 layers = ~476 submits (post-round-6 reductions; was ~500 pre-fusion) |
| KV cache layout | `[pos][n_kv_heads][head_dim]` row-major fp32 (NOT yet promoted to fp16) |
| Fused QKV+RoPE? | NO — separate matvec_bias + rope dispatches per Q/K/V. q6k_kv stash has the fused kernel ready, V kernel integration-ready, K/Q pending |
| Fused attention? | NO — split-by-head per-head dispatch (recently optimized with attention_pc push-constant pipeline but still 12 dispatches per layer) |
| Buffer strategy | Mixed: KV cache persistent ✓, per-head attention buffers PER-TOKEN ALLOC (Tier 1 bottleneck — fixed by attention scratch pool stash), pipeline-cache NOT wired yet |

## Their predicted root cause

> "If I had to bet: You are 1-2 kernel fusions away from a 3×-6× speedup.
>  Not a small tuning issue."

Specifically they predict:
- Case A (most likely): KV cache bandwidth bound (wrong layout or
  too many writes)
- Case B: Too many dispatches per token (CPU overhead dominates GPU)
- Case C: No kernel fusion (ggml-style assumption not implemented)
- Case D: Subgroup underfill on Pascal

Our reality: B + C combined, with D as a contributing factor we
haven't audited. A is partially false — our KV layout is correct
(`[seq-major][head-major]`) but we haven't promoted KV to fp16 yet
and we DO have too many KV writes (separate matvec then RoPE then
`copy_buffer_to_buffer`-into-cache, instead of fused write).

The fused K/V/Q proj+RoPE+cache kernel from the q6k_kv stash kills
the extra KV writes. Pure Case B mitigation. That's queued.

The dispatch-count problem (Case C) is the wgpu_hal port. That's
queued.

So their prediction "1-2 fusions away from 3×-6× speedup" maps to:
1. Land fused K/V/Q proj+RoPE+cache (their Case A and partial B fix)
2. Land FA-lite fused attention (their Case B + C fix)
3. Land wgpu_hal submit path (deeper Case C fix)

After all three: realistic projection 8-12 t/s on P100. Their
prediction is 3×-6× from 2.2 ≈ 6.6-13.2 t/s. Aligned.

## "Fast path vs slow path divergence" — useful framing

Their model:

### Matmul path

Fast: `Q4/Q6 → fused dequant + matmul → single dispatch per block`

Slow: `Q4 decode pass → fp16 buffer → second matmul pass → read back`

We are CURRENTLY slow. `tensor_loader_safe.rs` pre-dequants Q6_K to
fp32 at load time (Tier 1 bottleneck #3). The `matvec_q6k_fused.wgsl`
shader from prior round-6 work would fix this — it's compile-staged
but not dispatched because of the load-time dequant. Two changes:

1. Stop pre-dequanting at load (keep weights as packed Q6_K)
2. Switch matvec dispatch to `matvec_q6k_fused.wgsl`

Our fused K/V/Q kernels from the q6k_kv stash already do this for
the projection layers. The remaining matvec sites (output projection,
FFN up/down/gate, lm_head) need the same treatment. Track.

### KV cache layout

Fast: `[seq-major][head-major][packed fp16]` — continuous, append-only,
no transpose

Slow: `[head-major][seq-major]` OR per-token buffer alloc OR storage
buffer indirection

Our current: `[pos][n_kv_heads][head_dim]` (= [seq-major][head-major])
in fp32, NOT packed fp16. Layout correct, dtype wrong. Promotion to
fp16 is queued under "KV cache fp16" item from the homelab doctrine
stash.

### Dispatch model

Fast: 1 token step = 2-5 dispatches total (heavily fused)

Slow: 1 token step = matmul + bias + rope + kv-write + attention +
softmax + projection = 7+ dispatches per layer

We are slow. ~17 per layer post-round-6. Fused K/V/Q kernel takes
that to ~14. FA-lite kernel takes it to ~9. wgpu_hal takes the
per-submit cost to near-zero. Not "1 token step in 5 dispatches" —
that's monolithic-kernel territory which the homelab doctrine pushed
back on. Realistic target: 5-8 dispatches per LAYER post-FA-lite,
times 28 layers = 140-224 per token. Still 2× better than today.

## "Subgroup underutilization" — flagged for audit

Their concrete pattern:
- BAD: workgroup = 256 threads, only 40-60% active lanes
- GOOD: workgroup tuned so active lanes ≈ 90-100%

We have NOT audited this. Our existing kernels mostly use 256-thread
workgroups. For matvec where output rows < 256, we waste threads.
Polish-note action item:

> Audit existing matvec dispatch sites. For any kernel where `out_dim`
> can be < workgroup_size, restructure to either (a) use a smaller
> workgroup matched to out_dim, or (b) parallelize multiple output
> rows per workgroup to fill threads.

This is a separate optimization pass that pairs naturally with the
capability-detection work — once we have `subgroup_size` from the
profile, workgroup size becomes a derived quantity, not a hardcoded
256.

## Their concrete fix list (highest ROI ordering)

| Fix | Status |
|-----|--------|
| Reduce dispatch count first (target <4 per token step) | impossible target on our architecture without monolithic kernels we explicitly rejected; realistic target <8 per layer |
| Persistent buffers (KV cache + matmul scratch + RoPE tables) | KV cache already persistent ✓, matmul scratch covered by attention scratch pool stash, RoPE tables — track |
| Eliminate hidden barriers (storage buffer sync, compute→compute barriers, map/unmap) | wgpu_hal port |
| Fuse QKV proj + RoPE + initial reshape immediately | fused K/V/Q stash (queued) |

The final "fuse QKV + RoPE" point is exactly our q6k_kv stash plan.
"Often gives 1.5×-3× speedup" — matches their prior Pascal claims
and our prior round-6 fusion gains.

## What's NEW vs our existing analysis

Three pieces I hadn't documented before:

### 1. The "subgroup-locked pipeline" framing

> "We stop thinking in kernels and switch to: fixed subgroup-sized
> execution blocks. NVIDIA: warp = 32 lanes. AMD: wave = 64 lanes.
> Everything is shaped around that."

Useful mental model. Aligns with the workgroup-derivation rule from
the capability detection drop. Worth promoting to a design principle:

> Every dispatch's workgroup size is a multiple of the device's
> subgroup_size. Active-lane utilization is an explicit non-goal of
> "performance" — it's a hard correctness constraint.

### 2. Online softmax in the FA-lite attention kernel

Their attention_kv kernel sketch:
```
for chunk in kv_cache_tiles():
    let logit = dot(q, k)
    let m = max(max_logit, logit)
    let scale = exp(max_logit - m)
    acc *= scale
    sum *= scale
    let p = exp(logit - m)
    acc += p * v
    sum += p
    max_logit = m
```

This is the exact online-softmax pattern from the prefill_batching
stash polish notes (item #2). Confirmation that it's the canonical
form. Worth referencing across both stashes.

### 3. The 3-dispatch-per-token target architecture

> "1. FusedQKV_RoPE → FusedAttention_KV → OutputProjection
>  That's it: 3 dispatches/token max (target: 2)"

This is the **PER-LAYER** target, not per-token. Their prose is
ambiguous. At 3 dispatches per layer × 28 layers = 84 dispatches per
token. That's an aggressive target — currently 476. Achieving it
requires:
- Fused QKV+RoPE+cache write (1 dispatch, kills 4 separate ones)
- FA-lite fused attention (1 dispatch, kills 3 separate ones for QK,
  softmax, AV)
- O-projection + bias fused (1 dispatch, kills 2 separate ones)
- FFN up+gate+SwiGLU+down fused (1 dispatch — currently 4)

Worth an explicit goal in the bottleneck-tracker: **target 4-5
dispatches per layer** by the time all the queued fusion work lands.
Down from current ~17 = 3.5× dispatch reduction per layer = compounds
with the wgpu_hal per-submit cost reduction.

## Where this slots vs queued work

Doesn't add new work — validates and reframes existing queue. The
two-prong route compromise architecture remains the umbrella.

The diagnostic framework itself is worth wiring into `--bench` mode
when that lands: per-token reports of dispatch count, KV cache
bandwidth utilization, subgroup occupancy. All four axes (A/B/C/D)
become measured quantities, not inferred ones.

---

Filed under research_log because the diagnostic framework is useful
mental scaffolding but doesn't add executable work. The "3-dispatch
target per LAYER" framing becomes our north star for the fusion
work queue.
