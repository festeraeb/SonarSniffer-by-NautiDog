# Fleet Baseline Snapshot

Captured: 2026-05-24 (Phase 0 — read-only, no service restarts).

## Forge

| Item | Value |
|------|-------|
| Health | `GET http://127.0.0.1:9100/health` → `status: ok`, mode `self-healing-translator` |
| PID | `cesarops-forge-v2` on `:9100` |
| Missions | `GET /webhook/missions` → `[]` (no active webhook missions at snapshot time) |

## GPU (T440 local)

| Index | Name | VRAM used / total | Util |
|-------|------|-------------------|------|
| 0 | Tesla P100-PCIE-16GB | 15230 / 16384 MiB | 0% |
| 1 | Tesla P100-PCIE-16GB | 7647 / 16384 MiB | 0% |

## Listening ports (T440)

| Port | Process | Notes |
|------|---------|-------|
| 5001 | `llama-server` (Gemma-4-26B-MoE) | **Also** stray `koboldcpp` process with `--port 5001` in process list — kill Kobold only |
| 5002 | `llama-server` (Qwen MTP reviewer) | OK |
| 9100 | `cesarops-forge-v2` | OK |
| 5003 | nautivecs | Not verified at snapshot (curl failed) |
| 5678 | n8n | Not listening at snapshot |

## Config reference

- Cluster: [`cesarops-forge-v2/cluster_config.toml`](../cesarops-forge-v2/cluster_config.toml)
- P100 lifecycle: [`scripts/p100_cycle.sh`](../scripts/p100_cycle.sh)
- Deployment: [`FORGE_DEPLOYMENT.md`](../FORGE_DEPLOYMENT.md)

## Fleet endpoints (planned normal mode)

| Role | Endpoint |
|------|----------|
| Coder | `http://127.0.0.1:5001` |
| Reviewer | `http://127.0.0.1:5002` |
| Thinker | `http://10.0.0.201:5200` |
| Draft | `http://10.0.0.201:5571` |
| Intake sentinel | `http://10.0.0.201:5599` |
| n8n tool route | `http://127.0.0.1:5678/webhook/tool-route` |
| n8n PAMP | `http://127.0.0.1:5678/webhook/pamp-route` |

## Action items (post-snapshot)

1. Phase 1: `pkill` **koboldcpp only** (verify cmdline); keep llama on 5001/5002.
2. Phase 2: Install Cake scripts disabled until pipeline complete.
3. Phase 4+: Forge rebuild for baselines / PAMP — gated on user pipeline complete.
