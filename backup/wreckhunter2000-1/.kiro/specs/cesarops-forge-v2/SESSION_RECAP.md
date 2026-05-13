# Session Recap — cesarops-forge-v2 Build & Deploy

## Date: 2026-05-10/11

## What We Accomplished

### 1. Built cesarops-forge-v2 (Complete 7-file Rust crate)
All modules implemented from the self-healing translator spec:

| File | Lines | Purpose |
|------|-------|---------|
| `src/main.rs` | ~90 | Axum server, routes (/send, /health, /clear, /monitor), state |
| `src/translator.rs` | ~210 | Normalize output, extract tool calls (3-tier JSON parser), loop detection |
| `src/diagnostics.rs` | ~130 | 8B psychiatrist on cesarops2:5555, prior fix search via nautivecs |
| `src/tools.rs` | ~180 | write_file, read_file, cargo_check, think_harder, remember, run_command |
| `src/memory.rs` | ~80 | Auto-remember successful fixes to nautivecs + local log |
| `src/hardware.rs` | ~140 | nvidia-smi metrics, register pressure, AVX-512 awareness |
| `src/loop_engine.rs` | ~230 | Main Strategy→Execution→Verification loop (12 rounds, 2 diagnosis max) |
| `src/prompts.rs` | ~90 | QwenChatML formatting, /no_think, snark escalation, thinker prompt |
| `src/index.html` | ~150 | Web frontend with tool badges + diagnosis display |

### 2. Deployed to T440
- Compiled clean (release) on T440 at `/codebase/wreckhunter2000-1/cesarops-forge-v2/`
- Stopped old `cesarops-forge-web.service`
- Started forge-v2 on port 9100 (PID 895034)
- Health check passes: `{"status":"ok","service":"cesarops-forge-v2","mode":"self-healing-translator"}`

### 3. First Live Test — Sonar Sniffer Audit
- Sent audit prompt from web UI
- **Result: SHKT self-healing WORKS**
  - 35B generated a malformed tool call (Qwen JSON quirk)
  - Translator detected: `MalformedToolCall`
  - 8B diagnosed correctly: "JSON lacks proper structure"
  - Diagnosis displayed in UI with 🔧 badge
  - Model retried with corrected prompt

### 4. Patched Translator for Qwen JSON Quirk
- Added 3-tier fallback JSON parser to handle Qwen's `{"name": "x", {"arguments": {...}}}` pattern
- Pushed patch to T440, rebuilt (4.5s incremental)
- Restarted forge-v2 with fix

### 5. Updated nautivecs Index
- Indexed forge-v2: +68 chunks (total: 12,674)
- Indexed warp-grid-standalone: +74 chunks
- Added forge-v2 lessons to `research_log/lessons_learned.md`

### 6. Workspace Configuration
- Added `cesarops-forge-v2` to workspace `Cargo.toml` members

## What's Running on T440 Right Now
- **KoboldCPP** (:5001) — Qwen3.6-35B-A3B MoE on dual P100s
- **cesarops-forge-v2** (:9100) — Self-Healing Knowledge Translator (our new build)
- **nautivecs-server** (:5003) — 12,606 chunks (disk has 12,674, needs server restart)
- **cesarops-wso-server** — Web Search Oracle
- **wrecks-api** (:8099) — REST API

## Known Issues
- The forge-v2 process was started with `nohup` (not systemd) — will die on reboot
- nautivecs server needs restart to pick up new chunks from disk
- PowerShell → SSH quoting makes it hard to send JSON payloads from this machine (use curl from T440 or browser)
- The 35B still produces malformed JSON sometimes — the 3-tier parser handles most cases but edge cases may exist

## RAID Layout Confirmed
| Mount | Device | Size | Used | Content |
|-------|--------|------|------|---------|
| `/codebase` | `/dev/sdb1` | 465GB | 117GB | Repos, projects, models |
| `/data` | `/dev/sdb2` | 1.8TB | 34GB | Large data storage |
| `/shared_drive` | `/dev/sda2` | 916GB | 134GB | Shared/synced content |

Two copies of the repo:
- `/codebase/wreckhunter2000-1/` — live working copy (services run from here)
- `/codebase/repos/wreckhunter2000-1/` — full mirror

---

## NEXT SESSION: Deep Dive Codebase Audit

The next session should:
1. **Deep dive the entire codebase** — catalog every crate, script, service, and their status
2. **Create a proper systemd service** for forge-v2 (so it survives reboots)
3. **Restart nautivecs-server** to pick up the new 12,674 chunks
4. **Run the sonar sniffer test again** with the patched translator to confirm end-to-end success
5. **Evaluate**: Does the SHKT prove the concept? If yes → proceed to Burn/WGPU native inference. If no → iterate on the translator.
6. **Document the full system map** — what talks to what, on which ports, which node

### Deep Dive Should Cover:
- All Rust crates and their compile status
- All Python scripts and their purpose
- All systemd services and their health
- The nautivecs index coverage (what's indexed, what's missing)
- The MCP tools and their integration points
- The warp-grid shaders and their deployment status
- The sonar sniffer / soundtiles project structure
