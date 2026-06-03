# Unified fleet control plane

One manifest and one CLI replace scattered `start_*`, duplicate repo paths, and conflicting Forge/n8n URLs.

## Source of truth

| File | Role |
|------|------|
| `config/fleet_manifest.json` | Host roles, ports, n8n webhooks, pipeline scripts, paths |
| `scripts/fleet` | CLI: `status`, `up`, `down`, `recover`, `import-n8n`, `jobs`, `catalog`, … |
| `scripts/lib/fleet_resolve.sh` | Shared `REPO`, `FLEET_NODE`, `FORGE_URL`, `N8N_URL` |
| `scripts/fleet_manifest.py` | Machine-readable resolver (`env`, `json`, `get`) |

Forge routing and GPU workers remain in `cesarops-forge-v2/cluster_config.toml`. Mission shapes stay in `config/unified_pipeline_profiles.json`. Catalog roots stay in `config/fleet_catalog_roots.json`. The manifest **points at** those files; it does not duplicate them.

## Host roles

| Host | Roles | Start with |
|------|-------|------------|
| **cesarops2** | LLM (RTX `:5200` + 1070 `:5203`), fleet-jobs, operator | `fleet up` |
| **t440** | **Forge primary** `:9100`, n8n, P100 LLM `:5001`/`:5002`, NFS | `fleet up` on T440 |

Forge API (unified): **`http://10.0.0.61:9100`** from c2 ([FORGE_PRIMARY.md](./FORGE_PRIMARY.md)).  
n8n: **`http://10.0.0.61:5678`** (T440 primary).  
Repo tree: NFS `10.0.0.61:/codebase/repos/wreckhunter2000-1` → `/data/codebase/repos/wreckhunter2000-1` and `/mnt/t440/repo`.

## CLI quick reference

```bash
# From repo root (any mount path)
bash scripts/fleet status
bash scripts/fleet unified up      # FULL bootstrap (MCP, ZAYA :5203, Forge preset, n8n, peer jobs)
bash scripts/fleet unified on      # enable peer dispatch (no isolation block)
bash scripts/fleet up all          # alias for unified up
bash scripts/fleet recover         # import n8n + watchdogs + drain T440+c2 queues
bash scripts/fleet peer-jobs       # run T440 NFS jobs from cesarops2
bash scripts/fleet import-n8n
bash scripts/fleet jobs            # drain local node queue once
bash scripts/fleet dispatch t440 restart_n8n   # works when unified on
bash scripts/fleet catalog all     # catalog → rs-deep → summarize → plan
bash scripts/fleet env             # export REPO, FORGE_URL, …
```

**Unified mode** (`~/.cache/cesarops/fleet-unified`): Forge on c2, n8n DB on NFS (primary T440 host or c2 bridge), T440 P100 coders `:5001`/`:5002`, c2 `:5200` draft + `:5203` ZAYA thinker. Preset: `dual-coder-zaya`.

Optional symlink on `PATH`:

```bash
ln -sf /mnt/t440/repo/scripts/fleet ~/bin/fleet
```

## Service stack (unified)

```text
                    config/fleet_manifest.json
                              │
                    scripts/fleet (CLI)
                              │
         ┌────────────────────┼────────────────────┐
         ▼                    ▼                    ▼
   cesarops2              T440 n8n            var/fleet-jobs/
   Forge :9100            webhooks            NFS queue
   LLM :5200/:5202        fleet-ops ─────────► fleet-job-runner
         │                    │
         │              llm-translate (MXFP4 normalize)
         │              tool-route / pamp-route
         ▼
   mission_service_watchdog + gpu_slot_watchdog
         │
         ▼
   pipelines: catalog / rs-deep / rs-summarize / unified_pipeline_worker_bee
```

## n8n webhooks (canonical imports from `missions/`)

| Route | Workflow |
|-------|----------|
| `fleet-ops` | `missions/n8n_fleet_ops_dispatch.json` |
| `llm-translate` | `missions/n8n_llm_output_translator.json` |
| `tool-route` | `n8n_moe_tool_router.json` (uses same translator JS) |
| `pamp-route` | `missions/n8n_predictive_async_moe.json` |
| `prompt-tuner-*` | `missions/n8n_prompt_tuner_worker.json` |

Import once: `fleet import-n8n` or `bash scripts/import_n8n_health_workflows.sh`.

## Hygiene (done / required)

- Root `fleet-job-runner.sh` and `fleet-n8n-dispatch.sh` → wrappers to `scripts/`.
- Use **`scripts/fleet-job-runner.sh`** only (root copy was missing actions).
- Pin n8n data: `N8N_USER_FOLDER=/mnt/t440/data/n8n_data` (`start_n8n.sh` sets this when present).
- Do not import duplicate workflow JSON from repo root when `missions/` copy exists.

## Related docs

- [FLEET_N8N_ROUTES.md](./FLEET_N8N_ROUTES.md) — webhook URLs and recovery
- [FLEET_GPU_LAYOUT.md](./FLEET_GPU_LAYOUT.md) — GPU/port matrix
- [GPU_SLOT_WATCHDOG.md](./GPU_SLOT_WATCHDOG.md) — dynamic slot restore
- [FLEET_FILE_CATALOG_PLAN.md](./FLEET_FILE_CATALOG_PLAN.md) — catalog + RS pipelines
- [N8N_UNIFIED_WORKER_BEES.md](./N8N_UNIFIED_WORKER_BEES.md) — mission profile bees
