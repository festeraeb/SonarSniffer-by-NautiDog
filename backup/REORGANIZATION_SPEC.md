# WreckHunter 2000 Reorganization Specification

## Overview
This document describes the reorganization of the WreckHunter 2000 backup source tree into a clean deployment structure.

## Source Tree (`backup/src/`)
The raw backup contains 270 files across multiple modules:
- **Rust workspace crates**: cesarops-inference, cesarops-agent, cesarops-detection, cesarops-forge-v2, cesarops-forge, cesarops-forge-web, cesarops-aeromagnetic-worker, cesarops-supervisor, cesarops-watchdog, cesarops-wso, cesarops-wso-server, cesarops-thought-engine, cesarops-mcp-steered, cesarops-mission-control
- **Python services**: mission_control.py, research_agent.py, scan_queue.py, scan_worker.py, universal_downloader.py, watchdog.py, weather_service.py
- **Config files**: ai_stack_docker-compose.yml, n8n_moe_tool_router.json
- **Scripts**: start_forge.sh, warp-grid, warp-grid-standalone
- **Web/UI**: tauri, tauri-mission-control
- **Docs**: docs/, IMPLEMENTATION_ROADMAP.md, INVENTORY.md, GEMINI_WGPU_SPEC.md, SCM_RUN7_ANALYSIS.txt, SONAR_SNIFFER_ANALYSIS.md, research_log/
- **Other**: db/, searxng/, sovereign-cloud/, model-team-tool/, wrecks_api/, nauticuvs/, nautivecs/, thought-engine/, src/

## Deploy Structure (`backup/deploy/`)
```
deploy/
├── inference/          ← cesarops-inference (LLM GPU dispatch engine with wgpu)
├── agent/              ← cesarops-agent (task orchestration & agent harness)
├── detection/          ← cesarops-detection (wreck detection pipeline)
├── forge/              ← cesarops-forge-v2 (web UI & tool execution)
├── tools/              ← Specialized tools: aeromagnetic_worker, etc.
├── supervisor/         ← cesarops-supervisor (cluster management)
├── watchdog/           ← cesarops-watchdog (health monitoring)
├── wso/                ← cesarops-wso + wso-server (web service orchestrator)
├── mcp_steered/        ← cesarops-mcp-steered (MCP protocol steering)
├── mission_control/    ← cesarops-mission-control (mission coordination)
├── thought_engine/     ← cesarops-thought-engine (reasoning engine)
├── web/                ← cesarops-forge-web (frontend files)
├── scripts/            ← Shell scripts and utilities
├── docs/               ← Documentation, specs, roadmaps
└── config/             ← Config files (TOML, JSON, YAML)
```

## Copy Decisions
| Source | Destination | Reason |
|--------|-------------|--------|
| cesarops-inference/ | deploy/inference/ | Primary LLM inference engine |
| cesarops-agent/ | deploy/agent/ | Agent orchestration core |
| cesarops-detection/ | deploy/detection/ | Wreck detection pipeline |
| cesarops-forge-v2/ | deploy/forge/ | Current working forge version |
| cesarops-forge/ | (deprecated) | Old forge version, superseded by v2 |
| cesarops-forge-web/ | deploy/web/ | Frontend-only module |
| cesarops-aeromagnetic-worker/ | deploy/tools/aeromagnetic_worker/ | Magnetic anomaly tool |
| cesarops-supervisor/ | deploy/supervisor/ | Cluster supervisor |
| cesarops-watchdog/ | deploy/watchdog/ | Health monitoring |
| cesarops-wso/ + wso-server/ | deploy/wso/ | Web service orchestrator |
| cesarops-mcp-steered/ | deploy/mcp_steered/ | MCP protocol integration |
| cesarops-mission-control/ | deploy/mission_control/ | Mission coordination |
| cesarops-thought-engine/ | deploy/thought_engine/ | Reasoning/thought processing |
| scripts/ | deploy/scripts/ | Shell utilities |
| docs/ | deploy/docs/ | Documentation |
| ai_stack_docker-compose.yml | deploy/config/ | Docker compose config |
| n8n_moe_tool_router.json | deploy/config/ | Tool routing config |
| start_forge.sh | deploy/scripts/ | Forge startup script |
| *.py files | deploy/scripts/ | Python services |

## Duplicate/Old Versions Identified
- `cesarops-forge` → superseded by `cesarops-forge-v2`
- `thought-engine/` (standalone dir) → superseded by `cesarops-thought-engine/`
- `tauri/` and `tauri-mission-control/` → may be superseded by forge-web or mission-control
- `src/` in backup/src/ → likely duplicate of workspace source, needs review
