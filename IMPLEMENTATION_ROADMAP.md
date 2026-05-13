# CesarOps Implementation Roadmap
## May 13, 2026 — Post-Session Status

---

## WHAT'S WORKING RIGHT NOW

| Component | Port | Status |
|-----------|------|--------|
| KoboldCPP (Qwen3.6-35B MoE) | 5001 | ✅ Running on both P100s |
| Forge-v2 Web UI (chat + cluster panel) | 9100 | ✅ Running |
| Nautivecs (vector search, 12.6k chunks) | 5003 | ✅ Running |
| cesarops-inference (1.5B bootstrap, CPU) | 5002 | ⚠️ Stopped (was testing) |
| Corrector "Marvin" (14B on 1070) | 5555 | ❌ Offline (cesarops2 not serving) |
| WSO (web search oracle) | 5010 | ❌ Offline |
| n8n (orchestration/translator) | 5678 | ❌ Offline |
| Thinker/Translator | 5557 | ❌ Offline (was on n8n) |

---

## CRITICAL PATH — GET THE FORGE WORKING PROPERLY

### Priority 1: Thinker/Translator Must Be Running
**Problem**: The forge's thinker preflight runs (produces search queries + plan) but the main model ignores it and goes into a `<think>` spiral instead of calling tools.

**Root Cause**: The thinker/translator on port 5557 (which was an n8n workflow) is what enforced tool execution. Without it, the model just reasons without acting.

**Fix Options**:
1. Get n8n back up with the translator workflow
2. OR: Build the translator logic directly into forge-v2 (eliminate n8n dependency)
3. OR: Disable Qwen3's thinking mode via `<think>\n\n</think>\n\n` prefill in the assistant turn

### Priority 2: Corrector "Marvin" on Cesarops2
**Problem**: Malformed tool calls can't be fixed because the 14B corrector on cesarops2:5555 is offline.

**Fix**: Start KoboldCPP or cesarops-inference on cesarops2 with a 14B model.

### Priority 3: WSO (Web Search)
**Problem**: `think_harder` tool can't search the web because WSO on port 5010 is down.

**Fix**: Restart WSO or point it at an alternative search backend.

---

## COMPLETED THIS SESSION

1. ✅ **Q6_K dequantization fix** — correct interleaved bit extraction matching llama.cpp
2. ✅ **Attention bias patch** — Q/K/V bias vectors applied after matmul
3. ✅ **Chat template registry** — 6 templates (qwen2.5, qwen3, phi3, deepseek-r1, etc.)
4. ✅ **Special token handling** — tokenizer splits around `<|im_start|>` etc.
5. ✅ **Weight cache** — pre-dequantizes all tensors at load (5-8x CPU speedup)
6. ✅ **GPU matmul via wgpu** — f32 tiled shader, tested correct on P100
7. ✅ **GPU context** — device init, pipeline compile, buffer management
8. ✅ **Forward pass GPU dispatch** — `do_matmul()` routes to GPU when available
9. ✅ **CLI flags** — `--backend cpu|wgpu --gpu N`
10. ✅ **Agent harness** — async tool loop (shell, file, http, scan)
11. ✅ **Sub-agent router** — keyword dispatch to cluster nodes
12. ✅ **Cluster control panel** — web UI at /cluster
13. ✅ **Node discovery** — probes Tailscale IPs for online services
14. ✅ **KoboldCPP fallback** — unsupported quant formats auto-route to koboldcpp
15. ✅ **Forge timeout fix** — 600s instead of 120s

---

## NEXT SESSION PRIORITIES

### 1. GPU Weight Pre-Upload (Speed Fix)
**What**: Upload all model weights to VRAM once at load time. Currently each matmul re-uploads the weight tensor (890MB for lm_head = slow).

**Where**: `src/weight_cache.rs` → `GpuWeightCache::upload()` already written. Need to wire `matmul_with_cached_weight()` into the transformer's `do_matmul()`.

**Impact**: Turns 15 sec/token GPU → ~0.2-0.5 sec/token GPU.

### 2. Fix the Translator/Tool Enforcement
**What**: The model needs to be forced to use tools instead of just thinking about them.

**Options**:
- Restart n8n with the translator workflow
- OR build a simple "tool enforcer" in forge-v2 that detects when the model should be calling a tool and injects a nudge
- OR disable thinking mode for Qwen3.6 via prefill

### 3. Bring Corrector Online
**What**: Get cesarops2 (1070) serving a 14B model for tool call correction.

**How**: SSH to cesarops2, start koboldcpp with a 14B Q6_K model on port 5555.

### 4. Bring WSO Online
**What**: Web search capability for the `think_harder` tool.

**How**: Restart whatever was serving on port 5010, or replace with a simple searxng/brave API proxy.

---

## MEDIUM TERM (Next Week)

### 5. Multi-GPU Layer Split (Cake-style)
**What**: Split large models across multiple GPUs on the same machine (layers 0-13 on GPU 0, 14-27 on GPU 1).

**Where**: `src/transformer.rs` forward pass needs to switch GPU context mid-pass.

### 6. Distributed Inference Across Machines
**What**: Split a model across T440 + cesarops2 + cesarops3 over Tailnet.

**Where**: New module — hidden state transfer over HTTP between nodes mid-forward-pass.

### 7. M10 Integration
**What**: Add 3x Tesla M10 cards (12 GPUs total, 8GB each, Maxwell).

**Impact**: 12 specialist workers running 7B Q4 models simultaneously.

### 8. Dynamic Model Discovery
**What**: Worker bee that queries arxiv/HuggingFace for unknown quant formats and auto-generates dequant code.

---

## LONG TERM (This Month)

### 9. Full Wreck Hunting Pipeline
**What**: Point the cluster at Lake Michigan/Huron sonar/satellite data and run the triple-lock detection pipeline.

**Components needed**:
- Nautivecs loaded with bathymetric/sonar data
- satellite_stitch running on a GPU worker
- optical_mass running on a GPU worker
- galvanic_battery running on a GPU worker
- Detection pipeline (cesarops-detection) orchestrating the triple-lock

### 10. Overnight Research Automation
**What**: Cron-scheduled agent tasks that run overnight (scan regions, analyze data, write reports).

**Where**: `cesarops-agent/workflows.toml` + the AgentTask node we built.

---

## ARCHITECTURE DIAGRAM (Current)

```
[You / Browser]
      │
      ▼
[Forge-v2 Web UI :9100]
      │
      ├── /send → loop_engine.rs
      │     ├── Thinker preflight → :5001 (KoboldCPP)
      │     ├── Main generation → :5001 (KoboldCPP)
      │     ├── Tool execution → tools.rs
      │     │     ├── think_harder → nautivecs :5003 + WSO :5010
      │     │     ├── read_file / write_file → local filesystem
      │     │     ├── cargo_check → local cargo
      │     │     └── run_command → local bash (K-lined)
      │     ├── Diagnosis → :5001 (was :5557 thinker)
      │     └── Correction → :5555 (14B on cesarops2, offline)
      │
      └── /cluster → cluster_panel.html
            ├── Config read/write → cluster_config.toml
            ├── Start/stop workers → spawn cesarops-inference or koboldcpp
            └── Discover nodes → probe Tailscale IPs

[Hardware]
├── T440: 2x Xeon 4110, 2x P100 16GB, 94GB RAM
├── Cesarops2: GTX 1070 8GB (100.102.158.111)
├── Cesarops3: GTX 1060 6GB (100.105.77.74)
└── Nautik9 Laptop: Maxwell 4GB (100.110.214.86)
```

---

## FILES MODIFIED THIS SESSION

### cesarops-inference/src/
- `bridge.rs` — Fixed Q6_K dequant (interleaved layout)
- `transformer.rs` — Added bias, weight cache, GPU dispatch via do_matmul()
- `server.rs` — Chat template, weight cache init, GPU context wiring
- `tokenizer.rs` — Special token handling for ChatML
- `chat_template.rs` — NEW: 6-model template registry
- `weight_cache.rs` — NEW: Pre-dequant + GPU upload
- `gpu_context.rs` — NEW: wgpu device init + matmul dispatch
- `wgpu_uniform.rs` — NEW: MatrixDimensions struct
- `backend_wgpu.rs` — Rewritten: real GPU + CPU fallback
- `main.rs` — Added --backend and --gpu CLI flags
- `lib.rs` — Registered new modules
- `Cargo.toml` — Added half/bytemuck features

### cesarops-forge-v2/src/
- `main.rs` — Added cluster panel, discovery, worker start/stop, KoboldCPP fallback
- `loop_engine.rs` — Bumped timeout to 600s
- `cluster_panel.html` — NEW: Full cluster control UI
- `cluster_config.toml` — NEW: Cluster topology config

### cesarops-agent/src/
- `harness.rs` — NEW: Async agent loop with 5 tools
- `router.rs` — NEW: Sub-agent keyword router
- `nodes.rs` — Added AgentTask + RouteToSubAgent variants
- `lib.rs` — NEW: Module registry
- `workflow.rs` — Fixed NodeId conflict
- `dashboard.rs` — Fixed axum 0.7 handler signatures
- `main.rs` — Fixed axum::Server → axum::serve
- `Cargo.toml` — Added uuid/serde feature

### New files:
- `GEMINI_WGPU_SPEC.md` — Spec for Gemini (GPU dispatch)
- `IMPLEMENTATION_ROADMAP.md` — This file
- `start_forge.sh` — Launcher script
- `shaders/matmul_f32.wgsl` — Tiled f32 compute shader (Naga-safe)
