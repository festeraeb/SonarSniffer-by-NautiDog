# cesarops-forge Mode System

## Modes

### 1. `--mode auto` (Autonomous Implementation)
The current loop: reads tasks.md, searches nautivecs, fires at 35B, reviews with 8B, cargo checks, commits. Runs overnight unattended.

### 2. `--mode plan` (Freeform Spitball)
Interactive conversation with the 35B. No structure, no tasks, no cargo check. Just you typing a question/idea and getting a response. Like chatting with me but free.
- Input: stdin (local TUI) OR web interface (phone/remote)
- Output: 35B response printed to terminal or streamed to browser
- Context: nautivecs search results injected automatically based on keywords in your input
- History: last 5 exchanges kept in context window
- Accessible via: terminal locally, or https://forge.cesarops.org from phone

### 3. `--mode spec` (Structured Spec Workflow)  
Guided spec creation: requirements → design → tasks. The 35B acts as spec writer, you provide vision.
- Step 1: You describe what you want
- Step 2: 35B generates requirements with acceptance criteria
- Step 3: You approve/modify
- Step 4: 35B generates design doc
- Step 5: You approve/modify  
- Step 6: 35B generates tasks.md
- Output: .kiro/specs/<name>/ directory with all three files
- Accessible via: terminal or web

### 4. `--mode monitor` (Health Dashboard + Handoff)
Replaces the PowerShell dashboard. Polls all nodes, displays health, makes routing decisions.
- Polls /metrics on all nodes every 10s
- Displays GPU temp, util, VRAM, power, PCIe link status
- Alerts on thermal throttle, voltage sag, PCIe degradation
- Auto-triggers /repin if NUMA imbalance detected
- Web view: live-updating HTML dashboard accessible from phone

### 5. `--mode serve` (Web Frontend — always running)
Axum HTTP server that exposes ALL modes via a web interface. Runs alongside any other mode.
- Port: 9100 (behind Cloudflare tunnel as forge.cesarops.org)
- Endpoints:
  - `GET /` — main UI (big buttons: PLAN / SPEC / MONITOR / AUTO STATUS)
  - `POST /plan/send` — send a message in plan mode, get response
  - `GET /plan/history` — conversation history as JSON
  - `GET /monitor` — live health dashboard (auto-refresh)
  - `GET /auto/status` — current task progress, logs
  - `POST /spec/start` — begin a new spec workflow
  - `POST /spec/approve` — approve current step
  - `GET /spec/current` — current spec state
- UI: embedded HTML (like Mission Control — big buttons, dyslexia-friendly, mobile-responsive)
- WebSocket: `/ws` for live streaming of 35B responses (no page refresh needed)
- Auth: simple bearer token from env var (FORGE_TOKEN) — keeps randos out

## Key Design Decisions

- NO npm, NO Node.js — pure Rust + tokio
- NO cloud API calls — all inference on local hardware (P100s + 1070)
- Conversation history managed locally (JSON file or in-memory ring buffer)
- nautivecs context injection happens automatically in all modes
- The binary is the ONLY orchestrator — no external scripts needed

## What This Replaces

- Kiro (me) for implementation work → auto mode
- Kiro for spec writing → spec mode  
- Kiro for brainstorming → plan mode
- PowerShell health dashboard → monitor mode
- All the fire_*.ps1 scripts → built into the binary
- Mission Control web UI → serve mode (unified)
- Phone access via Cloudflare → forge.cesarops.org

## Access Patterns

| Location | Interface | How |
|----------|-----------|-----|
| At desk (Windows) | Terminal TUI | `cesarops-forge plan` via SSH or local |
| At desk (browser) | Web UI | http://localhost:9100 |
| On phone | Web UI | https://forge.cesarops.org (Cloudflare tunnel) |
| Remote (school/work) | Web UI | https://forge.cesarops.org |
| Overnight (unattended) | Auto mode | systemd service, no UI needed |

## UI Design (Web)

- Dyslexia-friendly: OpenDyslexic font, large text, high contrast
- Mobile-first: works on phone screen
- Big buttons: PLAN / SPEC / MONITOR / STATUS
- Chat interface for plan mode (like iMessage — your messages left, 35B right)
- Live-updating monitor (WebSocket push, no polling from browser)
- Voice input via browser SpeechRecognition API (same as Mission Control)

## Cost

- Electricity: ~$1-2/day running 24/7 (500W cluster)
- Cloud API: $0 (all local inference)
- Cloudflare tunnel: $0 (free tier)
- Your time: spec/plan mode only (the fun parts)
