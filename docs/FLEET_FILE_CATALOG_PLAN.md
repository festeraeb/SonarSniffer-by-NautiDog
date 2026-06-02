# Fleet-wide file catalog → consolidation plan → vector index

**Question:** Scan all files on all computers, assign repo affinity, produce a consolidation plan, and build vector search for later.

**Answer:** Yes — that is the right *shape*, if you treat it as a **data pipeline**, not a one-shot “move everything” job.

## Why this is legitimate

| Principle | Why it matters |
|-----------|----------------|
| **Catalog before mutate** | Crash recovery and NFS sprawl need an authoritative inventory with hashes — not guesswork. |
| **Central manifest** | One merged view on T440/NFS (`var/fleet-catalog/`) so Forge, n8n, and models see the same truth. |
| **Vectors on survivors only** | Embed text/code after dedup + junk tiering — not every `.gguf` / GeoTIFF byte. |
| **Consolidation plan as output** | Plan = `{path, host, sha256, target_repo, action, confidence}` — humans/Forge approve before moves. |
| **Matches your stack** | You already have script inventory, blueprint audit JSONL, P106 mapper, nautivecs — this unifies them. |

## What “scan all computers” should mean (scoped)

**Include (per node):**

- Shared repo: `/data/codebase/repos/wreckhunter2000-1`, `/mnt/t440/codebase/...`
- Recovery: `/mnt/t440/data/laptopdump`, `integrate/unmapped`
- Operator home project dirs: `~/cesarops*`, `~/src` (optional)
- Node-local only if reachable: `ssh cesarops2 'find ...'`

**Exclude (always):**

- `.git`, `node_modules`, `target/`, `.cache`, `venv`
- Active model weights (`.gguf`) — **hash + path only**, no embed
- OS paths (`/proc`, `/sys`, `/usr`)

**Do not** blindly `find /` on every host — define **roots per node** in config (below).

## Recommended pipeline (4 phases)

```text
Phase 1 — MECHANICAL CATALOG (all nodes, CPU only)
  Each host: walk configured roots → JSONL row per file
  Fields: host, node, path, size, mtime, sha256, ext, category_hint

Phase 2 — MERGE + DEDUP (T440 or c2)
  Union JSONL → fleet_master.jsonl
  Collapse JUNK_EXACT_DUPLICATE by sha256
  Flag “canonical copy” = shortest path on preferred host (usually NFS repo)

Phase 3 — REPO AFFINITY + CONSOLIDATION PLAN (P106 or batch)
  Rules + Jina code-v2 vs clean_repos centroids
  Emit: consolidation_plan.json
    action: keep_in_place | move_to_repo | archive | quarantine | review
    target_repo: wreckhunter2000-1 | cesarops-forge-v2 | …

Phase 4 — VECTOR INDEX (nautivecs / future)
  Ingest: path, repo, action, 1–2k snippet, embedding id
  Used by Forge RAG — not for moving files by itself

Phase 3b — RS LLM SUMMARIZE (when Forge idle)
  After rs-deep-scan: RTX thinker summarizes gdal/examine_blob → consolidation_note
  Runner: bash scripts/fleet_rs_llm_summarize.sh start
  Monitor: bash scripts/fleet_rs_llm_summarize.sh status
  Output: var/fleet-catalog/rs_llm_summaries.jsonl, fleet_master_summarized.jsonl
```

## Consolidation plan (schema sketch)

```json
{
  "path": "/mnt/t440/data/laptopdump/.../cesarops_engine.py",
  "sha256": "abc…",
  "source_host": "t440",
  "category": "MATCHES_REPO_wreckhunter2000-1",
  "confidence": 0.71,
  "action": "move_to_repo",
  "target_path": "cesarops-inference/src/integrate/cesarops_engine.py",
  "notes": "duplicate of live file; keep newer mtime on NFS"
}
```

**Nothing moves until** `action` is approved (Forge mission or `forge-verdict.json` style gate).

## Fleet roots config (starter)

| Node | Scan roots |
|------|------------|
| **t440** | `/data/codebase/repos/wreckhunter2000-1`, `/mnt/t440/data/laptopdump`, `/data/laptopdump` |
| **cesarops2** | same NFS repo mount, `/home/cesarops/src` (if present) |
| **nautik9** | recovery only when online |

## How this relates to existing tools

| Tool | Role in unified pipeline |
|------|---------------------------|
| `scripts/scan-script-inventory.sh` | Phase 1 subset: `.sh` only, live repo |
| `scripts/blueprint_audit_dispatch.py` | Phase 1 pattern: `file_inventory.jsonl` + batches |
| `scripts/recovery/p106_drive_map.py` | Phase 3: repo match + manifest |
| `scripts/gpu_slot_watchdog.sh` | Unrelated — keeps inference up |
| **nautivecs** `:5003` | Phase 4 vector store |
| `SCRIPT_CLEANUP_FORGE_MISSION.md` | Execute approved plan on scripts |

## Risks (honest)

1. **Runtime** — full laptopdump + repo is large; run in batches (`--max-files`, per-subtree).
2. **False repo match** — orphans need `review`, not auto-move.
3. **NFS duplicates** — same file via two mount paths → dedup by hash essential.
4. **Stale vectors** — re-index after consolidation executes.

## Verdict

**Yes, go this way:** fleet catalog → merged manifest → consolidation plan → vector index.

**No:** single script that moves files on first pass, or SST-2 “junk” model, or embedding binaries.

## Remote sensing — deep examine (not 3-line skim)

For `asset_class=remote_sensing`, `scripts/recovery/rs_deep_profile.py` runs:

| Step | What |
|------|------|
| **gdalinfo -json** | CRS, size, bands, stats (when GDAL installed) |
| **Sidecars** | Full read of `.aux.xml`, `.tfw`, `.prj` (up to 256KB each) |
| **Header window** | Up to 512KB of raster for GeoTIFF ASCII tags |
| **NetCDF** | `netCDF4` global attrs + variables, or header preview |
| **GeoJSON/STAC** | Parse structure, `stac_version`, collections |
| **Filename** | HLS `HLS.L30.T…` token parse, Sentinel/Landsat/SAR hints |
| **examine_blob** | Up to ~48KB text bundle for plan + vector ingest |

RS rows get `consolidation_priority=high` and actions `catalog_data` / `review_rs` — never auto-delete.

## Parallel RS deep scan (speed)

Fast catalog tags RS files as `deep_scan_status: pending` (`--defer-rs-deep`, default in `fleet_file_catalog.sh`).

Immediately after, **`fleet_rs_deep_scan.py`** fans work across configured workers in `config/fleet_catalog_roots.json` → `rs_deep_workers`:

| Worker | Role |
|--------|------|
| `c2-p106-gdal` | 2 parallel GDAL jobs on P106 (GPU 0) — **not** llama :5201 |
| `c2-cpu-pool` | 8-way CPU pool on cesarops2 |
| `t440-cpu-pool` | 8-way via SSH on T440 (NFS + laptopdump paths) |

This is **GDAL + sidecars + headers**, not Gemma/Qwen — keeps inference GPUs free for `plan --embed`.

Optional later: RTX thinker summarizes completed `rs_profile` JSON (small prompts) — not in v1.

## Commands

```bash
# Full pipeline: fast catalog → merge → parallel RS deep → merge → plan
bash scripts/fleet_file_catalog.sh all

# Or explicit steps
bash scripts/fleet_file_catalog.sh catalog-all
bash scripts/fleet_file_catalog.sh merge
bash scripts/fleet_file_catalog.sh rs-deep-scan
bash scripts/fleet_file_catalog.sh merge
bash scripts/fleet_file_catalog.sh plan --embed

# Local node only, cap for test
bash scripts/fleet_file_catalog.sh catalog --max-files 5000

# Merge + rules-only plan
bash scripts/fleet_file_catalog.sh merge
bash scripts/fleet_file_catalog.sh plan

# Plan with P106 Jina embeddings on examine_blob
bash scripts/fleet_file_catalog.sh plan --embed

# Outputs
ls var/fleet-catalog/fleet_master.jsonl
ls var/fleet-catalog/consolidation_plan.json
ls var/fleet-catalog/nautivecs_ingest.jsonl
```
