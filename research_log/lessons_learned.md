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
