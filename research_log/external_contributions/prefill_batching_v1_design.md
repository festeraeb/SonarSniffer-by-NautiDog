# Prefill Batching Architecture for Pascal P100 — Design Response

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-17. Response to Prompt 4 from our outbound research
asks (prefill batching for KV cache fill on prompt ingest).
Status: **REFERENCE — ARCHITECTURE APPROVED with caveats. Two reference
shaders included, both have known perf/correctness issues called out
below before integration.**

## Verdict

**Proceed with the dual-mode architecture.** This is the right call.
Their breakdown matches our internal model:

- **Decode (M=1)** — bandwidth-bound matvec (GEMV). Keep our existing
  optimized scalar matvec kernels including the recently-fused Q6_K
  cache writes.
- **Prefill (M>1)** — compute-bound batched matmul (GEMM). New kernel
  family. Different scheduling. Different attention strategy.
- **Do not unify them.** A single hybrid kernel compromises both paths.

Chunk-size guidance:

| Chunk M | Status |
|---------|--------|
| 128     | safe |
| 256     | good |
| 512     | optimal target |
| 1024+   | usually worse on Pascal |

Their recommendation matches the chunking strategy in koboldcpp /
llama.cpp / MLX / vLLM. **512 is the target chunk size for our P100
prefill mode.**

## What this collapses (per chunk per layer per token-batch)

Currently for an M-token prompt we run M independent decode passes:
- M × ~17 submits per layer × 28 layers = M × ~476 submits
- M × per-token KV cache write
- M × small dispatches for QKV proj, attention, FFN

After prefill batching:
- 1 chunked pass per chunk of 512 tokens
- ~13 dispatches per layer per chunk × 28 layers = ~364 dispatches per
  chunk (versus ~245k for 512 tokens decode-style)
- Single batched KV cache write per chunk
- GEMM throughput unlocked on the projection + FFN layers

For a 4096-token context:
- Decode-only path: 4096 × 476 × 28 = ~55M submits
- Chunked prefill at 512: 8 × 364 = ~2,900 submits
- Reduction: roughly **4 orders of magnitude** in dispatch count for
  the prefill phase alone.

## Kernel conversion matrix (theirs)

| Kernel | Decode | Prefill |
|--------|--------|---------|
| RMSNorm | vector | batched vector |
| Q/K/V proj | GEMV | tiled GEMM |
| O proj | GEMV | tiled GEMM |
| FFN up/gate/down | GEMV | tiled GEMM |
| SwiGLU | vector | batched vector |
| RoPE | vector | batched vector |
| KV write | scalar | batched scatter |
| Attention QKᵀ | tiny matvec | tiled attention |
| Softmax | scalar row | batched row |
| AV | per-head matvec | tiled attention |

Critical insight in their note: **only the projection layers and FFN
linear layers need true tiled GEMM kernels.** Everything else stays
as elementwise/reduction/rowwise vector ops, just batched across M.
This significantly reduces the new-kernel surface area.

## Attention architecture

Decode attention stays as our split-by-head per-head dispatch (recently
optimized with the attention_pc push-constant pipeline).

Prefill attention is fundamentally different:
- Q[M,D] × K[M,D] = scores[M,M] per head
- Causal mask required
- Softmax row-wise across M
- AV[M,M] × V[M,D] = O[M,D] per head

Their guidance is unambiguous:
> "DO NOT run M independent decode attention passes. This destroys
> throughput.
> DO use a single fused causal attention kernel per chunk."

Without fusion, materializing scores[M,M] per head:
- M=512, head_dim=128: 1 MB per head temporary
- × 12 heads × 28 layers × 2 K+V = unworkable on Pascal scratch budget

Recommended structure: FlashAttention-lite traversal — Q tile × K tile
→ partial scores → masked softmax → V multiply, never materialize the
full score matrix. Their reference shader (below) is correct but does
NOT implement this; production version is a follow-up.

## Memory budget verification

Their hidden state stream estimate for M=4096:
- 4096 × 1536 × 4 ≈ 25 MB ✓ (matches our calc)

Their KV cache estimate:
- 28 × 4096 × 1536 × 4 × 2 ≈ 1.4 GB

**This estimate assumes MHA (n_kv_heads = n_heads = 12).** Our model
is Qwen 1.5B with GQA (n_kv_heads=2, n_heads=12, head_dim=128). Real
KV cache for 4096 ctx:
- 28 × 4096 × 2 × 128 × 4 × 2 ≈ **234 MB**

Their note about 88 MB being too low unless quantized/fp16/GQA reduced
heads — exactly. Our model is GQA so we land at 234 MB which is well
within P100's 16 GB budget alongside model weights (~1.5 GB Q6_K) +
activations.

For a future MHA model (Llama 7B/Mistral 7B class) the 1.4 GB number
is realistic and we'd need to add KV quantization or limit context.
Track for the post-multi-model phase.

## Push-constant layout (reference shader)

```
struct Params {
    M: u32,
    N: u32,
    K: u32,
    _pad: u32,
};
```

Tiny — 16 bytes. Plenty of room within our 64-byte limit if we add
flags/scale/strides for fused variants.

## Bind group layout (reference GEMM)

```
@group(0) @binding(0) var<storage, read>       A: array<f32>;
@group(0) @binding(1) var<storage, read>       B: array<f32>; // [N,K] row-major
@group(0) @binding(2) var<storage, read_write> C: array<f32>; // [M,N] row-major
```

Three bindings — matches our existing matmul layout pattern.

## Polish notes for integration (matmul_tiled_gemm.wgsl)

1. **16×16 workgroup, 16×16 shared tiles** — portable, naga-valid,
   simple. NOT optimal for Pascal. P100's SMs prefer 32×32 or 64×16
   tiles for occupancy. Their note explicitly acknowledges this:
   > "Compared to tinygrad/MLX/vLLM/CUTLASS, this WGSL version lacks
   > vectorized shared loads, double-buffered tiles, async copies,
   > tensor-core paths, warp-specialization, subgroup MMA. Reason: WGSL
   > portability constraints, Pascal no tensor cores, wgpu subgroup
   > limitations."

   **Integration plan:** ship the 16×16 reference for parity proof,
   then variant pass to 32×32 tiles + vec4 loads measured against the
   reference for actual Pascal perf.

2. **No vectorization in shared-memory loads.** Each thread loads one
   f32 from A and one from B per tile iteration. Pascal HBM2 prefers
   vec4 loads. ~2-3× bandwidth gain available here. Same as the matvec
   kernels we already vec4-ified — same trick applies. ~30 LOC change.

3. **Single accumulator per thread.** Each thread computes one C
   element. Standard register-blocking would have each thread compute
   a 4×4 or 8×8 tile of C, increasing arithmetic intensity. ~80 LOC
   change. Major Pascal win once we have parity baseline.

4. **No double-buffering of shared tiles.** Pascal can hide global-load
   latency behind compute if we overlap tile-N+1's load with tile-N's
   accumulation. WGSL workgroup memory + careful barrier placement can
   approximate this. Track as a follow-up — production-grade GEMM is
   its own project.

5. **K dimension not a multiple of 16** — handled correctly via the
   `if (a_col < params.K)` guard with zero-fill. Bounds-safe.

6. **Row-major B layout: `[N,K]`.** Matches our weight tensor convention
   (output features × input features). The shader indexes
   `B[col * params.K + b_col]` which is `B[N][K]`. Correct.

7. **Output layout: `[M,N]` row-major.** This is "M tokens × N output
   features" which is the natural prefill output layout. Compatible
   with feeding into the next layer as input directly.

## Polish notes for integration (attention_prefill.wgsl)

1. **Severe perf bug in their reference shader.** The score
   computation is nested INSIDE the d-loop:

   ```
   for d in 0..head_dim:
       for k_pos in 0..=q_pos:
           score = sum_i Q[q_pos,i] * K[k_pos,i]   // <-- recomputed head_dim times
           ...
   ```

   Score doesn't depend on d. As written, this recomputes the QK^T dot
   product `head_dim` times per (q_pos, k_pos) pair. For head_dim=128
   that's 128× redundant matmul work. Trivial fix: hoist the score
   computation out of the d-loop into a per-(q_pos,k_pos) pass that
   precomputes scores into a shared array, then iterate d.

   They flag the kernel as "correct, reference-quality, NOT
   production-optimal" — this is one of the things they mean. Must fix
   before any benchmarking otherwise we'll measure ~128× slowdown
   versus what's achievable.

2. **Online softmax not implemented.** Their inline `numer/denom`
   pattern works but is numerically fragile — `exp(score)` overflows
   for any score > ~88 in fp32. Standard FlashAttention online softmax
   tracks running max:
   ```
   m_new = max(m_old, score)
   p     = exp(score - m_new)
   numer = numer * exp(m_old - m_new) + p * V
   denom = denom * exp(m_old - m_new) + p
   ```
   Mandatory for correctness on prompt ingest where logits routinely
   hit triple digits at later layers. Bug 3 in DEBUG_LOG.md (clamp to
   [-30,30] destroying signal) is the same bug class on the decode
   side — softmax max-subtraction is required.

3. **GQA not handled.** The kernel indexes K and V using the Q head
   index. Our model is GQA: n_heads=12 Q heads, n_kv_heads=2 KV heads.
   Every 6 Q heads share one KV head. Indexing K/V with the Q head
   index will read past the KV cache for heads 2..11.

   **Fix:** map Q head index to KV head index via integer division
   `kv_head = q_head / (n_heads / n_kv_heads)`, then index K/V using
   `kv_head` and the GQA-shaped `idx_kv()` helper.

4. **`workgroup_size(64)` with 1-thread-per-(q_pos,head)** is
   under-utilization. Each thread does the full head_dim×k_pos×head_dim
   triple loop sequentially. Pascal's SMs want at least head_dim
   threads cooperating per (q_pos,head). Restructure as one workgroup
   per (q_pos,head) with the workgroup parallelizing across head_dim.
   Standard FA pattern.

5. **No causal mask test in their score computation.** They handle
   causality implicitly via the loop bound `for k_pos in 0..=q_pos`
   which works correctly for the unfused reference. When we move to
   the FA-lite tiled version we'll need an explicit mask test against
   tile boundaries.

6. **Q,K,V layout assumption: `[pos][head][dim]`.** Our current decode
   path stores K,V in the KV cache as `[pos][n_kv_heads][head_dim]`
   row-major. Q is computed fresh per token in `[1][n_heads][head_dim]`
   shape. The prefill version needs Q in `[M][n_heads][head_dim]` —
   which is the natural output of a batched Q projection — and K/V in
   `[M][n_kv_heads][head_dim]` ALREADY MATCHING the KV cache layout.
   This is convenient: the prefill K/V projection writes directly into
   the KV cache at the chunk's pos range, and the prefill attention
   reads from the same layout. No reshape/copy needed.

7. **Output layout: `[M][n_heads][head_dim]`.** Natural prefill
   attention output. Feeds directly into the O-projection GEMM as
   `[M][hidden_dim]` after concat-heads.

## Rust dispatcher reference

Their pseudocode:

```rust
pub fn forward(tokens: &[u32]) {
    if tokens.len() == 1 {
        decode_one(tokens[0]);
        return;
    }
    prefill(tokens);
}

fn prefill(tokens: &[u32]) {
    const CHUNK: usize = 512;
    for chunk in tokens.chunks(CHUNK) {
        let m = chunk.len();
        for layer in 0..N_LAYERS {
            rmsnorm_batched(m);
            qkv_gemm(m);
            rope_batched(m);
            kv_cache_write_batched(m);
            attention_prefill(m);
            o_proj_gemm(m);
            residual(m);
            ffn_up_gemm(m);
            swiglu(m);
            ffn_down_gemm(m);
            residual(m);
        }
    }
}
```

Clean — and this slots into our `forward_pass.rs` neatly. The decision
point at the top is just `tokens.len()`. Decode and prefill never share
a kernel; they share buffers, pipelines (constructed at startup), and
the KV cache layout.

## Integration sequence (when we land this)

1. **Plumbing pass** — add a `mode: ExecutionMode { Decode, Prefill }`
   parameter to `forward_pass::execute_layer`, route to the right
   kernel set. No new shaders yet, just routing. Smoke test: decode
   path unchanged, prefill mode dispatches existing decode kernels in
   a loop (slow but correct).
2. **GEMM kernel** — drop in `matmul_tiled_gemm.wgsl` reference. Wire
   for QKV/O/FFN-up/gate/down projections. Smoke test: prefill output
   matches decode-loop output bit-exact (this is f32 matmul, parity
   should be exact).
3. **Batched RMSNorm + RoPE + SwiGLU** — straightforward extensions
   of existing scalar versions. M outer dimension only. Parity test.
4. **Batched KV cache write.** Our existing fused K/V proj+rope+cache
   writes a single position at a time. Need a batched variant that
   writes M positions starting at `pos`. ~30 LOC shader change since
   the per-position kernel already exists.
5. **Reference attention_prefill.wgsl with the 4 fixes above:**
   (a) hoist score out of d-loop, (b) online softmax, (c) GQA
   indexing, (d) workgroup parallelism over head_dim. Parity test
   versus the decode-loop reference.
6. **Optimization pass on GEMM** — vec4 loads, 32×32 tiles, register
   blocking. Smoke test for parity each step.
7. **FlashAttention-lite tiled attention.** Tile K/V, online softmax
   in shared memory, never materialize scores[M,M]. ~300 LOC shader.
   This is the production attention.

Total estimate: ~1.5-2 weeks for the full prefill mode landing,
mostly dominated by step 7 and step 6's GEMM tuning.

## Where this slots vs other queued work

Priority order remains as the operator set:

1. **Multi-model loading registry** (in-progress, spec to land)
2. **`--bench` mode + real `EngineBenchmarker`** (queued)
3. **Q6_K K/V/Q proj+RoPE+cache fusion** (integration-ready V kernel,
   K/Q after correctness check on partner_row)
4. **Prefill batching** ← this drop
5. **wgpu_hal::vulkan submit-path port** (architecture approved,
   deferred)

Prefill batching after benchmarking lands because the benchmarker is
where we'll measure the prefill perf gains. Without instrumented
TFLOPs/utilization counters we'd be flying blind on whether our GEMM
is actually achieving Pascal's ~10 TFLOPS f32 ceiling versus leaving
performance on the table. Their attention shader's score-recompute
bug is a 128× perf trap that benchmark mode would catch instantly;
without it we could ship the bug and never notice.

## "Next major win" they flagged

> "Fuse: QK attention score matmul + softmax + AV matmul into a
> persistent-head kernel — no intermediate score buffer. That is
> where the next large Pascal gain lives."

Same recommendation as in the q6k_kv stash. Confirmation across both
drops that the FA-lite persistent-head fused attention kernel is the
right next target after this prefill mode lands.

---

## Verbatim source — reference shaders + dispatcher

Captured here so the source survives even if the upstream cluster log
rolls. **Do not paste these into the codebase as-is**; apply the polish
notes above first.

### `matmul_tiled_gemm.wgsl`

```wgsl
enable chromium_experimental_push_constant;

struct Params {
    M: u32,
    N: u32,
    K: u32,
    _pad: u32,
};

var<push_constant> params: Params;

@group(0) @binding(0) var<storage, read>       A: array<f32>;
@group(0) @binding(1) var<storage, read>       B: array<f32>; // [N,K] row-major
@group(0) @binding(2) var<storage, read_write> C: array<f32>; // [M,N] row-major

var<workgroup> Asub: array<array<f32,16>,16>;
var<workgroup> Bsub: array<array<f32,16>,16>;

@compute @workgroup_size(16,16,1)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let row = gid.y;
    let col = gid.x;

    var acc: f32 = 0.0;

    let tx = lid.x;
    let ty = lid.y;

    let num_tiles = (params.K + 15u) / 16u;

    for (var t: u32 = 0u; t < num_tiles; t++) {
        let a_col = t * 16u + tx;
        let b_col = t * 16u + ty;

        if (row < params.M && a_col < params.K) {
            Asub[ty][tx] = A[row * params.K + a_col];
        } else {
            Asub[ty][tx] = 0.0;
        }

        if (col < params.N && b_col < params.K) {
            Bsub[ty][tx] = B[col * params.K + b_col];
        } else {
            Bsub[ty][tx] = 0.0;
        }

        workgroupBarrier();

        for (var k: u32 = 0u; k < 16u; k++) {
            acc += Asub[ty][k] * Bsub[k][tx];
        }

        workgroupBarrier();
    }

    if (row < params.M && col < params.N) {
        C[row * params.N + col] = acc;
    }
}
```

### `attention_prefill.wgsl` — reference (HAS PERF + GQA + SOFTMAX BUGS, see polish notes)

```wgsl
enable chromium_experimental_push_constant;

struct Params {
    M: u32,
    n_heads: u32,
    head_dim: u32,
    scale: f32,
};

var<push_constant> params: Params;

@group(0) @binding(0) var<storage, read>       Q: array<f32>;
@group(0) @binding(1) var<storage, read>       K: array<f32>;
@group(0) @binding(2) var<storage, read>       V: array<f32>;
@group(0) @binding(3) var<storage, read_write> O: array<f32>;

fn idx(pos: u32, head: u32, dim: u32) -> u32 {
    return
        pos * params.n_heads * params.head_dim +
        head * params.head_dim +
        dim;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let q_pos = gid.x;
    let head  = gid.y;

    if (q_pos >= params.M) {
        return;
    }

    for (var d: u32 = 0u; d < params.head_dim; d++) {
        var numer: f32 = 0.0;
        var denom: f32 = 0.0;

        for (var k_pos: u32 = 0u; k_pos <= q_pos; k_pos++) {
            var score: f32 = 0.0;

            for (var i: u32 = 0u; i < params.head_dim; i++) {
                score += Q[idx(q_pos, head, i)] * K[idx(k_pos, head, i)];
            }

            score *= params.scale;

            let w = exp(score);

            denom += w;
            numer += w * V[idx(k_pos, head, d)];
        }

        O[idx(q_pos, head, d)] = numer / denom;
    }
}
```

### Rust dispatcher pseudocode

```rust
pub fn forward(tokens: &[u32]) {
    if tokens.len() == 1 {
        decode_one(tokens[0]);
        return;
    }
    prefill(tokens);
}

fn prefill(tokens: &[u32]) {
    const CHUNK: usize = 512;

    for chunk in tokens.chunks(CHUNK) {
        let m = chunk.len();

        for layer in 0..N_LAYERS {
            rmsnorm_batched(m);
            qkv_gemm(m);
            rope_batched(m);
            kv_cache_write_batched(m);
            attention_prefill(m);
            o_proj_gemm(m);
            residual(m);
            ffn_up_gemm(m);
            swiglu(m);
            ffn_down_gemm(m);
            residual(m);
        }
    }
}

fn decode_one(token: u32) {
    for layer in 0..N_LAYERS {
        rmsnorm_scalar();
        qkv_matvec();
        rope_scalar();
        kv_write_scalar();
        attention_decode();
        o_proj_matvec();
        ffn_scalar();
    }
}
```

---

Filed under research_log because:
- the GEMM reference is integration-ready after the vec4/tile-size
  optimization pass,
- the attention reference has 4 polish items that must be applied
  before parity testing makes sense,
- both production-grade variants (FA-lite tiled attention, GEMM with
  register blocking + double buffering) are follow-ups,
- the dispatcher architecture is approved as-is.

This drop unblocks the prefill phase whenever the `--bench` mode
lands and we have real perf instrumentation. Until then it sits here
with its polish notes intact.
