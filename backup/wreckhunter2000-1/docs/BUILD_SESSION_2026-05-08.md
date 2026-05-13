# CESAROPS Build Session — May 8-9, 2026

## What We Built in One Session

This document records a single continuous build session where we took a broken cluster (NVIDIA driver mismatch, services down, stale routing) and turned it into a self-authoring, self-deploying AI system running entirely on repurposed enterprise hardware.

---

## Starting State

- T440 (dual P100 32GB): SSH unresponsive, NVIDIA 595.71 driver incompatible with Pascal GPUs, KoboldCPP dead
- cesarops2 (1070 8GB): Running a 3B model, underutilized
- Cloudflare tunnel: Pointing at retired i7 server
- Frontend: Stale, pointing at `home.cesarops.com:8099` (dead)
- nautivecs: Library only, no HTTP server, no indexed data
- Web search: Nonexistent

## Ending State

| Service | Port | Status |
|---------|------|--------|
| **Mission Control** (Web UI) | 3000 | ✅ Live — 3 buttons: CODE/SCAN/RESEARCH |
| **KoboldCPP** (Qwen3.6-35B-A3B) | 5001 | ✅ 32K context, flash attention |
| **nautivecs** (search API) | 5003 | ✅ 2272 chunks indexed, OpenAI-compatible |
| **sovereign-cloud** (cluster API) | 8765 | ✅ Node coordination |
| **Cloudflare tunnel** | 443 | ✅ All services via cesarops.org |
| **NVIDIA 580.159.03** | — | ✅ Both P100s healthy, pinned |

---

## What Was Built (Chronological)

### 1. Hardware Recovery
- Diagnosed NVIDIA driver mismatch (595.71 dropped Pascal support)
- Installed nvidia-driver-580 (Pascal legacy branch)
- Pinned driver to prevent future auto-upgrades
- Mounted 916GB external drive, moved models/caches off root
- Attempted Coral TPU fix (gasket incompatible with kernel 6.17 — needs source patch)

### 2. KoboldCPP Deployment
- Wrote new systemd service pointing at external drive model path
- Configured 32K context window with flash attention
- Verified Qwen3.6-35B-A3B MXFP4 MoE loads across both P100s (~20GB)
- Model serving at ~15-20 tok/s with full 32K context

### 3. nautivecs HTTP Server
- Added `serve` command to nautivecs-cli (axum on port 5003)
- Endpoints: `POST /query`, `POST /v1/search`, `GET /health`, `GET /stats`
- CORS enabled for frontend access
- Indexed 2272 code chunks from the entire workspace
- Deployed as systemd service (auto-starts on boot)

### 4. Infrastructure Updates
- Updated Cloudflare tunnel routing (all services → T440)
- Replaced all `home.cesarops.com:8099` references with `api.cesarops.org`
- Updated `deploy_web.py` (T440 primary, cesarops3 backup)
- Consolidated credentials into `scripts/credentials.sh`
- Deployed frontend to IONOS hosting
- Added SSH-over-Cloudflare-tunnel design for school access

### 5. Self-Authored Whitepaper (The Model Writing About Itself)
- Full pipeline: nautivecs context injection → KoboldCPP generation
- 62KB of real source code injected as grounding context
- Model produced 17KB whitepaper referencing actual code (`AllocationEngine`, `SyntheticTile`, `ResearchEngine`)
- Graded B+ — good structure, some hallucination on storage backend details

### 6. Web Search Oracle (cesarops-wso)
- Model wrote its own spec (graded A-)
- Model implemented the full Rust crate (8 files)
- Includes: SearXNG client, Google CSE, DuckDuckGo HTML scraper, Brave API, caching, token budget
- First build: 11 errors → model self-fixed to 3 → I fixed last 1
- **Compiles clean in release mode**

### 7. Thought Engine (cesarops-thought-engine)
- Architecture: 8B model on 1070 THINKS → dispatches to 35B on P100s for EXECUTION
- Model wrote Python prototype, then rewrote in Rust
- 6 source files: main.rs, clients.rs, engine.rs, models.rs, handlers.rs
- First build: 19 errors → model self-fixed to 8 → I fixed remaining
- **Compiles clean in release mode**

### 8. Rust Codegen Corrections (Steering)
- Created `.kiro/steering/rust-codegen-corrections.md`
- Documents 7 recurring LLM Rust errors with WRONG/RIGHT examples
- Indexed into nautivecs — future code generation will pull these as context
- Covers: trait object safety, borrow-after-move, string types, scraper tendrils, numeric ambiguity, match arms, collect inference

### 9. Mission Control (cesarops-mission-control)
- Model designed the full spec (17KB architecture document)
- Model implemented it as a Rust crate (axum server + embedded HTML)
- 3 big buttons: CODE / SCAN / RESEARCH
- Voice input via browser SpeechRecognition API
- WebSocket progress streaming
- Dyslexia-friendly fonts, large targets, mobile-responsive
- **Deployed and running on port 3000**

---

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                    CLOUDFLARE TUNNEL                              │
│  app.cesarops.org → :3000  (Mission Control)                     │
│  llm.cesarops.org → :5001  (KoboldCPP / Qwen3.6-35B)           │
│  api.cesarops.org → :8099  (wrecks-api)                          │
│  search.cesarops.org → :5003 (nautivecs)                         │
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────────────┐
│                    T440 (Dual P100, 32GB HBM2)                   │
│                                                                   │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ Mission      │  │ nautivecs    │  │ KoboldCPP            │  │
│  │ Control      │→ │ (2272 chunks)│→ │ Qwen3.6-35B-A3B      │  │
│  │ :3000        │  │ :5003        │  │ :5001 (32K ctx)      │  │
│  └──────────────┘  └──────────────┘  └──────────────────────┘  │
│                                                                   │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ sovereign-   │  │ cesarops-wso │  │ cesarops-thought-    │  │
│  │ cloud :8765  │  │ (compiled)   │  │ engine (compiled)    │  │
│  └──────────────┘  └──────────────┘  └──────────────────────┘  │
│                                                                   │
│  Storage: /mnt/data-external (916GB SSD)                         │
│  Models: Qwen3.6-35B-A3B-MXFP4_MOE.gguf (20GB)                 │
│  Driver: NVIDIA 580.159.03 (Pascal legacy, pinned)               │
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────────────┐
│  cesarops2 (1070 8GB) — Thought Engine target                    │
│  cesarops3 (1060 6GB) — Backup frontend                          │
│  Pi — Health sentinel, DDNS                                      │
└─────────────────────────────────────────────────────────────────┘
```

---

## Key Innovations

### 1. Self-Authoring AI
The 35B model wrote its own web search oracle, thought engine, and mission control interface. It designed specs, implemented Rust code, and fixed most of its own compiler errors. The remaining fixes were mechanical (1-3 errors per crate).

### 2. nautivecs as Persistent Grounding
By indexing the entire codebase (2272 chunks) and injecting relevant context into every LLM call, the model references real code instead of hallucinating. The steering corrections file means it learns from its own mistakes.

### 3. Distributed Reasoning Architecture
The thought engine splits cognition: fast 8B model on cheap hardware for planning/searching, powerful 35B on P100s for execution. This is the "Librarian Pattern" — small model + tools > large model alone.

### 4. Zero-Cloud Sovereignty
Everything runs on ~$500 of used enterprise hardware. No API keys needed for core functionality. Accessible from anywhere via Cloudflare tunnels (free tier). The system can operate fully air-gapped if needed.

### 5. Accessibility-First Design
Mission Control was designed for a dyslexic operator: large buttons, voice input, visual progress, no code walls. The system handles complexity internally and presents simple choices externally.

---

## What's Next

- [ ] Deploy Qwen3-8B on cesarops2 for the thought engine
- [ ] Wire Mission Control to actually call the full pipeline (currently serves UI + mock responses)
- [ ] Add `search.cesarops.org` and `ssh.cesarops.org` to Cloudflare dashboard
- [ ] 4TB RAID array (arriving tomorrow) for persistent scan archive
- [ ] Fix Coral TPU gasket driver (needs kernel 6.17 compatible source build)
- [ ] Tauri desktop wrapper for Windows local access
- [ ] Deploy Mission Control HTML to IONOS hosting

---

## Files Created This Session

### New Crates
- `cesarops-wso/` — Web Search Oracle (DuckDuckGo, Brave, SearXNG, caching)
- `cesarops-thought-engine/` — Distributed reasoning (8B plans → 35B executes)
- `cesarops-mission-control/` — Web UI with 3-mode interface

### Services
- `scripts/koboldcpp_580.service` — KoboldCPP with 32K context
- `scripts/nautivecs.service` — Boot-time indexing
- `scripts/nautivecs-server.service` — HTTP search API
- `scripts/setup_t440_services.sh` — Full T440 service setup

### Documentation
- `docs/whitepaper_cesarops_self_authored.md` — AI-written whitepaper
- `docs/spec_web_oracle.md` — WSO architecture spec
- `docs/mission_control_spec.md` — Mission Control architecture
- `.kiro/steering/rust-codegen-corrections.md` — LLM Rust error patterns

### Infrastructure
- `scripts/cloudflared_config.yml` — Tunnel routing
- `scripts/deploy_tunnel_t440.sh` — Tunnel deployment
- `scripts/setup_cloudflare_ssh.sh` — SSH over tunnel
- `scripts/fix_t440_post_reboot.sh` — Driver + storage fix

---

*Session duration: ~4 hours. Lines of Rust generated by the AI: ~1500. Compiler errors fixed: ~40 (AI fixed ~30, human fixed ~10).*
