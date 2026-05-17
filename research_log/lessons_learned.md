# Cesarops Fleet Lessons Learned

Polisher-curated notes that the orchestrator pushes when fleet members produce code that needed fixing or when a model behavior pattern shows up.

When nautivecs comes online, ingest this file via the `/add` endpoint so `think_harder` can retrieve these lessons automatically.

---

## [model-behavior,qwen3.6,thinking-mode] 2026-05-16
Qwen3.6-35B-A3B-MoE on the local koboldcpp endpoint defaults to a chain-of-thought ("thinking") mode and burns ~70-80% of generation tokens on internal reasoning even when asked for direct code output. The deliverable arrives at the bottom — buried in the reasoning trace.

**Mitigation when dispatching to qwen3.6 family:**
- Strip `<think>...</think>` blocks from the response before treating output as code.
- Or set generation params to forbid the `<think>` open token.
- Prefer Gemma-4-MoE for direct code-generation tasks; route qwen3.6 to actual reasoning tasks (architecture decisions, debugging traces, why-does-this-fail questions).

## [model-behavior,gemma4,code-generation] 2026-05-16
Gemma-4-26B-MoE on koboldcpp produced clean, working Rust speculative-decoder code on a single attempt, with stable softmax, both correction branches, and 2 unit tests. Only post-processing needed: strip the surrounding markdown ```rust fences. Grade A-.

**Use Gemma-4-MoE for:** API-spec-driven code (where a clear API contract is given in the prompt), single-file deliverables, anything that doesn't need architectural reasoning.

## [task-format,patches,brittle] 2026-05-16
Both Gemma-4 and Qwen3.6-A3B fabricate context lines when asked to produce unified diffs. Qwen even hallucinated wgpu API names (`base_groups`, `base_bindings` aren't real `PipelineLayoutDescriptor` fields).

**Don't ask the fleet for diffs.** Ask for full file output (or a clearly-bounded code block), then the orchestrator computes the diff and applies it.

## [tool-wiring,kobold-probe] 2026-05-16
The forge `/cluster/discover` was probing only `/health` to determine if a worker is up. Koboldcpp does NOT expose `/health` — it returns 404. So every kobold worker showed offline despite responding correctly on `/api/extra/version` and `/v1/models`.

**Probe priority:** `/api/extra/version` → `/v1/models` → `/health`. First success wins.

## [tool-wiring,routing,lan-vs-tailscale] 2026-05-16
Forge previously gated all remote service probes behind Tailscale peer state. When the user moved cesarops2 (1070+P1000) and cesarops3 (1060) to LAN IPs (10.0.0.x), they showed permanently offline because Tailscale doesn't see them at LAN addresses.

**Routing rule:** localhost + RFC1918 (10.x / 192.168.x / 172.16.x) → optimistically reachable, port probes decide. Only `100.x` (Tailscale) requires `tailscale status` to confirm reachability. M2200 stays on Tailscale because it's mobile.

## [persistence,toml-edit] 2026-05-16
Forge `update_worker_config` / `update_worker_field` / `update_corrector_function` were TODO stubs that only logged. APPLY buttons and per-card toggles in the cluster panel appeared to work but never persisted to `cluster_config.toml`.

**Fix:** use `toml_edit::DocumentMut` for round-trip preservation of comments + ordering. Inline-tables (like `corrector_functions = { json_fixer = true, ... }`) need `Item::Value(Value::InlineTable(_))` not `Item::Table(_)`.

## [route-table,axum] 2026-05-16
Don't register the same path twice on different handlers in axum 0.8 — it panics on `Router::serve` with "Overlapping method route". Use a distinct path (e.g. `/cluster/config/full` vs `/cluster/config`).

## [path-extractor,axum] 2026-05-16
If the route declares `/cluster/worker/{name}/start` and the handler is `Path<usize>`, every call fails because the path segment is a worker name string. Either match the type to the path param or accept `Path<String>` and resolve internally.


## [model-behavior,gemma4,vec4-accumulator] 2026-05-16
Gemma-4-26B-MoE struggles to reproduce vec4 accumulator semantics in WGSL even when given a near-identical reference shader. The model reverts to using `dot()` (which returns scalar f32), then realizes it can't add f32 to a vec4 acc, and spirals into self-reflection until the token budget runs out. Polisher (Claude) wrote `matvec_bias_vec4_pc.wgsl` directly using the proven pattern from `matvec_vec4_pc.wgsl`.

**Pattern to capture for future fleet dispatches:** when asking a fleet model for a vec4-accumulator shader, give the EXPLICIT loop body (`acc = acc + x0*w0 + x1*w1 + ...`) plus the EXPLICIT lane-sum at the end (`output[n] = acc.x + acc.y + acc.z + acc.w + ...`). Do not let them invent the inner-product expression — they get it wrong.

## [model-behavior,qwen3.6,raw-generate] 2026-05-16
Qwen3.6-A3B-MoE on the koboldcpp `/api/v1/generate` raw-prompt endpoint emits only 1 token (empty completion) because the model expects a Qwen chat-template (`<|im_start|>user\n...<|im_end|>\n<|im_start|>assistant\n`). Without the template the EOS fires immediately.

**Mitigation:** route Qwen3.6 dispatches through `/v1/chat/completions` (OpenAI-compat) which applies the template automatically. Don't use `/api/v1/generate` for Qwen3.6.

## [tool-rule,three-strikes] 2026-05-16
Round 4: matvec_bias vec4_pc + attention_pc — Gemma+Qwen both failed (0 strikes used → polisher writes both).
Round 5: Q6_K fused matvec + pipeline_cache wire — Gemma+Qwen both failed again (1 fleet-strike each).

When the same model fails the same general pattern (Qwen on raw-generate, Gemma on vec4 accumulators) the lesson is more valuable than another retry. Capture lesson, polisher writes it, move on. Don't burn fleet cycles on patterns we've already learned.

## [scope-decision,pipeline-cache-defer] 2026-05-16
Pipeline cache wire-in (load+save the wgpu binary cache) was queued. Investigation showed:
- `Device::create_pipeline_cache` requires `Features::PIPELINE_CACHE` (separate from `PUSH_CONSTANTS`), and we'd need to thread `cache: Option<&wgpu::PipelineCache>` through every `create_compute_pipeline(...)` call in `pipeline_init.rs` (12+ sites) plus `shader_ops.rs` and `tensor_chunker.rs`.
- The cold-start gain is unclear without measurement; we're already at 77s smoke (cold engine + model load + 12-token decode). Pipeline compile is a small fraction.
- Risk of regression on a non-test feature is high.

Decision: **defer until cold-start time becomes a measurable bottleneck.** The module exists and is feature-complete; the plumbing is what needs to land. When we benchmark and see pipeline compile dominating cold-start, revisit.


## [scope-decision,command-buffer-replay-deferred] 2026-05-16
Engine optimization #4 (command buffer replay) was queued. Investigation showed:
- wgpu's `CommandBuffer` is single-use by design — once submitted to the queue, it's consumed. wgpu 24+ has no public API for Vulkan-style `vkCommandBuffer` reset/reuse.
- Bind groups + buffers ARE reusable (we already do this via `ScratchBuffers`); only the encoder/buffer wrapping is per-submit.
- The realistic win — batching multiple layers into a single submit — collides directly with the P100 storage-buffer hazard the engine was specifically designed to honor (`forward_pass.rs` comment: "P100 Vulkan requires a submit boundary whenever op B reads a buffer written by op A").
- A real implementation would require dropping to `wgpu_hal` raw Vulkan, which trades portability for a sub-millisecond gain that's invisible at our current 76s/26s smoke times.

Decision: **defer command-buffer-replay indefinitely.** The remaining submit-overhead win is not worth the complexity at our current scale. Revisit only if (a) we drop to a tighter sync model that allows multi-layer batching, or (b) profiling shows queue.submit() is the bottleneck — currently it isn't.

Better next moves on the same theme:
- **Direct KV cache write from projection kernel**: skip the `copy_buffer_to_buffer` from k_buf/v_buf into the cache by having the K/V projection kernel write directly to the cache offset. Saves 2 copies per layer per token. Requires a new shader variant that takes `cache_byte_offset` as a push constant.
- **Pre-record submission objects** as `Vec<wgpu::CommandBuffer>` per layer pre-token: still creates new ones each call but caches the bind groups. Modest gain.
- **Async compute queue for KV cache writes**: overlap K/V copy with next layer's QKV proj. Pascal supports a separate compute queue family.

## [model-behavior,r1-7b-design-docs] 2026-05-16
DeepSeek-R1-Distill-Qwen-7B on the raw `/api/v1/generate` endpoint, when asked for a markdown design document, produces GLSL-flavored pseudocode mixed with reasoning blocks (`</think>` tags leak through). Output is not usable as a spec.

Mitigation: either route through a chat-template-aware endpoint (`/v1/chat/completions`) or use R1-7B only for narrow-question Q&A (single-paragraph answers, code snippets up to ~50 lines). Don't ask it for full design proposals.

## [model-behavior,shader-generation,row-vs-lane-mixup] 2026-05-16
A "Pascal-tuned wgpu/Vulkan/SPIR-V" agent cluster delivered a Q6_K fused
matvec v2 that internally mixed two parallelism idioms: it indexed `row =
gid.x` (per-thread row, the WG=256 idiom from our v1) AND ran a 128-thread
shared-memory reduction at the end (the one-WG-per-row idiom). Result: the
reduction sums partials from 128 *different* rows and writes the cross-row
total to one output slot, leaving 127/128 outputs unwritten. Each thread's
"lane-stride inner loop" also runs only once per kb iteration because `l =
lane; l += 32; if l >= 32 break;`.

This is the third distinct class of fleet shader-generation failure we've
catalogued:
1. Vec4 accumulator confusion (Gemma-4) — model can't carry vec4 acc through dot()
2. Raw-prompt EOS (Qwen3.6 on `/api/v1/generate`) — chat template missing
3. **Parallelism-idiom mixup (this one)** — model picks "WG=128 with shared
   reduction" from one mental template and "row = gid.x" from another, doesn't
   notice the contradiction, ships an internally inconsistent kernel.

**Mitigation when asking the fleet for collaborative-thread shaders:**
- Specify the parallelism explicitly in the prompt: "one workgroup serves one
  row, all 128 threads collaborate via shared-memory reduction" or "one thread
  serves one row, no shared memory".
- Verify by checking: if the kernel uses `workgroupBarrier()` or `var<workgroup>`
  storage, the row index MUST come from `wid.x` not `gid.x`. If it uses
  `gid.x`, there must be no inter-thread sync.
- Other tells of the mixup: a "lane-stride inner loop" (`l = lane; l += 32`)
  alongside a per-thread row index — the lane stride is collaborative-thread
  vocabulary in a per-thread context.

The fused-decode + vec4-dot + branchless fp16 + WG=128 pieces of the v2 drop
were genuinely good — the structural bug is mechanical and fixable. The
lesson is that the fleet's "tuned" agents still don't validate their own
parallelism semantics, so the polisher has to.

## [external-code,perf-trap,attention] 2026-05-17
External attention reference shaders frequently nest the QK^T score computation INSIDE the head_dim output loop, recomputing the same dot product `head_dim` times per (q_pos, k_pos) pair. For head_dim=128 that's a 128× redundant matmul. **Always check loop nesting structure** when reviewing attention kernels from the cluster — the score depends only on (q_pos, k_pos, head), not on d. Hoist score computation OUT of the d-loop, store per-(k_pos) scores in shared memory or registers, then iterate d.

This is the same bug class as the prefill_batching_v1 drop's `attention_prefill.wgsl`. Polish notes flagged it before any benchmarking ran. Without the `--bench` mode landed first, this kind of bug would ship invisibly.

## [external-code,softmax,numerical-stability] 2026-05-17
External attention shaders almost always ship with the naive `numer += exp(score) * V; denom += exp(score)` softmax pattern. This overflows in fp32 for any score > ~88. Standard FlashAttention online softmax is mandatory:
```
m_new = max(m_old, score)
p     = exp(score - m_new)
numer = numer * exp(m_old - m_new) + p * V
denom = denom * exp(m_old - m_new) + p
```
This is the same bug class as DEBUG_LOG.md bug #3 (clamp to [-30,30] destroying signal) — softmax max-subtraction is required, not optional. Apply this fix to every attention kernel from the cluster before parity testing.

## [external-code,gqa-indexing] 2026-05-17
External attention/KV reference shaders default to MHA indexing (using Q head index for K/V access). Our model is GQA (n_heads=12 Q heads, n_kv_heads=2 KV heads, every 6 Q heads share one KV head). **Always check K/V indexing** when integrating: map `kv_head = q_head / (n_heads / n_kv_heads)` before indexing K/V buffers. If the cluster's shader uses bare `head` for K/V it'll read past the KV cache for heads beyond `n_kv_heads-1`.

## [external-code,allocator,reset-cadence] 2026-05-17
External scratch-pool designs frequently default to either per-token reset (wastes intra-pass reuse) or persistent-across-passes (compounding fragmentation). The right cadence for transformer inference scratch is **per forward pass**, because:
- KV cache already defines the token-level persistence boundary, so scratch doesn't need to live across tokens
- All per-layer scratch (attention scores, FFN intermediate, projection outputs) has lifetimes that collapse cleanly to forward-pass boundaries
- Per-layer reset would underutilize reuse within a layer (e.g., tiled attention reusing K-tile across multiple Q tiles)

Forward-pass-scope reset is the canonical choice. Recorded so future scratch-allocator polish doesn't re-derive this.

## [external-code,allocator,sync-correctness] 2026-05-17
Reset-style scratch pools have one easy-to-miss correctness flag: **the GPU must finish consuming the previous frame's allocations before reset() runs.** Otherwise reuse during in-flight dispatch causes silent corruption. wgpu doesn't enforce this for us. The pattern:
- `submit()` returns a `SubmissionIndex`
- Forward pass returns the index to the caller
- Caller calls `device.poll(Maintain::WaitForSubmissionIndex(idx))` before resetting the pool
- This is fence-equivalent in wgpu and upgrades cleanly to native fences when wgpu_hal port lands

Make `reset()` private and expose only `reset_after(submission_idx, device)` to enforce this at the type level.

## [external-code,yagni,dead-code] 2026-05-17
External allocator designs often ship with both bump-pointer and free-list paths "for completeness." With forward-pass-scope reset, every allocation has the same lifetime so the free list is dead code. Drop it. ~30 LOC simpler, lock-free, easier to reason about. If we later need mid-pass reuse, add it then. Pure YAGNI.

## [design-principle,subgroup-locked-pipeline] 2026-05-17
Doctrine adopted from cluster: every dispatch's workgroup size MUST be a multiple of the device's `subgroup_size`. Active-lane utilization is an explicit correctness constraint, not a performance optimization.

Concrete rule:
- subgroup=32 (NVIDIA Pascal/Volta/Turing/Ampere) → workgroup=128 default
- subgroup=64 (AMD wave64, older RDNA) → workgroup=256 default
- variable RDNA → use device's `subgroup_max_size` as the multiplier base

For matvec kernels where `out_dim < workgroup_size`, restructure to either (a) shrink the workgroup to match, or (b) parallelize multiple output rows per workgroup to fill threads. Hardcoded 256-thread workgroups are an anti-pattern when out_dim varies.

## [design-principle,kernel-fusion-contract] 2026-05-17
Doctrine adopted from cluster: every fused kernel MUST satisfy:
1. operates on subgroup-sized tiles
2. NEVER writes intermediate global buffers (only registers and shared/workgroup memory between stages)
3. passes data via registers → shared memory only between stages
4. avoids dispatch chains inside the kernel (no recursive enqueue)

Consequence: the K-buffer + V-buffer + `copy_buffer_to_buffer`-into-cache pattern we currently use violates rule 2. The q6k_kv stash fix replaces that with direct cache writes from inside the projection kernel.

## [target-architecture,dispatch-budget] 2026-05-17
Post-fusion per-layer dispatch target on Pascal:
1. rmsnorm
2. fused_qkv_rope_cache (replaces matvec×3 + bias×3 + rope×1 + copy×2)
3. fused_attention (FA-lite, replaces split per-head)
4. out_proj_bias (matvec + bias fused)
5. rmsnorm (post-attention)
6. fused_ffn (up + gate + SwiGLU + down)

= 6 dispatches per layer × 28 layers = 168 dispatches per token.
Down from current ~476 = 2.8× dispatch reduction per token.

Stacks with wgpu_hal submit-path port for an additional dispatch-overhead reduction (per-submit cost approaching zero with command-buffer reuse).

## [perf-projection,t-per-s-ladder] 2026-05-17
Realistic tokens/sec projection ladder for Qwen 1.5B Q6_K on P100 16GB:
- Current baseline: 2.2 t/s
- + fused K/V/Q proj+RoPE+cache (q6k_kv stash): 3.0-4.0 t/s
- + FA-lite fused attention: 5.0-7.0 t/s
- + KV cache fp16 promotion: 6.0-9.0 t/s
- + KV cache read amplification fix (pending cluster drop): 8.0-14.0 t/s
- + wgpu_hal submit path: 12.0-25.0 t/s
- + persistent-head fused attention: 18.0-35.0 t/s

Each step assumes the prior step has landed. Sub-linear stacking because each fix removes part of the bottleneck the next one was assuming. Target koboldcpp parity (60+ t/s) requires steps beyond this ladder (probably packed-int8 paths + speculative decoding).

## [external-code,softmax-naive,three-strikes] 2026-05-17
The cluster has now shipped THREE attention/KV kernels with naive `numer/denom` softmax (no max-subtraction): prefill_batching_v1 attention_prefill, fusion_rewrite_architecture attention_kv, and kv_prefetch_v1 attention_compute. Each one will overflow fp32 for any score > ~88. The fix (online softmax with running max) is cheap and mandatory.

This is now a confirmed pattern, not a one-off. Going forward: assume every external attention kernel ships with naive softmax and budget the polish-pass for it. Same as how we budget the GQA indexing fix and the score-loop-nesting fix.

Three-strikes rule applies: don't ask the cluster for "polished softmax" again. Just ship the fix in our integration polish layer.

## [external-code,megakernel-rejected] 2026-05-17
Cluster offered a persistent megakernel design for "zero-sync decode loop" (Stage #12). The technique works in CUDA but has Vulkan/wgpu portability and correctness risks:
- Vulkan TDR timeout exposure on consumer cards (1070 may enforce, P100/P40 typically don't)
- Naga validation may reject GPU-polling-state-buffer infinite loop patterns
- Cross-workgroup state via storage buffers needs explicit atomics + memory barriers (drop did not address)
- Read-after-write hazard inside a single dispatch is exactly the bug we hit on P100 storage buffers (DEBUG_LOG bug #1)
- Trades observability for performance — we cannot bisect a 28-layer megakernel by dumping intermediates

The same dispatch-cost reduction is achievable via the wgpu_hal::vulkan submit-path port (pre-recorded command buffers + replay) without these risks. Architecture choice: stick with multi-dispatch + wgpu_hal, reject persistent kernels.

## [submit-cost-calibration,vulkan] 2026-05-17
Per cluster: Vulkan `vkQueueSubmit` ≈ 10-50 µs per dispatch on Pascal. At 30 µs average × 476 submits/token = ~14 ms of submit overhead per token. At 2.2 t/s (455 ms/token), that's ~3% pure-submit overhead.

Implication: the wgpu_hal port's value is NOT removing 14 ms of submit cost (it's only 3% of token time). The real gain is removing per-submit barrier insertion + bind group revalidation, which compounds across 476 submits and is much harder to measure but much bigger.

Calibration: don't oversell wgpu_hal as "30-60% gain" (cluster's general claim). For our specific workload at 476 submits/token, the gain is in the per-submit overhead REDUCTION, not the submit count itself. Realistic projection: 1.3-1.5× gain on P100 from wgpu_hal alone, stacking sub-linearly with kernel fusion.

## [external-code,doctrine-drift] 2026-05-17
The cluster has now contradicted ITSELF on two architecture decisions across drops:
1. Capability detection part 1 explicitly rejected device-name-string matching ("vendor branching is an anti-pattern"). The wgpu_hal scratch pool drop two messages later used `device_name.contains("P100")` for classification.
2. Attention scratch pool drop adopted forward-pass-scope reset with no free list. The wgpu_hal scratch pool drop reverted to per-layer reset with a `reset_each_layer` boolean flag and bump-only offset (no per-pass reset at all).

Implication: cluster outputs DRIFT across messages. They don't carry their own prior recommendations forward. Each drop is fresh-context.

Mitigation: when integrating, ALWAYS re-apply our previously-locked overrides. Don't assume a later drop respects an earlier polish. Specifically check for:
- device-name string matching (replace with feature bitfield)
- naive softmax (replace with online max-subtract)
- MHA-default indexing (replace with GQA mapping)
- per-layer-only allocator reset (replace with reset_after(submission_idx))
- score-loop nested in d-loop (hoist out)

This is now confirmed as the ~5 standard polish patterns per cluster drop.

## [design-principle,three-stream-memory] 2026-05-17
Final memory architecture per cluster framing:
- Stream 1: KV cache - bandwidth-bound, persistent across tokens, never reset, lives in device-local-fast heap
- Stream 2: FFN intermediates - compute-bound, fused inline (registers + small workgroup memory), never hits global memory
- Stream 3: Attention scratch - subgroup-windowed, per-pass reset, lives in pool

Three orthogonal lifetime tiers. Each stream gets its own placement strategy. The ScratchManager only owns Stream 3 - KV cache is owned by model context, FFN intermediates are register/workgroup-only.

Useful test for future kernel design: which stream is the data in? If it crosses stream boundaries (e.g., a "scratch buffer for intermediate FFN result that gets read back next layer") it's an architecture smell.

## [external-code,swiglu-vs-gelu] 2026-05-17
Cluster shipped a "fused FFN" kernel with the math expression `silu(h) * gelu_gate(h)`. This is wrong notation that mixes GeLU and SiLU. The actual SwiGLU pattern is `silu(W_gate · x) ⊙ (W_up · x)` - TWO separate weight matrices applied to the same input, then element-wise multiply. Their kernel had only ONE matmul before activation.

Different model architectures use different FFN flavors:
- GeLU FFN (GPT-2, BERT, older Llama): 2 matmuls, single activation
- SwiGLU FFN (Qwen, Llama-2/3, Mistral): 3 matmuls (gate, up, down), elementwise multiply
- ReLU FFN (rare): 2 matmuls

For our Qwen 1.5B target we need SwiGLU. Make activation pluggable via spec constant or push-constant flag so we can support multiple model families with one shader.

## [external-code,workgroup-memory-budget] 2026-05-17
Pascal workgroup memory limit is 48 KB. For Qwen 1.5B FFN with intermediate_dim=8960:
- gate vector: 8960 fp16 = 17.5 KB
- up vector: 8960 fp16 = 17.5 KB
- gate * up holding both = 35 KB peak before elementwise multiply

Fits in 48 KB but leaves only 13 KB for other shared-memory uses. Tight.

Standard mitigation: chunked streaming SwiGLU - compute gate × up element-by-element in workgroup-sized chunks (128 elements = 256 B per chunk). Working set drops from 35 KB to ~512 B. ~70× less shared-memory pressure.

When designing FFN-class fused kernels, ALWAYS check shared-memory budget against intermediate_dim × 2 for gated activations. Hand-waving "shared memory (optional)" without numbers is a red flag.

---

## DII vs Tensor-Parallel (May 17, 2026)

**Distributed Independent Inference (DII)** is the correct architecture for our fleet.
Never attempt NCCL-style tensor-parallel over Ethernet/PCIe on Pascal hardware.

Key lessons:
- Each node runs a complete model independently. Network carries only prompts/completions.
- Queue-per-model routing (not queue-per-node) handles heterogeneous fleets correctly.
- Cross-node speculative decoding works when inter-node latency < 50ms (LAN).
- Fault isolation: one node crash = one lost request, not a broken tensor graph.
- KV-prefix-aware routing minimizes prefill cost — route continuations to the node that already has the session cached.
- The "Compute Exchange" model (contribute GPU → earn credits) is the sustainable path for volunteer federation.
- `cesarops-node` daemon is the implementation vehicle: register, heartbeat, spawn/stop, VRAM accounting.

See: `research_log/external_contributions/distributed_independent_inference_doctrine.md`
