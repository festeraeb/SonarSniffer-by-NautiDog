# Multi-Model Loading v1 — External Contribution (Registry + Scheduler)

Source: dropped in by operator from a friend, 2026-05-16. Companion to
the MoE drops in this directory.
Status: **REFERENCE MATERIAL — NOT INTEGRATED YET.**

This is the scaffolding for in-process multi-model loading: device
registry, active model registry, priority scheduler, persistence,
and the HTTP multiplexer surface. Lines up directly with the spec
the operator is working on.

---

## What it ships

5 modules across the engine:

1. `src/registry/device.rs` — `DeviceRegistry` boots N wgpu devices
   (one per discrete GPU), compiles the parameter-agnostic
   `LayerPipelines` once per device, hands out `DeviceInstance` refs.

2. `src/registry/model.rs` — `ActiveExecutionRegistry` tracks loaded
   `ActiveModelInstance`s in an `RwLock<HashMap<String, Arc<…>>>`,
   enforces `max_concurrent_models`, and runs a per-GPU VRAM budget
   check with a 1.2 GB system reserve before each load.

3. `src/orchestration/scheduler.rs` — `MultiQueueScheduler` with 4
   priority queues (Admin/High/Standard/Background) backed by
   `crossbeam_queue::SegQueue`, lock-free `submit_job` /
   `pop_next_job`. Default-model fallback for legacy unprefixed
   kobold calls.

4. `src/config/persistence.rs` — `EnginePersistenceConfig` JSON
   round-trip so loaded model topology survives restarts when
   `persistence: true`.

5. `src/server/routes.rs` — axum router with three new endpoints
   plus the existing kobold compat:
   - `POST /api/v1/generate` — kobold legacy, model_id falls
     through to `default_model`
   - `POST /v1/chat/completions` — OpenAI-style, takes `model` field
   - `POST /api/v1/model/load` / `unload` — admin lifecycle

This matches the architecture I sketched in the multi-model spec
clarification (Q1 device topology, Q2 routing, Q4 default model
behavior). It also slots cleanly into the answers I expect we'll
end up with.


---

## Polish notes for integration

These are the gotchas I'd hit if I tried to compile this drop as-is.
None of them break the design — they're just the tiny things that
fail naga / cargo until smoothed out.

1. **`DeviceRegistry::init_from_system` always picks the first
   adapter** — `instance.request_adapter(&...)` doesn't take a GPU
   index parameter. wgpu's adapter selection by index needs
   `instance.enumerate_adapters(backends)` then index into that Vec.
   Our existing `gpu_context::GpuContext::init` already does this
   correctly; pull from there.

2. **`Limits::downlevel_webgl2_defaults()` is the wrong floor.** It
   caps `max_storage_buffer_binding_size` at 128 MB, way under the
   ~1 GB tensors we load. Use `Limits::default()` and override
   `max_storage_buffer_binding_size: 1024 * 1024 * 1024` like
   `gpu_context::GpuContext::init` does.

3. **`GpuContext::from_raw(device, queue, idx)` doesn't exist** in
   our tree. Either add it (small constructor) or factor
   `init_from_index(idx)` to take a pre-built device/queue pair.

4. **`LayerPipelines::compile_universal` is a new constructor we
   don't have.** Today's `pipeline_init::init_layer_pipelines(device)`
   does this work — rename or wrap it.

5. **`ScratchBuffers::total_bytes()` exists** ✓ — already in our tree.
   Good.

6. **`KVCache` ArrayQueue** — `crossbeam_queue::ArrayQueue<KVCache>`
   needs a fixed capacity. Sizing: at minimum `n_layers` per request
   slot. For the default 4-model cap × Qwen-class 28 layers = 112
   chunks. Worth a config knob.

7. **`uuid` and `crossbeam-queue`** are new Cargo deps. `serde_json`,
   `serde`, `axum`, `tokio` already present.

8. **`futures::executor::block_on` inside `init_from_system`** —
   we're inside an async-capable engine startup. Use the existing
   `pollster::block_on` (already a transitive dep) or just make
   `init_from_system` async itself. Current `gpu_context::GpuContext::init`
   is already async; match that.

9. **`KoboldGenerateRequest` is missing the existing optional fields**
   (`max_length`, `temperature`, `top_p`, `rep_pen`, `stop_sequence`,
   `use_chat_template`). These are already serde-defaulted in the
   live `server.rs` `GenerateRequest` struct — port the full struct,
   not the trimmed version.

10. **`OpenAICompletionsRequest` is missing the `messages` array**
    that `/v1/chat/completions` actually takes — the contributor
    used a `prompt` field which is `/v1/completions` semantics.
    Need both endpoints or unify the model on `messages`.

11. **`handle_load_model`'s `mock_hardware_vram_limit = 16_000_000_000`**
    and `estimated_model_footprint = 5_000_000_000` are placeholders.
    Real values: query `wgpu::Adapter::get_info()` + actual GGUF size
    from `ModelWeights` (we already have `weights.tensor_bytes()`
    plumbing).

12. **No actual model load logic** — the contributor is explicit:
    "In a real execution graph, parsing logic would invoke
    model_initializer layers here." That's the part the integration
    pass writes. The shape is: parse GGUF → arch_detect → allocate
    LayerWeights → compile-or-reuse pipelines → register.

13. **`ActiveExecutionRegistry::remove_model` VRAM accounting** uses
    a hand-rolled estimate (`scratch.total_bytes() + max_seq_len * 512`).
    Should track the *actual* allocated bytes per model — which means
    storing the load-time estimate in `ActiveModelInstance` itself
    so unload returns exactly what it took.

14. **No eviction policy.** Per the multi-model spec answer, this is
    fine — fail with 507 / require manual unload. The contributed
    code already does that via `verify_and_allocate_budget` returning
    `Err`. Just need the HTTP handler to map that to status 507.

15. **`MultiQueueScheduler::pop_next_job`** is strict priority
    (always drains Admin before any Standard). For starvation
    safety we'd want weighted-deficit-round-robin (the contributor
    mentions it in the section title but didn't implement it).
    For v1 strict priority is fine; flag for follow-up.

---

## Integration order (after operator finalizes the multi-model spec)

1. **Extract**: copy these scaffolds into a feature branch
   `feat/multi-model-v1`. Apply polish notes 1-9 to make it compile.
2. **Wire device registry** as a replacement for the single
   `gpu_context::GpuContext` in main.rs. `serve` mode constructs
   the registry once.
3. **Implement `model_initializer::load_model`** that takes a path
   + pinned_gpu and produces an `ActiveModelInstance`. This is the
   actual GGUF parse → arch detect → LayerWeights load pipeline,
   refactored from main.rs's `run_generate_mode`.
4. **Wire HTTP routes**: replace `server::run_server`'s axum
   router with the multiplexer from this drop, plumbed to the
   scheduler and registry.
5. **Worker thread**: tokio task that drains the scheduler
   (`pop_next_job`) and runs jobs through `generate_tokens` against
   the right `ActiveModelInstance`.
6. **Persistence**: load `EnginePersistenceConfig` at startup, run
   `load_model` for each provisioned model. Save on graceful
   shutdown.
7. **Smoke test**: load Qwen 1.5B + load TinyLlama, send `/v1/chat/completions`
   to each by name, verify both work concurrently. Expect different
   GPUs if more than one is available locally.
8. **Regression gates**: T440 P100 + cesarops2 1070 must both stay
   green throughout.

---

## What this unblocks

If this design lands cleanly, the MoE drops slot in naturally:
`ActiveModelInstance::moe_pool` is already in the contributed
struct, and the dispatcher signature matches part 2's `MoeBufferPool`.
That makes the MoE follow-up spec a much smaller delta — just the
shaders + loader + dispatcher, not the registry/lifecycle work.


---

## Operator's Final Spec Decisions (2026-05-16)

These are the answers to the 6 clarifying questions. Locking these
into the spec so the integration pass doesn't re-derive them.

| # | Question | Answer |
|---|---|---|
| 1 | Device topology | **KoboldCPP-style per-GPU pinning only.** Models bind 1:1 to a `gpu_index`. Cake-style sharding deferred — severe sync penalties on Pascal PCIe. |
| 2 | Eviction / lifecycle | **Reject with HTTP 507 Insufficient Storage.** No silent LRU. Unload is deliberate API call only — avoids unprompted driver stalls mid-inference. |
| 3 | Max concurrent models default | **4.** Balanced for dual-P100 (16GB each) plus system reserve. |
| 4 | Legacy fallback (empty `model` field) | **Route to configured `default_model`.** No 400 error. |
| 5 | MoE scope | **Compile the framework now, keep complex sharding out of v1 merge.** Single-pass 3D coalesced memory layout handlers ship; full-blown distributed expert execution stays staged. |
| 6 | Other | Persistence: yes, auto-restore on reboot. Telemetry: per-model isolated scorecards, async log. Fairness: **Deficit Round-Robin**, Admin/High class can preempt. |

These directly map onto the contributed code in this drop:
- (1) → `gpu_index: usize` field on `ActiveModelInstance` ✓
- (2) → `verify_and_allocate_budget` returns `Err` → handler maps to 507 ✓
  (polish note #14 in this file already flagged this — confirmed)
- (3) → `max_concurrent_models: usize` arg to `ActiveExecutionRegistry::new` ✓
- (4) → `default_model: RwLock<String>` in `MultiQueueScheduler`, used in
  `submit_job` when `job.model_id.is_empty()` ✓
- (5) → `MoeBufferPool` slot pre-allocated on `ActiveModelInstance`,
  initialized only when arch_info.is_moe ✓
- (6) → Persistence module already in the drop;
  scheduler is currently strict-priority, **needs DRR upgrade** before merge.

Polish note added: **upgrade `MultiQueueScheduler::pop_next_job` from
strict priority to Deficit Round-Robin per operator's decision.** Strict
priority risks low-priority queue starvation.
