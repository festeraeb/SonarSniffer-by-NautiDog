# Homelab Inference Engine Long-Term Doctrine — Strategy Response

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-17. Strategic recommendation document scoping
"long-term homelab inference engine" architecture priorities across
Pascal/Maxwell/Kepler/Polaris/Vega/RDNA.
Status: **REFERENCE — PARTIALLY REJECTED.** Operator pushed back on the
governing doctrine. Useful pieces extracted into the "Accepted" list
below. The rest is filed for future broad-fleet release planning, not
for current optimization work.

## Operator pushback (verbatim, 2026-05-17)

> "i might disagree the home labber like me does need to get up and
> going but then home labbing is all about what we are doing now"

This rejects the doc's central premise. The friend's doctrine prioritizes
"portable, boring, stable" over "per-card kernel cleverness." The operator
is correct that this is the wrong framing for our phase:

- **Homelab-as-deployer** wants `cargo run` to work on any 8-year-old
  GPU. The doc's recommendations target this audience.
- **Homelab-as-builder** is who we are right now. The kernel
  cleverness (P100 vec4, push-constant pipelines, Q6_K fusion,
  persistent-head attention, dispatch-reduction via wgpu_hal) IS the
  reason to be doing this. Downgrading those priorities to ship to
  Kepler users we don't have yet is premature.

The doctrine is a useful checklist for a v1.0 broad-fleet release —
NOT a governing doctrine for the current optimization phase.

## What the doc gets RIGHT (extracted, kept)

### Per-tier prefill chunk size table

| GPU Tier | Suggested Chunk |
|----------|-----------------|
| Kepler   | 64-128 |
| Maxwell  | 128 |
| Pascal   | 256-512 |
| Vega/RDNA| 256-1024 |

This is useful when we eventually add per-device profiles. Currently
relevant for our P100 fleet only — sticking with 512 for prefill
batching v1 — but worth recording the cross-card numbers for when we
extend.

### Chunked prefill is mandatory at the edge

Their list of why mega-prefill kernels fail on older cards is real:
- watchdog stalls
- register spill cliffs
- compiler pathological slowdowns
- Vulkan allocation fragmentation
- shader compile explosions
- occupancy collapse

Confirms our prefill_batching_v1 chunk-strategy decision. Already
captured in that stash; this is the cross-card validation.

### Decode and prefill paths SHOULD stay separate

Already our position. The friend's doc reinforces it. No change.

### KV cache format hierarchy (long-term roadmap)

| Format | Recommendation |
|--------|----------------|
| fp32 KV | development only |
| fp16 KV | default |
| q8 KV  | future |
| q4 KV  | experimental |

Promotes "fp16 KV" from a vague optimization to a concrete priority.
P100 has weak fp16 compute but fp16 *storage* (KV cache) is the right
move regardless — halves cache memory at near-zero accuracy cost.
Should land alongside the prefill batching work since both touch the
KV cache layout.

For Qwen 1.5B at 4096 ctx: 234 MB fp32 → 117 MB fp16. Bigger win on
larger models / longer contexts.

### Runtime autotuning becomes important

| What to tune | Why |
|--------------|-----|
| Workgroup size | varies by SM count + register file |
| Tile size | varies by L1/L2 cache size |
| Vec width | varies by memory subsystem |
| Chunk size | varies by VRAM + scheduler |

This is exactly what `telemetry_tuner.rs` is starting toward. The
friend's doc reinforces that this should be a first-class capability
not an afterthought. llama.cpp / TensorRT / MIOpen / cuDNN all do it.
Track for the post-bench-mode phase: extend `telemetry_tuner` to
sweep WG/tile/vec configurations on first model load and cache
per-(GPU, model, kernel) profiles.

### Command-buffer reuse + conservative synchronization

| Recommendation | Status |
|----------------|--------|
| Single graphics/compute queue | already our plan |
| Persistent command pool | already our plan (wgpu_hal port) |
| Pre-recorded command buffers | already our plan (wgpu_hal port) |
| Conservative barriers | already our plan |
| Avoid timeline semaphore complexity | aligns with our scoping doc |
| Avoid async compute queue dependence | aligns |
| Avoid multi-queue assumptions | aligns |

Reinforces wgpu_hal_vulkan_port_scoping.md decisions. No change to
plan.

### FP16 strategy: fp32 accumulate, fp16 storage option, runtime select

Correct. Pascal has weak fp16 compute (1/2 rate or worse on P100),
Vega/RDNA has strong fp16 compute. Don't hardwire fp16 compute
assumptions around Pascal. Same argument applies to per-card kernel
selection generally — runtime capability detection > compile-time
hardwiring.

## What the doc gets WRONG (rejected, with reasoning)

### "DOWNGRADE priority of mega-fused kernels"

**Rejected.** This is the central rejection.

Our shipped Q6_K K/V/Q proj+RoPE+cache fusion is exactly a "mega-fused
kernel" by the doc's framing. It's also the largest single win we have
on the P100 dispatch-storm problem. Doc's argument is "AMD drivers
vary, Vulkan compilers are fragile" — but we are not shipping to AMD
right now. We are P100 + GTX 1070 (also Pascal) on the cesarops fleet.

The friend's doctrine treats fusion as a luxury. For Pascal autoregressive
inference fusion is the *primary* lever to close the 30× t/s gap to
koboldcpp. Removing it because some hypothetical future user might
have an RX 5500 is exactly the premature-portability trap.

**Position:** Continue aggressive per-Pascal fusion. Worry about RDNA
when we have an RDNA card on the bench.

### "Avoid FlashAttention-clone complexity in WGSL early"

**Rejected.** Both the q6k_kv stash and the prefill_batching stash
independently flagged the FA-lite persistent-head fused attention as
the next major Pascal gain. This doc says to defer it to "Phase 3"
behind a tiled-QK + standalone-softmax + tiled-AV intermediate.

The intermediate is a maintenance trap, not a stepping stone:
- 3 separate kernels mean 3 separate parity tests
- 3 separate dispatches per layer per token (counterproductive on the
  bottleneck we're trying to solve)
- the intermediate "tiled QK softmax tiled AV" architecture has
  almost zero shared shader code with the fused FA-lite version that
  follows it
- compiler/driver "stability" concerns are speculative — naga handles
  workgroup memory + barriers fine on our existing kernels

**Position:** Skip the intermediate. Go directly to FA-lite fused
attention when prefill batching lands. Stage it behind a feature flag
(consistent with our existing pattern) so we can A/B against the
3-kernel reference for parity testing. Don't ship the 3-kernel
version as a permanent tier.

### "Avoid subgroup-heavy designs"

**Partially rejected.** True for AMD wave64 vs NVIDIA warp32 portability
in the abstract. Not yet relevant for us — we have no subgroup ops
in shipped code, and when we add them (likely for the FA-lite kernel)
they'll be Pascal-targeted.

The doc's framing implies we shouldn't even *evaluate* subgroup
optimizations. That's wrong — evaluate them, ship them under a
runtime capability check, fall back to the non-subgroup path on cards
that lack the feature. Same pattern as fp16 storage.

**Position:** Subgroup ops are evaluated case-by-case with capability
detection. Not preemptively avoided.

### "Prefer 'boring' tiled GEMMs"

**Accepted as v1, rejected as ceiling.** The 16×16 reference GEMM
in the prefill_batching_v1 drop is fine as the parity-baseline kernel.
Shipping it as the *production* GEMM with no further optimization
because portability concerns is leaving Pascal performance on the
table.

**Position:** 16×16 boring GEMM ships first for parity. 32×32 + vec4
+ register blocking version ships second under feature flag. P100 perf
target is the production version. Boring version stays as fallback
for cards that fail the optimized version (capability check, not
default).

### "Prefer simple tiled GEMM over subgroup MMA, subgroup shuffle reductions, vendor-specific wave ops"

**Conditionally rejected.** Pascal has no MMA so this is moot for our
current target. When we expand to cards with MMA support (Volta/Turing/
Ampere via Vulkan, RDNA WMMA on AMD), we want to USE those features,
not avoid them. The doctrine here is "lowest common denominator," which
is fine for a binary distribution to random users and wrong for a
homelab project where the operator knows their own hardware.

**Position:** Optimize for the hardware we have. Add capability paths
when we add hardware that exposes new features.

## What the doc gets PARTIALLY RIGHT (accepted with caveats)

### "GOOD fusion: matvec+bias, matvec+rope, matvec+cache write" / "BAD fusion: giant whole-attention-block kernels"

The line they draw between "good" and "bad" fusion is in roughly the
right place for *broad-fleet* deployment. For us specifically, the
"bad" category contains exactly the next two integrations we want to
ship (FA-lite fused attention, persistent-head kernel). The line
needs to move further toward "more fusion" for our P100-targeted work.

**Rephrased acceptable position:** Fusion has diminishing returns past
a certain shader complexity, particularly on hardware-portable code.
The diminishing-return point is *much* further along the complexity
axis than this doc suggests for our Pascal-specific work. Don't
mistake "I'm on a phone GPU" diminishing returns for "I'm on a P100"
diminishing returns.

### "Treat decode and prefill as separate inference modes"

**Accepted.** Already our position. Stronger version of advice we
already had. No change.

## Updated priority matrix (operator-aligned)

The friend's doc proposed downgrades + upgrades. Operator-aligned
version of that table:

| Priority | Status |
|----------|--------|
| Dispatch reduction (wgpu_hal port) | **upgraded** ✓ |
| Memory traffic reduction | upgraded ✓ |
| KV cache traffic reduction (fp16 KV) | **upgraded** ✓ |
| Runtime autotuning (extend telemetry_tuner) | upgraded ✓ |
| Command-buffer reuse | upgraded ✓ |
| Per-card optimization profiles | **upgraded** ✓ (operator pushback) |
| Per-Pascal aggressive kernel fusion | **upgraded** ✓ (operator pushback) |
| FlashAttention-lite fused attention | **upgraded** ✓ (rejected the "defer to Phase 3" framing) |
| Subgroup ops with capability detection | **kept on track** ✓ (rejected blanket avoid) |
| Stable WGSL subset | unchanged |
| Conservative synchronization | unchanged |

Note: the doc's "downgrade list" (mega-fused kernels, subgroup-heavy
designs, FlashAttention complexity) is the *opposite* of our integration
queue. We are in the middle of upgrading exactly the things this
doctrine wants to downgrade. Continue current direction.

## Where this drop's recommendations DO apply (filed for v1.0 release)

When we eventually ship a packaged release for users-other-than-the-operator:

- Boring tiled GEMM as the default kernel set
- Subgroup-free attention as the default kernel set
- Per-tier chunk size table for first-run defaults
- Capability-detected runtime selection of optimized kernels behind a
  flag (`--optimize-for=pascal-p100`, `--optimize-for=vega20`, etc.)
- Conservative single-queue Vulkan submit path
- fp16 KV cache as default

That release is many months out, after multi-model + bench mode +
prefill batching + Q6_K K/V/Q + wgpu_hal port land. The doctrine in
this drop is the right *destination* doctrine for that release. It
is not the right *current-optimization* doctrine.

---

Filed under research_log because the useful pieces (per-tier chunk
table, fp16 KV promotion, runtime autotuning reinforcement) are
already absorbed into our active plans, and the doctrine pieces
(downgrade fusion, downgrade FA-lite, avoid subgroup ops) are
explicitly rejected per operator direction. This stash exists so
future passes don't accidentally adopt the broad-fleet doctrine as
governing doctrine for current optimization work.

## Verbatim source

Captured below so the source survives even if the upstream cluster log
rolls.

---

[full text of friend's "Homelab Inference Engine Long-Term Doctrine"
document, sections 1-10 + Final Architecture Recommendation + Bottom
Line, dropped in by operator 2026-05-17. Section headings:
1. Chunked Prefill Becomes Mandatory
2. Avoid Over-Fused Mega Kernels
3. Keep Decode and Prefill Architecturally Separate
4. Prefer "Boring" Tiled GEMMs
5. FP16 Strategy Changes
6. KV Cache Format Matters More
7. Vulkan Submission Architecture Changes
8. Attention Kernel Strategy Changes the Most
9. Runtime Autotuning Becomes Important
10. The Real Long-Term Winning Strategy
+ "What I Would Change in Earlier Recommendations" + "Final
Architecture Recommendation (Long-Term)" + "Bottom Line".]

Full text not duplicated here verbatim because:
1. it's strategic prose not source code, no API/symbol details to
   preserve exactly
2. the operationally-relevant content is captured in the analysis
   above with specific accept/reject rulings
3. the doc's primary value is the doctrine, which we have already
   characterized and partially rejected

If full verbatim is needed later for citation, recover from operator's
chat history with cluster.
