# Cluster topology — May 2026

Resilient layout: **T440 (2× P100) is primary**; **cesarops2 (ML350e Gen8)** augments when online.  
**cesarops3 is retired.** Nothing hard-fails when a node drops — pools probe and fall back.

## Nodes

| Node | IP | GPUs | Role |
|------|-----|------|------|
| **T440** | `10.0.0.61` / `127.0.0.1` | 2× Tesla P100 16GB | Forge :9100, MoE/Gemma LLM, detection orchestrator :5580, CPU vision fallback |
| **cesarops2** | `10.0.0.201` (eno2) · `10.0.0.200` (eno1) | P106 + RTX 2060 SUPER + GTX 1070 | Remote LLM + vision CPU sim; NFS client → T440 |
| **Nautik9** | Tailscale | M2200 | Mobile optional |

## cesarops2 hardware (live inventory)

| Component | Details |
|-----------|---------|
| Host | HP ProLiant ML350e Gen8 |
| CPU | 2× Xeon E5-2440, 12C/24T |
| RAM | ~78 GB DDR3 ECC |
| Storage | `/` 1.8TB HDD, `/mnt/data-external` 112GB SSD, `/data` 489GB SSD (all RAID-0) |
| Driver | NVIDIA 580.159.03, CUDA 13.0 |
| USB | Intel Movidius NCS (MA2X5X) — candidate for jitter lock later |
| NFS | 5 mounts from T440 `10.0.0.61` — all working |

### GPU indices — nvidia-smi vs llama.cpp (important)

| nvidia-smi | Model | llama `-dev` | Lab use |
|------------|-------|--------------|---------|
| **GPU 0** | P106-100 | CUDA2 | Free / optional Scout LLM |
| **GPU 1** | RTX 2060 SUPER | **CUDA0** | `:5200` thinker (DeepSeek R1 7B) |
| **GPU 2** | GTX 1070 | **CUDA1** | `:5571` draft (Phi-3 mini) |

llama.cpp reorders devices by capability — always use **CUDA0/CUDA1** in launch scripts, not nvidia-smi index.

Vision triple-lock uses **CPU sim** on `:5570`, `:5572`, `:8080` so GPUs stay free for LLM.

### Network

| Interface | IP | Notes |
|-----------|-----|-------|
| eno1 | `10.0.0.200/24` | Alternate reachability (same host) |
| eno2 | `10.0.0.201/24` | **Canonical** in cluster_config |

Forge/detection pools try **both** `.201` and `.200` before T440 fallback.

## cesarops2 port map

| Port | Service | GPU |
|------|---------|-----|
| **5200** | Thinker / coder (DeepSeek R1 7B, MTP flags) | RTX 2060 SUPER (`CUDA0`) |
| **5571** | MTP reviewer / draft (`Qwen3.5-*-MTP`, `--spec-type draft-mtp`) | GTX 1070 (`CUDA1`) |
| **5570** | Scout vision (CPU sim) | CPU |
| **5572** | Validator vision (CPU sim) | CPU |
| **8080** | Jitter (CPU sim) | CPU |
| **5580** | Detection orchestrator (optional local) | CPU |

Start on cesarops2:

```bash
bash /mnt/t440/codebase/repos/wreckhunter2000-1/scripts/cesarops2_research_lab.sh start
```

## Triple-lock failover (detection)

| Lock | Try (in order) | Last resort |
|------|----------------|-------------|
| Scout | `.201:5570`, `.200:5570` | T440 `:5570` CPU sim |
| Validator | `.201:5572`, `.200:5572`, `:5571` | T440 `:5572` |
| Jitter | `.201:8080`, `.200:8080` | T440 `:8080` |

## Forge LLM routing

| Role | Primary | Fallback |
|------|---------|----------|
| Coder | T440 `:5001` (P100 llama-server) | — |
| Reviewer (MTP) | cesarops2 `:5571` (1070) | `:5200` → T440 `:5002` → `:5001` |
| Thinker | cesarops2 `:5200` (2060) | T440 `:5002` |
| Draft / intake | cesarops2 `:5571` (MTP) | T440 `:5002` / `:5001` |

MTP pool: `[endpoint_pool.mtp]` in `cluster_config.toml` — forge `/cluster/agent/run` with `endpoint=mtp` or `role=reviewer` probes this list.

## When cesarops2 is offline

1. Detection uses T440 CPU sim workers (auto-started by `start.sh`).
2. Forge keeps coding on P100s; thinker/draft routes fall back locally.
3. Satellite/mag pipelines continue on T440 + NFS.

## Ops notes (from health check)

- SMBIOS reports past **firmware hardware failure** — review iLO event log when convenient.
- All volumes are **RAID-0** (no redundancy); consider `ssacli` for array health.
- Mixed 4 GB + 8 GB DIMMs — works but not ideal for bandwidth.

## Mission orchestrator (forge :9100)

Sequential satellite pipeline (spec JSON → tools → detection):

| Endpoint | Purpose |
|----------|---------|
| `POST /orchestrator/plan` | Heuristic plan from scenario text or `spec_path` |
| `POST /orchestrator/execute` | Run plan synchronously (full report) |
| `POST /webhook/satellite` | Async mission + `mission_id` (straits validation default) |
| `GET /webhook/missions/{id}` | Poll status; `report.review` = MTP polish |

Module chain for `spec_path` missions: `weather_window` → `sat_mission` (2h timeout) → `sat_read_mission_report` → `detection_health` → `detection_scan` (tiles from `wreck_targets_all.csv`) → `detection_poll`.

```bash
ORCHESTRATOR=1 DRY_RUN=true bash scripts/resume_satellite_pipeline.sh
```

Env: `FORGE_URL`, `DETECTION_URL` (default `http://10.0.0.201:5580`), `CESAROPS_PROJECT_ROOT`.

## Vision + MCP wiring

| Component | Start | Env |
|-----------|-------|-----|
| Vision triple-lock | `scripts/start_vision_workers.sh` | `VISION_MODE=cpu\|gpu\|yolo` |
| MCP tool worker | `cesarops-mcp-worker :8090` | `MCP_WORKER_URL`, `MCP_DELEGATE_TOOLS=1` |
| Forge delegates tools | `/cluster/agent/run` + `/tool/*` | Routes `think_harder`, `remember`, `read_file`, `write_file`, `cargo_check`, `run_command` to MCP when URL set |

YOLO11 scout: `VISION_MODE=yolo` — `scout_yolo11.py` on `:5570`. Movidius jitter: `jitter_movidius.py` on `:8080` (OpenVINO when model present).

## Config files

- `cesarops-forge-v2/cluster_config.toml` — workers, agents, endpoint pools
- `cesarops-detection/scripts/start.sh` — T440 resilient detection start
- `scripts/cesarops2_research_lab.sh` — full cesarops2 stack
