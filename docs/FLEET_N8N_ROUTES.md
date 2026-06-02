# Fleet n8n routes and cesarops2 recovery

> **Unified entry:** see [FLEET_UNIFIED.md](./FLEET_UNIFIED.md) — `bash scripts/fleet status|recover|import-n8n`.

## T440 n8n webhooks (primary)

| Route | URL |
|-------|-----|
| Fleet ops (all recovery actions) | `http://10.0.0.61:5678/webhook/fleet-ops` |
| Tool route (MoE) | `http://10.0.0.61:5678/webhook/tool-route` |
| LLM output translator (MXFP4 MoE) | `http://10.0.0.61:5678/webhook/llm-translate` |

**Translator behavior** (shared with `tool-route`): merges empty `content` + `reasoning_content`, strips thinking preamble/leaks, extracts `<tool_call>` JSON. Library: `scripts/n8n_llm_translator_normalize.js` (embedded in workflow JSON).

```bash
curl -s -X POST http://127.0.0.1:5678/webhook/llm-translate \
  -H 'Content-Type: application/json' \
  -d '{"choices":[{"message":{"content":"","reasoning_content":"<tool_call>{\"name\":\"think_harder\",\"arguments\":{\"query\":\"wreck\"}}</tool_call>"}}]}'
```
| Prompt tuner (initial) | `http://10.0.0.61:5678/webhook/prompt-tuner-initial` |
| Prompt tuner (failed) | `http://10.0.0.61:5678/webhook/prompt-tuner-failed` |

## Scheduled workflows (auto-active after import)

| Workflow | Interval | Purpose |
|----------|----------|---------|
| Forge Health (stall vs broken) | 2 min | Probe Forge; clear stall or full recovery |
| Fleet Route Health | 3 min | LLM endpoint + webhook probe |
| Fleet Ops Dispatch | webhook | Execute fleet job actions |

Import + activate:

```bash
bash /codebase/repos/wreckhunter2000-1/scripts/import_n8n_health_workflows.sh
```

## Shared NFS job queue (visible on cesarops2)

After `setup_cesarops2_fleet_recovery.sh`:

| Path on cesarops2 | Purpose |
|-------------------|---------|
| `/mnt/t440/repo/var/fleet-jobs/pending/t440/` | Jobs for T440 (enqueue when T440 n8n down) |
| `/mnt/t440/repo/var/fleet-jobs/pending/cesarops2/` | Jobs for cesarops2 |
| `/mnt/t440/repo/var/fleet-jobs/done/` | Completed job logs |
| `/mnt/t440/repo/var/fleet-jobs/failed/` | Failed job logs |

Permissions: `chmod -R ugo+rwX var/fleet-jobs` on T440 repo (done by import script).

## When T440 is broken — fix from cesarops2

```bash
# One-time on cesarops2
T440_IP=10.0.0.61 bash /mnt/t440/repo/scripts/setup_cesarops2_fleet_recovery.sh

# Enqueue recovery (no n8n needed)
FLEET_DISPATCH=nfs bash /mnt/t440/repo/scripts/fleet-n8n-dispatch.sh t440 forge_full_recovery

# Or run queue processor directly
FLEET_NODE=t440 FORGE_URL=http://10.0.0.61:9100 \
  bash /mnt/t440/repo/scripts/fleet-job-runner.sh

# Probe Forge from cesarops2
FORGE_URL=http://10.0.0.61:9100 bash /mnt/t440/repo/scripts/forge-health-probe.sh
```

## Forge / LLM ports

| Service | T440 | cesarops2 |
|---------|------|-----------|
| Forge | `:9100` | — |
| Gemma coder | `:5001` | — |
| R1 thinker | `:5002` | — |
| Qwen MoE reviewer | — | `:5200` |
| R1-7B corrector | — | `:5201` |
