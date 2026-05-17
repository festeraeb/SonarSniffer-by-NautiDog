# Pascal Fusion Rewrite Architecture — Drop-in Replacement

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-17. UNPROMPTED follow-up to the bottleneck diagnostic
drop. Tuned specifically for P100 (sm_60) + GTX 1070 (sm_61) +
Vulkan subgroup execution.
Status: **REFERENCE — ARCHITECTURE OVERLAPS HEAVILY with already-stashed
designs.** Useful as a synthesis document; most of the concrete
content is already captured in q6k_kv_proj_rope_cache_fused.md and
prefill_batching_v1_design.md and attention_scratch_pool_v1_design.md.
Awaiting the contributor's promised follow-up on KV cache read
amplification (item #11 in their writeup) before triggering integration.

## Verdict

This drop synthesizes the previous three drops (capability detection,
bottleneck diagnosis, attention scratch pool) into a unified target
architecture. It's:

- **Correct** in its high-level "3 fused kernels per layer" target
- **Aligned** with our existing queued work
- **Mostly redundant** with content already stashed in the other 3 drops
- **Useful** as the integration-time blueprint to read alongside the
  per-component stashes

The kernel sketches it provides for QKV+RoPE and attention+KV are
shorter than the q6k_kv stash and prefill_batching stash respectively,
and skip details those stashes already covered (Q6_K dequant, online
softmax, GQA indexing, online-softmax max-subtraction). Treat this
as the index document, not the source of truth for individual kernels.

## What's NEW or CRYSTALLIZED here

### 1. The "subgroup-locked pipeline" doctrine (formalized)

> "We stop thinking in kernels and switch to: fixed subgroup-sized
> execution blocks. NVIDIA: warp = 32 lanes. AMD: wave = 64 lanes.
> Everything is shaped around that."

Same sentence as in the bottleneck diagnostic drop. Worth promoting
to design-principle status — every kernel post-fusion ships with a
`workgroup_size = N * subgroup_size` constraint enforced by capability
detection.

### 2. The 3-kernel-per-layer target

```
FusedQKV_RoPE → FusedAttention_KV → OutputProjection
```

Plus FFN. So realistically 4-5 dispatches per layer, not 3. The
contributor's prose is loose on whether "3 per token step" means per
layer or per token. Per layer is the achievable target; per token
would require persistent kernels we already rejected.

### 3. The kernel fusion contract

> "Each kernel MUST:
>  - operate on subgroup-sized tiles
>  - never write intermediate global buffers
>  - pass registers → shared memory only
>  - avoid dispatch chains inside kernel"

Worth lifting into the lessons file as the canonical fusion contract.
"Never write intermediate global buffers" specifically rules out
patterns we currently use (the K-buffer + V-buffer + copy_buffer_to_buffer
into KV cache). Already targeted by the q6k_kv stash.

### 4. The expected performance shift table

| State | tokens/sec |
|-------|------------|
| Current (typical wgpu pipeline) | 2-3 t/s |
| After fusion only | 4-7 t/s |
| After KV layout fix + fusion | 6-12 t/s |

Our current state matches "current typical wgpu pipeline" exactly
(2.2 t/s). Their predicted post-fusion target of 6-12 t/s assuming
KV layout fix matches our internal projection of "8-12 t/s after
queued work lands."

The remaining 30× gap to koboldcpp 60+ t/s would require:
- All queued work landed
- wgpu_hal port for dispatch overhead
- KV cache read amplification fix (their pending item #11)
- Probably persistent-head FA kernel beyond just FA-lite

So 8-12 t/s is the realistic short-term target. 30+ t/s is the
medium-term target after wgpu_hal. Reaching koboldcpp parity is the
long-term target.

## What's REDUNDANT with existing stashes

### QKV+RoPE kernel sketch

Their sketch is a 5-step pseudo-WGSL:
```
1. Load input embedding (coalesced)
2. Fused QKV projection (no intermediate buffers)
3. RoPE applied immediately to Q and K
4. Write directly to KV cache (no staging buffer)
5. Output Q forwarded via subgroup shuffle
```

The q6k_kv stash already has the FULL Q6_K-aware version of this
with proper sign-extension, scale handling, partner-row computation
for RoPE rotation, and the GQA-aware KV cache stride. This drop's
version is the "design intent"; the q6k_kv stash is the "actual
shader source." Use the q6k_kv stash for integration.

### Attention+KV kernel sketch (online softmax)

Their attention_kv sketch IS the FA-lite kernel design we've been
queueing, but it's missing:
- GQA head indexing (kv_head = q_head / (n_heads / n_kv_heads))
- Causal masking handling at tile boundaries
- Workgroup memory tiling for K/V (needed for actual FA-lite,
  otherwise it's still globally-accessing)
- Q tile size + K/V tile size definitions

The prefill_batching_v1 stash covers this in much more detail with
the polish notes flagging the score-recompute trap, naive softmax,
GQA, and workgroup parallelism issues. Use that stash for the actual
attention kernel design.

### Workgroup sizing rule

Same rule as the capability detection part 2 drop: subgroup=32 →
workgroup=128, subgroup=64 → workgroup=256. Use the capability
detection stash for the wired version.

### KV cache layout requirement

Same as the bottleneck diagnostic drop: `[token-major][head-major][packed fp16]`.
Our layout already matches modulo the fp16 packing.

## What's MISSING or AMBIGUOUS

### 1. No Q6_K dequant logic in the QKV+RoPE sketch

Their sketch is dtype-agnostic. For our shipping path (Qwen 1.5B Q6_K)
the kernel needs the Q6_K dequant inline. The q6k_kv stash has this
correctly. This drop assumes f16 storage of weights, which we don't
have yet (weights are dequanted to fp32 at load time per Tier 1
bottleneck #3). The proper fix is keep weights packed as Q6_K and
dequant inline — q6k_kv stash already does this.

### 2. No FFN fusion

Their architecture stops at the attention layer. FFN (up + gate +
SwiGLU + down) is currently 4 dispatches in our pipeline. A fused
FFN kernel would cut that to 1, hitting the "3-5 dispatches per
layer" target.

The drop does not provide an FFN fusion design. Track as a follow-up
prompt to the cluster: "Pascal-tuned fused FFN kernel design."

### 3. No bias-fusion strategy for the output projection

Their out_proj kernel sketch:
```
let x = load_attention_output();
let y = matmul_o(x);
write_final(y);
```

No bias add. Our existing matvec_bias path handles this. For the
fused version we want matmul_o to include the bias add inline (which
matches the bias-fusion work shipped in round 6).

### 4. Pending item #11: KV cache read amplification

> "If you want the next 2× jump: The next bottleneck after this is
> KV cache read amplification (L2 miss pattern).
> I can give you a second-stage rewrite that:
>   - prefetches KV in subgroup windows
>   - aligns cache lines to 128B segments
>   - eliminates strided reads entirely
> That's usually where another +40-80% gain appears on Pascal."

This is the next drop they're waiting to send. Target: another
+40-80% gain on Pascal AFTER the fusion work lands. Stack:

- Current 2.2 t/s
- After fusion (queued): 6-12 t/s
- After KV cache read amplification fix: +40-80% → 8.4-21.6 t/s

Worth greenlighting their item #11 — closes another bottleneck axis
(B in their diagnostic framework) without conflicting with anything
queued.

## Polish notes for integration

### 1. The "3 dispatches per token step" claim is misleading

It's 3 dispatches per LAYER. With 28 layers that's 84 dispatches per
token, not 3. The pseudocode `run_token_pipeline` they show:

```rust
fn run_token_pipeline(token: Token) {
    qkv_rope_kernel.dispatch(token);
    attention_kv_kernel.dispatch(token);
    output_kernel.dispatch(token);
}
```

is misleading because it shows three top-level dispatches but each
kernel dispatch internally covers all 28 layers? That's not how wgpu
dispatch works — each dispatch covers a single shader invocation
grid. Either they're describing a meta-loop (one dispatch per layer
× 3 stages), or they're proposing a monolithic "all layers in one
dispatch" kernel which we already rejected as a homelab-portability
concern.

Charitable read: it's the per-layer count, prose is loose. Use as
blueprint for per-layer fusion, not as a literal "3 dispatches per
token" goal.

### 2. The expected-perf table is optimistic

Their table predicts 6-12 t/s after KV layout fix + fusion. Our
realistic projection accounting for the work we've already done:

| Phase | tokens/sec |
|-------|------------|
| Current baseline | 2.2 t/s |
| + fused K/V/Q (q6k_kv stash) | 3.0-4.0 t/s |
| + fused attention (FA-lite) | 5.0-7.0 t/s |
| + KV cache fp16 promotion | 6.0-9.0 t/s |
| + KV cache read amplification fix (pending) | 8.0-14.0 t/s |
| + wgpu_hal submit path | 12.0-25.0 t/s |
| + persistent-head fused attention | 18.0-35.0 t/s |

These are spread out further than their compressed "after fusion =
4-7, after layout = 6-12" grouping. Our realistic projection assumes
each gain stacks somewhat sub-linearly because each one removes part
of the bottleneck the next one was assuming.

The 30× gap to koboldcpp 60+ t/s closes substantially through this
chain — 25-35 t/s is in striking distance once everything lands.

### 3. The "pipeline collapse" `run_token_pipeline` should be the post-fusion shape

Once all queued work lands, the per-layer loop should look something
like:

```rust
for layer in 0..N_LAYERS {
    rmsnorm.dispatch();           // 1
    fused_qkv_rope_cache.dispatch(); // 2: replaces matvec×3 + bias×3 + rope + cache_write
    fused_attention.dispatch();   // 3: FA-lite, replaces split per-head
    out_proj_bias.dispatch();     // 4: matvec + bias fused
    rmsnorm.dispatch();           // 5
    fused_ffn.dispatch();         // 6: up + gate + SwiGLU + down
}
```

6 dispatches per layer × 28 layers = 168 dispatches per token. Down
from current ~476. Then wgpu_hal makes each submit nearly free.

### 4. The "what I need from you to pinpoint it precisely" question list

Their question list is documented in the bottleneck_diagnosis stash.
If we engage further with the contributor we have those answers ready.
We probably DON'T need to engage on the diagnostic — our internal
analysis is already correct and matches theirs. The next ask is
greenlighting their item #11 (KV cache read amplification rewrite).

## Where this slots vs queued work

Doesn't add new work. Confirms the ordering and target architecture.

## Next ask to the cluster

**Greenlight their item #11** — KV cache read amplification rewrite.
Promised content:
- KV prefetch in subgroup windows
- 128B cache-line alignment for L2 hit pattern
- Elimination of strided reads

Expected gain: +40-80% on Pascal after fusion lands. Stacks with
queued work cleanly. Target: drop the next stash + integration plan.

The greenlight prompt is short:
```
Send the second-stage rewrite for KV cache read amplification:
- prefetch model
- 128B cache-line alignment
- strided read elimination
- explicit Pascal sm_60 / sm_61 tuning
- WGSL/Vulkan kernel sketch with shared memory layout
- fallback path for non-Pascal cards (the two-prong-route compromise)
- expected performance characteristics on P100 vs 1070 specifically
```

---

Filed under research_log as a synthesis document. Individual kernel
designs live in the per-component stashes (q6k_kv,
prefill_batching_v1, attention_scratch_pool_v1). This drop's value
is the unified architectural blueprint and the explicit perf target
ladder.
