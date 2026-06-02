# Great Lakes wreck database inventory

Last audit: 2026-05-24. **You do not need to rebuild from scratch** — the primary catalog exists and is online.

## Role of Swayze

David Swayze’s Great Lakes wreck research is the **baseline census** (~9,800 features) in `wrecks.db` (`source: Swayze2019-1.xlsx`). Most coordinates are **estimated** (`coord_quality: swayze_parsed`, `place_estimated`, `lake_center`). Swayze is the resource for wrecks **still missing** precise positions — not the only catalog we use.

Enrichments on top of Swayze in the same DB:

| Enrichment | Count (live `/stats`) |
|------------|----------------------|
| With coordinates | 9,688 |
| BGSU hull material | 9,466 |
| NAMAG magnetic features | 578 |
| Steel freighters (ML) | 2,740 |
| Iron ore carriers | 424 |

## Online (hosted today)

| Service | URL | Data |
|---------|-----|------|
| **Wrecks REST API** | https://api.cesarops.org/wrecks | `wrecks.db` → `features` table |
| Stats | https://api.cesarops.org/stats | Summary counts |
| DB health | https://api.cesarops.org/db/health | SQLite probe |
| Search | https://api.cesarops.org/wrecks/search/query?q= | Name search |
| KML live | https://api.cesarops.org/wrecks/live.kml | Map overlay |
| Erie tool list | https://api.cesarops.org/tools/erie-scanner/known-wrecks | ~47 hardcoded Erie wrecks |
| Swagger | https://api.cesarops.org/docs | OpenAPI |
| **Web browser** | https://api.cesarops.org/wrecks.html | Static UI → API (after deploy) |

**Runtime:** `wrecks-api.service` on T440 `:8099`, Cloudflared → `api.cesarops.org`.

**DB file:** `/codebase/repos/wreckhunter2000-1/db/wrecks.db` (16 MB, **9,847** rows live).

## Fragmented catalogs (not merged into one table yet)

| Catalog | Path / API | Format | ~Records | Purpose |
|---------|------------|--------|----------|---------|
| **known_wrecks.json** | `backup/deploy/tools/cesarops-core-github/known_wrecks.json` | JSON bboxes | **18** | Satellite GT, conductor probes (MI/Huron/Straits) |
| **erie_known_wrecks_db.py** | `projects/pipelines/mag/erie_known_wrecks_db.py` | Python → CSV | **91** | Erie mag training targets (NDA + Wikipedia + ShipwreckWorld) |
| **erie_wellhead_discriminator** | `projects/pipelines/mag/erie_wellhead_discriminator.py` | Python lists | **~47** | Erie scanner positives (API endpoint) |
| **web_scraped_wrecks.json** | `backup/wreckhunter2000-1/outputs/web_scraped_wrecks.json` | JSON | **167** | Preserve-registry / web scrape coords |
| **wrecks_db_verified_coords.json** | `backup/wreckhunter2000-1/outputs/wrecks_db_verified_coords.json` | JSON export | **~9,128** | Subset of Swayze with estimated coords |
| **known_wrecks_erie.json** | `backup/wreckhunter2000-1/known_wrecks_erie.json` | JSON | **2** | Stub Erie scrape |
| **LAKE_MICHIGAN_CENSUS_2026.db** | `projects/cesarops-db-connector/...` | SQLite | anomalies | SWOT/thermal **detections**, not wreck census |

## Ingest / rebuild scripts

| Script | Status | Action |
|--------|--------|--------|
| `rebuild_enhanced_wrecks_db.py` | **Missing from repo** (documented in `gl-wrecks-api/README.md`) | Re-merge CSVs from `bagfilework/training/`, `recovered/` |
| `wh2k_awois_scraper.py` | `projects/pipelines/mag/` | INSERT `dive_verified` rows (AWOIS, 3dshipwrecks, CLUE, Thunder Bay) |
| `wreck_web_scraper.py` | `backup/deploy/tools/` | → `web_scraped_wrecks.json` |
| `wreck_scraper.py` | `cesarops-core-github/` | Builds `known_wrecks.json` |
| `scripts/merge_wreck_sources.py` | **New** | Report + optional `supplemental_wrecks` table for non-Swayze sources |

## Consumers by pipeline

| Pipeline | Uses |
|----------|------|
| Satellite `sat_mission_orchestrator` | `known_wrecks.json` only (18) — **not** full Swayze |
| BAG / advanced scan | `wrecks.db` + `_match_swayze_wrecks()` |
| Mag `mag_data_pipeline` | `wrecks.db` `features` |
| Forge tools | `load_satellite_env()` + optional Swayze match |
| Tauri / Mission Control | `api.cesarops.org` client |

## Recommended “one hosted database” plan

1. **Keep** `wrecks.db` as canonical (already online).
2. **Merge supplements** — Erie 91 + web_scraped 167 + `known_wrecks` 18 into `features` with `source` / `coord_quality` tags (`erie_nda`, `preserve_registry`, `mission_gt`).
3. **Fix satellite GT** — point missions at merged DB or expand `known_wrecks.json` from Swayze bbox query.
4. **Website** — `wrecks.html` on app/api host (done in `tauri/dist-web/`).
5. **Restore** `rebuild_enhanced_wrecks_db.py` from backup or reimplement via `merge_wreck_sources.py`.

## External references (not in repo)

- greatlakeswrecks.com, ohiodnr.gov lake-erie-wrecks, NOAA AWOIS, BGSU hull research, NAMAG grids — cited in scrapers and docs.
