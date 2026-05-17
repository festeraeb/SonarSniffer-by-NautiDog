# Q6_K Fused Matvec v2 — Pascal-Tuned (friend's wgpu/Vulkan/SPIR-V agent cluster)

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-16. Response to Prompt 2 from our outbound research
asks ("Pascal-tuned Q6_K fused matvec v2").
Status: **REFERENCE — NEEDS ONE STRUCTURAL FIX before integration. Holds
the same `tensor_loader_safe` deferred-dequant blocker as v1 (see
integration order below).**

## What's in the drop

A new shader (`matvec_q6k_fused_v2.wgsl`) and README claiming a
~2-3× speedup vs a naive scalar fused implementation. The drop also
includes a `#![no_std]`-compatible Rust unit test that mirrors
llama.cpp's `dequantize_row_q6_K` layout — that test is sound and
matches our `dequant_probe.rs::cpu_dequant_q6_k` byte-for-byte.

## Genuine improvements over our existing v1 (`shaders/matvec_q6k_fused.wgsl`)

| Feature | v1 (compile-staged in our repo) | v2 (this drop) |
|---|---|---|
| Workgroup size | 256 (one thread/row) | 128 (intended: one wg/row, 128-thread cooperation) |
| Inner accumulation | 4 scalar `acc += d*s*q*x` lines | `dot(vec4<f32>, vec4<f32>)` |
| fp16 decode | Branchful (subnormal + Inf paths) | Branchless normalized-only path |
| Lane parallelism | None (one thread does all 32 inner positions) | 32 lanes each own one `l` (when fixed) |
| Pascal occupancy reasoning | Implicit | Explicit: 4 warps/wg, register pressure budget |

If the structural bug (below) is fixed, the v2 design is meaningfully
better on Pascal because v1's one-thread-per-row scalar path leaves
~127 lanes idle inside each workgroup. v2 corrected would have all 128
threads of each WG collaborating on a single row's dot product, which
is the right pattern for a memory-bound matvec.

## Critical bug — row-vs-lane parallelism mismatch

The kernel as written is internally inconsistent. **DO NOT INTEGRATE
AS-IS.**

```wgsl
@compute @workgroup_size(128, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, ...) {
    let row = gid.x;            // <-- per-thread row, 0..N-1
    if (row >= params.N) { return; }
    let lane = lid.x & 31u;
    var acc: f32 = 0.0;
    for (kb ...) {
        // lane-stride inner loop — each thread does only l=lane (one iter)
        var l: u32 = lane;
        loop {
            if (l >= 32u) break;
            ... acc += dot(qv, xv);
            l = l + 32u;
        }
    }
    // Then a 128-thread shared-memory reduction:
    partials[lid.x] = acc;
    workgroupBarrier();
    var stride = 64u;
    loop { ...; partials[lid.x] += partials[lid.x + stride]; ... }
    if (lid.x == 0u) { output[row] = partials[0]; }
}
```

What this actually computes:
- Thread `lid.x = i` of workgroup `wid.x = w` has `gid.x = w*128 + i`,
  so `row = w*128 + i`. **Each of the 128 threads in a workgroup
  operates on a different row.**
- Each thread's inner loop runs exactly once (`l = lane`, `lane <
  32`, then `l += 32` exits). So each thread covers only 1/32 of
  the K dimension for its row.
- The final shared-memory reduction sums `partials[0..128]` — values
  from 128 *different rows* — and writes that cross-row total to
  `output[row=w*128+0]`. The other 127 outputs are never written.

In short: **wrong outputs for 127/128 rows, partial answers for the
1/128 it does write to.**

## Intended design (almost certainly)

The README's framing ("4 warps/workgroup, good occupancy, enough
parallel rows") and the lane-stride pattern only make sense if
**one workgroup serves one row** with 128 threads collaborating
across the K dimension. The fix:

```wgsl
@compute @workgroup_size(128, 1, 1)
fn main(@builtin(local_invocation_id) lid: vec3<u32>,
        @builtin(workgroup_id) wid: vec3<u32>) {
    let row = wid.x;                  // <-- one wg per row
    if (row >= params.N) { return; }

    let tid = lid.x;                  // 0..127
    var acc: f32 = 0.0;

    for (var kb: u32 = 0u; kb < params.K_blocks; kb++) {
        let block_byte_base = row * params.K_blocks * BLOCK_BYTES + kb * BLOCK_BYTES;
        let input_base = kb * QK_K;

        // Cover both halves and all 32 inner positions across 128 threads.
        // 2 halves * 32 inner_l = 64 unique (half, l) pairs.
        // Distribute 64 pairs across 128 threads -> each pair owned by 2 threads,
        // OR cover only 64 of the 128 threads. Cleaner: have threads 0..63 own
        // (half=0, l=0..31) and (half=1, l=0..31) each, with stride.
        // Pascal-friendly: tid in 0..63 active, 64..127 idle this iteration,
        // then second iteration covers other half. Or more efficient:
        //
        //   half = tid >> 5;        // tid 0..31 -> half 0, 32..63 -> half 1
        //   l    = tid & 31;        // 0..31 within the half
        //   tid 64..127 idle (or extended to also accumulate vec4 chunks).
        //
        // Simplest correct version: only first 64 threads do work; threads
        // 64..127 contribute zero. The 128-wide reduction still works.

        if (tid < 64u) {
            let half = tid >> 5u;
            let l    = tid & 31u;
            let qv   = decode_q6(block_byte_base, half, l);
            let base = input_base + half * 128u;
            let xv = vec4<f32>(
                input[base + l],
                input[base + l + 32u],
                input[base + l + 64u],
                input[base + l + 96u],
            );
            acc += dot(qv, xv);
        }
    }

    var<workgroup> partials: array<f32, 128>;
    partials[tid] = acc;
    workgroupBarrier();
    var stride = 64u;
    loop {
        if (stride == 0u) { break; }
        if (tid < stride) { partials[tid] += partials[tid + stride]; }
        workgroupBarrier();
        stride = stride >> 1u;
    }
    if (tid == 0u) { output[row] = partials[0]; }
}
```

Key changes:
- `row = wid.x` (one workgroup per output row), not `gid.x`
- Threads 0..63 each own one `(half, l)` pair; threads 64..127 sit
  idle but contribute 0 to the reduction (cheap)
- Inner loop deleted — the work is now spread across the workgroup
  rather than across iterations of a single thread
- Shared-mem reduction is unchanged but now correctly sums partial
  dot-products for the *same* row

Dispatch math: launch `ceil(N)` workgroups (was `ceil(N / 128)`).

A more aggressive version uses all 128 threads by having threads
64..127 cover an additional `kb` stripe; that's a v2.1 optimization.

## Smaller issues to clean up

1. **Dead arg**: `decode_q6(block_byte_base, half, l, lane)` takes
   `lane` but never reads it. Drop the parameter.
2. **fp16 zero**: branchless decoder returns `~3.05e-5` for input
   `h=0` instead of `0.0` because `f_exp = (0 + 112) << 23` is
   `2^-15`. Harmless because Q6_K block scale `d` is never zero
   in real models, but flag it. If we ever quantize a model with
   a bias-of-zero somewhere this would surface. Cheap fix:
   `if (h == 0u) return 0.0;` — one branch, end of decoder.
3. **Style**: `bitcast<i32>((v << 24u)) >> 24` works in current
   naga but canonical WGSL is `bitcast<i32>(v << 24u) >> 24i`
   (typed literal `24i`). Cosmetic.
4. **Push constant struct** is identical to v1 — `Params { N, K,
   K_blocks, _pad }`. Reuse v1's `Q6KMatvecPipeline` plumbing
   when we eventually wire either version.

## What to keep verbatim

- The branchless fp16 normalized-path decoder (with the zero-guard
  added back) — cleaner and faster than v1's branchful version.
- The `vec4<f32>` dot for the inner accumulation — matches the
  pattern we already use in `matvec_vec4_pc.wgsl` and is the
  proven Pascal idiom.
- The `workgroup_size(128)` choice and its rationale (4 warps,
  register-pressure budget). Matches our experience tuning
  `matvec_vec4_pc`.
- The Rust unit test — ours and theirs are independent
  implementations of the same llama.cpp reference, so cross-check
  is good.

## Rust unit test cross-check

Their `dequant_q6k` in the test file matches our
`dequant_probe.rs::cpu_dequant_q6_k` element-for-element:

| Element | Theirs | Ours |
|---|---|---|
| `out[base + l]` | `vals[0] * s[0] * d` (slot 0) | `d * sc0 * q1 as f32` (slot 0) |
| `out[base + l + 32]` | `vals[1] * s[1] * d` (slot 1) | `d * sc1 * q2 as f32` (slot 1) |
| `out[base + l + 64]` | `vals[2] * s[2] * d` (slot 2) | `d * sc2 * q3 as f32` (slot 2) |
| `out[base + l + 96]` | `vals[3] * s[3] * d` (slot 3) | `d * sc3 * q4 as f32` (slot 3) |
| Scale interleave | `is, is+2, is+4, is+6` | same |
| ql/qh slot mapping | `ql_a low/high` × `qh shift 0/4`, `ql_b low/high` × `qh shift 2/6` | same |

The slot ordering in the assembled value is slightly different
(theirs: ql_a-low then ql_a-high; ours: ql_a-low then ql_b-low),
but the **output positions** (`base + l`, `+32`, `+64`, `+96`) and
the **values written to those positions** are identical because
both implementations use matching `s[0..3]` ordering for those
positions. So the two reference implementations agree on the
final 256 floats — they just label the intermediate `q0..q3` in
different orders.

We can drop their unit test in alongside ours when we land the
fixed shader.

## Integration blocker (same as v1)

Our `tensor_loader_safe` currently dequants Q6_K to f32 at load
time and uploads f32 to GPU. **A fused matvec is useless if the
input is already dequanted f32.** To realize any gain from v1 OR
v2, we need:

1. `tensor_loader_safe` to keep Q6_K bytes packed on GPU (210
   bytes per super-block, total ~1.2 GB for Qwen 1.5B Q6_K vs
   ~6 GB pre-dequanted f32).
2. The forward pass to dispatch the fused kernel against the
   packed bytes for projection ops (Q, K, V, O, gate, up, down,
   lm_head).
3. A fallback path for non-Q6_K tensors (Q4_K_M, IQ4_XS, F16) —
   keep the current dequant→matvec path for those.

This is a tensor_loader rewrite of comparable scope to the
multi-model spec. Sequence: multi-model lands → benchmarking
lands → tensor_loader_safe rewrite → wire either Q6_K matvec
variant.

## Integration order (when we're ready)

1. **Fix the row-vs-lane bug** (above) and call the fixed shader
   `matvec_q6k_fused_v2.wgsl`. Keep v1 in repo as a documented
   fallback in case Pascal exposes a regression.
2. Wire `Q6KMatvecPipeline` in `pipeline_init.rs` (slot already
   reserved during round 6 — pipeline compile is staged but
   never invoked).
3. Land the `tensor_loader_safe` packed-on-GPU rewrite.
4. Dispatch the fused kernel from `forward_pass.rs` for Q6_K
   projection ops; gate the path behind a feature flag for the
   first few tokens to allow A/B against current dequant→matvec.
5. Add Q6_K-specific unit test (theirs verbatim, alongside ours).
6. T440 P100 + cesarops2 GTX 1070 regression must both stay green
   AND show measurable speedup before flipping the flag from
   opt-in to default.

## Relationship to the wgpu_hal scoping work

The Vulkan fastpath (Prompt 6 response, stashed separately) is
about CPU-side submit overhead — collapsing ~500 submits/token to
1. The Q6_K fused matvec is about GPU-side memory bandwidth —
keeping weights packed at 6 bits/element instead of expanding to
32 bits before the matvec. **They're orthogonal wins** and stack:

- Fused Q6_K matvec: ~1.5-2× steady-state on per-token work
- Vulkan fastpath: ~10-30% on CPU submit overhead

Together with prefill batching (Prompt 4, still out) they're the
three biggest single contributors to closing the 30× gap to
koboldcpp.

---

Filed under research_log because the kernel needs the structural
fix described above before we can ship it, and the
tensor_loader_safe rewrite is the actual integration prerequisite.
