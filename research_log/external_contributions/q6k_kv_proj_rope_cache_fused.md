# Fused Q6_K K/V projection + RoPE + KV cache write — Response

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-16. Response to Prompt 3 from our outbound research
asks.
Status: **REFERENCE — INTEGRATION-READY shaders + dispatch helper.**

## What was delivered

Three WGSL shaders + Rust dispatch helper:
- `matvec_q6k_kproj_rope_cache.wgsl` — Q6_K dequant + matmul + bias + RoPE + direct KV cache write
- `matvec_q6k_vproj_cache.wgsl` — same minus RoPE (V doesn't rotate)
- `matvec_q6k_qproj_rope.wgsl` — Q with RoPE, writes to regular output buffer (not cache)
- Rust `dispatch_kproj_rope_cache()` helper

What this collapses (per layer per token):
- 2 dispatches (matvec, rope) -> 1 dispatch
- 1 explicit `copy_buffer_to_buffer` (k_buf -> kv_cache) -> direct write
- 2 storage-buffer synchronization points -> 0
- 1 temporary k_buf / v_buf allocation -> 0

That eliminates ~3 of our ~17 submits per layer per token, plus the
allocator pressure on k_buf / v_buf scratch.

## Critical observations (theirs, called out themselves)

> "The current K shader computes `decode_q6_dot(row) + decode_q6_dot(partner_row)`
> which **doubles matmul cost**. For production: you should NOT do this."

Their first-pass K shader recomputes the dequant for the partner dim
(used in RoPE rotation). Naive but correct. Their production rec:

> "one thread = rotary pair, avoid recompute, vec4 input loads,
> 128-thread WG, pairwise rope, shared trig reuse"

Expected gain from the production version: **1.7-1.9×** over the
naive paired recomputation they shipped.

For our integration, we ship the naive version FIRST (it's already
correct and a net improvement over the existing 3-dispatch path) then
optimize to the rotary-pair version once we have parity numbers.

## Push constant layout (48 bytes — fits our 64-byte limit)

```
struct Params {
    hidden_dim: u32,
    out_dim: u32,
    n_kv_heads: u32,
    head_dim: u32,
    pos: u32,
    kv_stride: u32,
    k_blocks: u32,
    flags: u32,        // bit 0 = bias enabled, bit 1 = interleaved rope
    theta: f32,
    rope_scale: f32,
    _pad0: vec2<u32>,
};
```

Flags pattern is what we already use elsewhere (`FLAG_BIAS`,
`FLAG_INTERLEAVED`). Both RoPE conventions supported via flag — no
shader switch needed.

## Bind group layout (4 bindings — minus the params uniform we used to need)

```
@group(0) @binding(0) var<storage, read>       input: array<f32>;
@group(0) @binding(1) var<storage, read>       q6k:   array<u32>;
@group(0) @binding(2) var<storage, read>       bias:  array<f32>;
@group(0) @binding(3) var<storage, read_write> kv_cache: array<f32>;
```

For no-bias models (Llama, Gemma): bind a 4-byte dummy buffer + zero
the FLAG_BIAS bit. Their note explicitly says this is faster than
optional bindgroups — no validation overhead, single pipeline, one
push-constant flag controls behavior. Confirmed correct pattern for
our needs.

## Polish notes for integration

1. **`enable chromium_experimental_push_constant;` directive** — naga
   may want a different prefix. We've been using `var<push_constant>`
   without the enable line and it works on our tree's wgpu version.
   Test both; the enable line is a no-op if naga ignores it.

2. **`load_i8` uses `bitcast<i32>(v << 24u) >> 24`** — this is the
   correct sign-extension trick. Worth noting: our existing
   `dequant_q6k.wgsl` uses `select(0, 256, b >= 128u)` for the same
   purpose. Both work; theirs is one fewer instruction.

3. **Q6_K decode** — I cross-checked against our shipped
   `dequant_q6k.wgsl` and `matvec_q6k_fused.wgsl` (both polished from
   prior fleet attempts). The slot logic matches:
   - half 0/1 with ql_base, qh_base, scales_base offsets
   - inner loop l = 0..32
   - 4 outputs at l, l+32, l+64, l+96
   - scale interleave at is, is+2, is+4, is+6 within the half
   - signed scale × global d × (q6 - 32)

   They got it right. This means we can drop `matvec_q6k_fused.wgsl`
   compile-staged work and use these instead, since they're a strict
   superset (matvec + bias + cache-write).

4. **Workgroup size 128** — matches their guidance that 128 is
   best occupancy for Pascal's 60 SMs. Our existing kernels mostly
   use 256; will need to measure. Their rationale (in the bonus
   "Better Pascal Mapping" section) is sound.

5. **`partner_row` computation in K shader** — the naive path. To
   convert to the production "one thread = rotary pair" version,
   each thread computes 2 output dims (dim and dim+half_dim
   simultaneously) by accumulating the dot product ONCE and rotating
   both at the end. ~150 LOC change to the shader, halves the matmul
   work. Definitely a follow-up after parity is verified.

6. **`apply_rope` uses `pow(theta, ...)`** — Pascal's SFU is OK but
   transcendentals stack up. The "shared trig reuse" optimization
   means computing sin/cos once per (pos, head_dim/2) and reusing
   across both K and Q rows — is a workgroup-shared-memory trick
   that's worth ~5-10% on Pascal. Track for the production pass.

7. **K and V shapes differ from Q.** Q output dim = n_heads * head_dim.
   K and V output dims = n_kv_heads * head_dim. Their helper signature
   uses `out_dim = n_kv_heads * head_dim` for K/V. Q dispatch needs
   `out_dim = n_heads * head_dim`. Three pipelines because of this
   shape difference, not three because the shaders fundamentally
   differ.

8. **`kv_stride` is the linear stride per token in the cache.** They
   set it to `out_dim` (= n_kv_heads * head_dim). Matches our
   existing KV cache layout `[pos][n_kv_heads][head_dim]`.

9. **`apply_rope` — the partner computation is wrong direction for
   half-split.** Standard half-split RoPE rotates pairs (dim, dim+half).
   Their formula uses the partner as the second component, but the
   full 2D rotation also needs the partner's contribution to compute
   the partner's NEW value. As-written this only updates one element
   of the pair correctly; the partner element gets the wrong rotation.
   Our existing `rope.wgsl` handles this by having each thread own
   one full pair (gets BOTH new values from BOTH old values). Need
   to either (a) split this kernel into pair-aware mode, or (b) verify
   that running it twice (once for each half of every pair) actually
   produces correct results. **This is the correctness check the
   parity test catches.**

10. **`flags` integer comparison `if ((params.flags & FLAG_BIAS) != 0u)`**
    — naga validates this fine. Standard pattern.

## Numerical parity expectations (theirs)

> "Expected parity target: |gpu - cpu| <= 1e-6
>
> Sources of tiny drift: fused accumulation ordering, SFU sin/cos
> precision, FP32 associativity.
>
> You should achieve: exact parity for V projection, epsilon parity
> for K RoPE."

Matches our experience with the existing fast paths. V should be
bit-exact since it's just dequant + matmul + bias (same ops we
already verified). K will have RoPE float drift in the last bit or
two, which is fine.

## Their "next major win after this fusion"

> "Fuse: QK attention score matmul + softmax + AV matmul
> into: persistent-head kernel — no intermediate score buffer
> That is where the next large Pascal gain lives."

This is **exactly** Prompt 4 territory (prefill batching) plus the
attention fusion we considered earlier. Confirmation from a Vulkan/
SPIR-V specialist that the persistent-head fused-attention kernel is
the right next target. We'll dispatch that prompt next.

## Integration plan when we land this

1. **Land V projection kernel first.** No RoPE drama, exact parity
   easy to verify. Becomes the proof-out of the new fused-cache
   write pattern.
2. **Land K projection kernel second** — shipped as the naive
   recompute version per the contributor's first-pass shader. Verify
   numeric parity vs current rope.wgsl + matvec + copy path. Smoke
   test on T440 + 1070.
3. **Land Q projection kernel third** — same shape as K kernel
   minus the cache write target. Replaces our current matvec + RoPE
   for Q.
4. **Optimization pass: rotary pair fusion in K and Q.** ~150 LOC
   shader change, no dispatch changes. Smoke test again.
5. **Optimization pass: shared trig reuse via workgroup memory.**
   Another ~50 LOC.
6. Total expected gain when all 3 kernels + both optimizations land:
   - 3 fewer dispatches per layer per token
   - 0 k_buf / v_buf allocations
   - 1.7-1.9× speedup on K/Q kernels themselves vs naive
   - Plus the dispatch overhead reduction (which compounds with the
     Vulkan fastpath work later)

Slots into the "after multi-model + benchmarking" queue alongside
the wgpu_hal port. This particular contribution is shippable any
time after the multi-model registry lands — doesn't need it as a
prerequisite, but it's cleaner to integrate the kernels through a
working registry path.

---

Filed under research_log because the V kernel is integration-ready
today, the K kernel needs the partner_row correctness check, and the
production pair-fusion + trig-share optimizations are follow-ups.
