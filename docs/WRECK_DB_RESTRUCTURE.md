# Wreck database restructure plan

## Problem (May 2026 audit)

`features` is one flat table mixing:

| What it is | Rows | How you can tell |
|------------|-----:|------------------|
| Swayze historical census | ~2,090 | `source = Swayze2019-1.xlsx`, `coord_quality = swayze_parsed` |
| Your location estimates | ~7,026 | `source = Swayze2019-1.xlsx+estimated`, `place_estimated` / `lake_center` |
| Agent merge mislabels | **650** | Same as above but wrongly `dive_verified` |
| Real NOAA Thunder Bay | 81 | `source = ThunderBay_NOAA` |
| Preserve GPS scrape | **0 in DB** | Lives only in `outputs/great_lakes_preserve_wrecks.json` |

Also: **3,909 duplicate name groups** (e.g. 21× `DETROIT`) — same vessel name, different estimate passes.

**Swayze is not wrong to keep** — it is the missing-wreck census. The bug is treating estimated pins and survey GPS as the same trust level.

## Phase 1 (done by `scripts/restructure_wrecks_db.py --apply`)

- Backup `db/wrecks.db.bak-*`
- `record_tier`: `census` | `estimated` | `survey`
- Fix `coord_quality`: `agent_estimated`, `survey_verified`, keep `swayze_parsed`
- `supplemental_wrecks` table ← preserve scrape (312 GPS rows)
- Views: `features_census`, `features_estimated`, `features_survey`, `features_public_map`

## Phase 2 (applied)

- `pin_class` on `features` + `supplemental_wrecks`: `verified` | `estimated` | `parsed` | `unknown`
- `wreck_canonical` (~3,979 sites) + `wreck_canonical_members`
- 37 supplemental rows matched/upgraded onto Swayze features
- API: `GET /wrecks/map/geojson?layers=verified,estimated,parsed,canonical`
- UI: `tauri/dist-web/wrecks.html` — Leaflet map with selectable colored layers

### Pin colors (map)

| Layer | Meaning | Color |
|-------|---------|-------|
| **verified** | Survey, preserve registry, NOAA | Green |
| **estimated** | Place name / lake center / agent pass | Amber |
| **parsed** | Coordinates parsed from Swayze text | Gray |
| **canonical** | Best pin per vessel name (deduped) | Cyan |

Run: `python3 scripts/wreck_db_phase2.py --apply`

## Commands

```bash
python3 scripts/audit_wrecks_db.py
python3 scripts/restructure_wrecks_db.py --dry-run
python3 scripts/restructure_wrecks_db.py --apply   # backs up first
```

## coord_quality vocabulary (target)

| Value | Meaning |
|-------|---------|
| `swayze_parsed` | Coordinate text parsed from Swayze |
| `place_estimated` | Geocoded from historical place name |
| `lake_center` | Fallback lake centroid |
| `agent_estimated` | Agent merge pass (was wrongly `dive_verified`) |
| `preserve_registry` | Michigan preserves / sanctuary tables |
| `survey_verified` | NOAA Thunder Bay, AWOIS, dive surveys |
| `gps` / `chart` | Erie NDA / charted positions |
| `mission_gt` | Satellite mission ground truth bboxes |
