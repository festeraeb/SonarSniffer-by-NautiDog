---
inclusion: fileMatch
fileMatchPattern: 'cesarops-inference/**/*.rs'
---

# Gemma-4 MoE Inference: Hard-Won Lessons

When working in `cesarops-inference/`, especially around `gemma4_*` modules or any GPU compute path, these lessons should be loaded.

## Active known issues

* **Numerics:** the Gemma-4-26B-MoE forward pass produces incorrect text. Both `Gemma4Runner` (CPU-bouncing) and `Gemma4GpuRunner` (GPU-resident) share the same bug. Every individual GPU primitive is verified correct against a CPU reference; the bug is in **orchestration of the layer** (norm ordering, RMSNorm gain convention, or layer_output_scale interaction). See `docs/GPU_RESIDENT_RUNNER_REPORT.md` §3.
* **No reference logits dump yet.** Without one we cannot bisect by layer. Future debug should pull layer-0 hidden state from llama.cpp / Ollama for the prompt `"<bos> 3"` and compare against `Gemma4GpuRunner::scratch.norm_out` after `dispatch_attention_block(0)`.

## RMSNorm gain convention is unsettled

Two conventions appear in Gemma checkpoints:

| Formula | Used by |
|---|---|
| `out = (x / rms) * weight` | most non-Gemma models |
| `out = (x / rms) * (weight + 1.0)` | "canonical" Gemma 1/2/3 |

The Gemma-4-26B-MoE GGUF stores attn_norm with mean magnitude ~3-9 and `post_ffw_norm` with magnitudes up to ~16. With the `+1` convention these become gains of 5-17×, which corrupts everything. With raw weights output is also broken but differently. Both shaders (`rmsnorm_f32.wgsl`) accept a `plus_one_flag` push field — use both when bisecting.

## Pascal / wgpu gotchas

* `f16` extension is not safely available on Tesla P100. Use the `_f32` shader variants (`silu_mul_separate_f32.wgsl`, `weighted_accum_f32.wgsl`).
* `max_storage_buffer_binding_size` on P100 is `2,147,483,647` bytes. Embeddings + LM head for 26B at fp32 are 2.95 GB → split into two halves (`lo` / `hi`).
* `max_buffer_size` requested ≤ `2 GB - 1` byte — never pass `2 * 1024 * 1024 * 1024` exactly.
* Same `wgpu::Buffer` cannot bind to both `read_only` and `read_write` bindings within one dispatch. If you need to "in-place" something (RMSNorm-per-head), use a scratch + `copy_buffer_to_buffer` to overwrite.
* `device.create_buffer_init` is in `wgpu::util`, not on `Device` directly. Prefer `create_buffer + queue.write_buffer`.
* RoPE `head_dim` for Gemma-4 is 512 (full attn) or 256 (SWA). Hardcoded `workgroup_size 64` shaders silently truncate. Use stride-loop variants.

## GGUF dequant + tokenizer

* `loader::load(path, profile)` takes `&Path` not `&str`.
* `loader::ModelWeights::tensor_bytes(name)` returns the raw mmap'd bytes — pass these to `bridge::dequant_iq4_xs` / `dequant_iq4_nl` for CPU reference.
* Embedded vocab is at metadata key `tokenizer.ggml.tokens`. BOS/EOS at `tokenizer.ggml.bos_token_id` / `tokenizer.ggml.eos_token_id`.
* SentencePiece "▁" (U+2581) is the word-boundary marker. Always prefix the first non-empty token with one.

## Multi-GPU sharding

* `MultiGpuExpertContext` and `MoeFfnDispatch` exist and compile, but no actual cross-GPU activation transfer code is written. Pascal has no GPUDirect — any cross-device transfer must round-trip through host pinned memory.
* `enumerate_experts(model)` returns `Vec<ExpertTensor>`, not `Option<Vec<…>>`. Don't `?` it.

## Multi-GPU MoE bug fixes (already applied)

These were caught during multi-GPU plumbing:

* `lookup_expert` must destructure `&(gpu_idx, ref key)` — the `lookup` API returns `Option<&(usize, String)>`.
* `MultiGpuExpertContext` was duplicated in `gpu_tensor.rs` and `moe_multigpu_dispatch.rs`. Consolidated to `gpu_tensor.rs`.

## Performance baseline

P100, single-GPU, prompt `"3x3="`, greedy:

| Path | Decode rate |
|---|---|
| `Gemma4Runner` (CPU-bouncing) | 0.34 tok/s |
| `Gemma4GpuRunner` (GPU-resident) | 4.50 tok/s |

The GPU runner still does ~360 individual `queue.submit` calls per token. Bundling all dispatches per layer into one encoder is the next high-leverage perf change.

## Don't waste time on

* Adding speculative decoding / continuous batching before fixing §3 numerics.
* Optimizing the MoE router — top-k readback is the only unavoidable CPU sync per layer and is dominated by 128 floats × 30 layers = 15 KB per token, negligible compared to compute.
* Trying yet another single-line numerical fix without a reference comparison. We've burned multiple iterations on this.
