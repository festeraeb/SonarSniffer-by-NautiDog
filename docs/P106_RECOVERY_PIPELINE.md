# P106 recovery pipeline — lost files → manifest → vector index

Maps recovered assets from the **laptop dump** (`/data/laptopdump`) onto healthy CESAROPS repos using P106 GPU for embeddings (not PAMP / not `:5201` llama).

## Architecture (improved vs raw dual-model sketch)

```text
[ Recovered tree: /data/laptopdump, integrate/unmapped, … ]
                    │
        ┌───────────┴───────────┐
        ▼                       ▼
 [ Phase 0: mechanical ]   [ Phase 1: heuristics ]
 SHA256 dedup              path patterns, empty stubs,
 skip .git/node_modules    wget/curl one-liners, EPHEMERAL
 skip binaries (.gguf…)    (no DistilBERT SST-2 — wrong task)
        │                       │
        └───────────┬───────────┘
                    ▼
        [ Phase 2: Jina code-v2 on P106 ]
        Centroids from var/recovery/clean_repos
                    │
                    ▼
        forge_file_manifest.json
        nautivecs_ingest.jsonl  → nautivecs / vector lane
                    │
                    ▼
        [ Fleet Phase 3 — later ]
        RTX thinker routes MATCHES_REPO_* batches → P100 coders format/commit
```

### Why not DistilBERT SST-2?

`distilbert-base-uncased-finetuned-sst-2-english` is **sentiment**, not “junk vs code”. Mislabels are common. We use **deterministic rules** first, then **semantic repo match** only on survivors.

### Tie-in to existing work

| Existing | Role |
|----------|------|
| `docs/LAPTOP_DUMP_ARCHIVES.md` | Where zips / broken archive live |
| `scripts/scan-script-inventory.sh` | Live repo `.sh` tiers (keep/review/ephemeral) |
| `SCRIPT_CLEANUP_FORGE_MISSION.md` | Phase 2 after manifest |
| `nautivecs` `:5003` | Ingest `nautivecs_ingest.jsonl` for RAG |

## n8n (online)

```bash
N8N_DIR=/mnt/t440/data/n8n bash scripts/start_n8n.sh
bash scripts/n8n_activate_fleet_workflows.sh
```

Fleet-ops / health webhooks should return 2xx after activation.

## Run on P106 (cesarops2)

```bash
# 1) Baseline symlinks to healthy repos
bash scripts/p106_recovery_run.sh bootstrap

# 2) Fast rules-only pass (no torch) — good first scan
bash scripts/p106_recovery_run.sh rules --max-files 5000

# 3) Full semantic pass on P106 GPU 0
bash scripts/p106_recovery_run.sh full

# Outputs
ls -la var/recovery/forge_file_manifest.json var/recovery/manifest_summary.json
```

Optional scan roots:

```bash
RECOVERY_SCAN_ROOT=/data/laptopdump/programming bash scripts/p106_recovery_run.sh rules
RECOVERY_SCAN_ROOT=$REPO/integrate/unmapped bash scripts/p106_recovery_run.sh rules
```

## Manifest categories

| Category | Action |
|----------|--------|
| `JUNK_EXACT_DUPLICATE` | Auto-archive / delete candidate |
| `JUNK_EMPTY_OR_STUB` | Auto-skip |
| `LIKELY_JUNK_SCRATCHPAD` | Human review list |
| `BINARY_OR_LARGE_SKIP` | Hash catalog only |
| `MATCHES_REPO_*` | Migration bundle per repo |
| `VALID_CODE_ORPHAN` | Review — maybe new module |
| `REVIEW_HEURISTIC_OK` | rules-only mode; re-run `--full` |

## Next steps (fleet)

1. Filter manifest → move duplicates/junk to `var/recovery/quarantine/`
2. POST `nautivecs_ingest.jsonl` into nautivecs (or Forge vector inject)
3. Forge mission: per `MATCHES_REPO_wreckhunter2000-1` batch → P100 format + import fix
4. Merge with `var/script-inventory/latest/forge-verdict.json` for shell cleanup

## GPU note

This workload uses **PyTorch on P106** (`CUDA_VISIBLE_DEVICES=0` on cesarops2). It does **not** bind `:5201` PAMP. Fleet roles can keep RTX/1070/P100 on llama while P106 runs recovery jobs.
