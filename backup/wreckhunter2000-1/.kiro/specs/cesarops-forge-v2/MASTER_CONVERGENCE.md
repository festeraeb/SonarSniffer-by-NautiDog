# Master Convergence Recap — For New Session Ingestion

## Status: SHKT Built & Deployed, Sonar Sniffer Test In Progress

The 7-file SHKT (Self-Healing Knowledge Translator) is **ALREADY BUILT AND RUNNING** on the T440.
Do NOT rebuild it from scratch. It compiles clean and is live on port 9100.

---

## 🛠️ Mission Recap & Global Context

We are building CESARops Forge, an autonomous maritime SAR (Search and Rescue) platform.
The immediate objective is finding the sailboat **Rossa**.

- **The Iron**: A "Sleeper" Dell T440 workstation. 2x Tesla P100 (16GB HBM2), 2x Xeon 4110 (AVX-512), 94GB RAM, RAID storage.
- **The Logic**: Moving from "Text Generation" to a Tool-Calling Agentic Loop.
- **The Pivot**: Replacing KoboldCPP with a native Rust Burn/WGPU inference engine to achieve zero-copy memory between LLM "thinking" and SAR "scanning."

---

## 🧠 The Universal Translator (SHKT) — ALREADY BUILT

### Location: `/codebase/wreckhunter2000-1/cesarops-forge-v2/`

### Current Status:
- ✅ Compiles clean (release build on T440)
- ✅ Running on port 9100 (PID active)
- ✅ Health check passes
- ✅ First test showed self-healing working (caught MalformedToolCall, 8B diagnosed it)
- ⚠️ Running via nohup (needs systemd service for persistence)
- ⚠️ Translator patched with 3-tier JSON parser for Qwen's malformed output

### The 7-File Structure (all implemented):
- `main.rs`: Axum server, routes, state management
- `translator.rs`: Normalizes all model dialects, 3-tier JSON parser, loop detection
- `diagnostics.rs`: 8B Scout on cesarops2:5555, prior fix search via nautivecs
- `tools.rs`: write_file, read_file, cargo_check, think_harder, remember, run_command
- `memory.rs`: Auto-remember successful fixes to nautivecs + local log
- `hardware.rs`: nvidia-smi metrics, register pressure, AVX-512 awareness
- `loop_engine.rs`: Main Strategy→Execution→Verification loop (12 rounds, 2 diagnosis max)
- `prompts.rs`: QwenChatML formatting, /no_think, snark escalation
- `index.html`: Web frontend with tool badges + diagnosis display

### The "Nudge & Healing" Logic (implemented):
1. **Detection**: If 35B returns only `<think>`, Translator triggers 8B Diagnostic
2. **Correction**: 8B analyzes failure, rewrites System Prompt Override
3. **Injection**: Fix injected using QwenChatML format as user message
4. **Memory**: Successful fixes auto-saved to nautivecs for future retrieval

---

## 🎯 Immediate Task: Complete the Sonar Sniffer Audit

The test prompt:
> "Audit the Sonar Sniffer at /codebase/projects/cesarops/rust/sonarsniffer/ - read the source files, tell me what is working, what is not, and suggest prioritized improvements."

**Success criteria**: Actual assessment with file contents and prioritized fix list (not think-only or empty).

The first test showed the SHKT catching a malformed tool call and self-healing. The patched translator (3-tier JSON parser) is deployed. Need to confirm end-to-end success.

---

## 🚀 The "Burn/Cake" Inference Masterpiece (NEXT PHASE)

Once the SHKT proves stable on the Sonar Sniffer audit, proceed to native inference.

### Objective
Replace KoboldCPP with a native Rust inference engine integrated into the warp-grid unified memory fabric.

### 1. Distributed KV "Cake" Protocol (Infinite Context)
- **Tier 0 (HBM2 - P100s)**: Active Attention window. FP16 for 2:1 throughput.
- **Tier 1 (GDDR5 - 1070/P106)**: Warm Memory. KV pages for last 50-100 tiles.
- **Tier 2 (DDR4 - Xeon)**: Cold Archive. Full temporal history (20+ days).
- **Bridge**: QUIC (quinn) to pull KV pages from 1070 to P100 in real-time.

### 2. P100 HBM2 Tweaks
- `matmul_half2.wgsl`: Force native FP16 mode (19.05 TFLOPS)
- Kernel Fusion (Burn-Wgpu): Fuse Attention + RoPE into single GPU pass
- Zero-Init Weight Mapping: pmetal-gguf or memmap2 (mmap, don't load)

### 3. Unified "Zero-Copy" Fabric
- Shared `wgpu::Device` between Inference Engine and SAR Pipeline
- Zero PCIe Hop: GridBuffer hands HBM2 pointer directly to matmul kernel
- "Thinking" and "Scanning" happen in the same silicon

### 4. Logic-Level Suppression (The "Think" Killer)
- Logit Filtering: Set `<think>`/`</think>` token probability to -infinity
- Grammar-Constrained Sampler: Force valid JSON output for tool calls

### Spec Location: `.kiro/specs/cesarops-inference/`

---

## 📍 Cluster Topology

| Node | IP (Tailscale) | Hardware | Role |
|------|---------------|----------|------|
| T440 | 100.72.182.77 | 2x P100, 2x Xeon 4110, 94GB | Main inference + forge |
| cesarops2 | 100.102.158.111 | 1070 | 8B thinker/diagnostic (DeepSeek-R1) |
| cesarops3 | 100.105.77.74 | P106 | Future Cake KV shard |

### Services on T440:
| Port | Service | Status |
|------|---------|--------|
| 5001 | KoboldCPP (Qwen3.6-35B-A3B MoE) | ✅ Running |
| 5003 | nautivecs-server (12,606 chunks) | ✅ Running |
| 5010 | cesarops-wso (web search) | ✅ Running |
| 8099 | wrecks-api | ✅ Running |
| 9100 | cesarops-forge-v2 (SHKT) | ✅ Running (nohup) |

### RAID Layout:
| Mount | Size | Content |
|-------|------|---------|
| `/codebase` (sdb1) | 465GB | Repos, projects, models |
| `/data` (sdb2) | 1.8TB | Large data storage |
| `/shared_drive` (sda2) | 916GB | Shared/synced content |

---

## 📋 Next Session Priority Order

1. **Deep dive codebase audit** — catalog every crate, script, service
2. **Create systemd service** for forge-v2 (survive reboots)
3. **Restart nautivecs-server** to pick up new chunks (12,674 on disk)
4. **Re-run sonar sniffer test** with patched translator
5. **If SHKT passes** → begin cesarops-inference (Burn/WGPU) build
6. **If SHKT fails** → iterate on translator/diagnostics

---

## 🔑 Key Technical Decisions (Locked In)

- Tool results go as `<|im_start|>user` messages (NOT custom XML tags)
- `/no_think` added to system prompt to disable Qwen3.6 thinking mode
- Pre-fill `<think>\n</think>\n` before assistant turn as backup
- 8B has Instructional Authority (rewrites prompts) but NOT System Authority
- Translator is the single source of truth for conversation state
- Every successful diagnosis → auto-remember to nautivecs
- Max 12 tool rounds, max 2 diagnosis attempts, then hard reset
- Storage: nautivecs v0.1.0 uses serverless JSON store (NOT LanceDB/Arrow)
- Hardware: Dual P100 GPUs running custom wgpu 29.x and WGSL shaders

---

## 🔐 Access

- T440: `ssh cesarops@100.72.182.77` (password: cesarops)
- cesarops2: `ssh cesarops@100.102.158.111`
- cesarops3: `ssh cesarops@100.105.77.74`
- Cargo on T440: `source ~/.cargo/env` before running cargo commands
- sudo on T440: password is `cesarops`
