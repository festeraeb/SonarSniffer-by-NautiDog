# Straits of Mackinac — survey / BAG context (Cursor synthesis)

**Pinned Google URL** (friend prepped): see `../COLLAB_GOOGLE_URL.txt`  
Headless Playwright often gets CAPTCHA; use **Cursor Browser MCP** on that same URL while signed in.

## What “.bag file survey numbers” means here

1. **BAG (Bathymetric Attributed Grid)** — NOAA/NCEI standard grid per **hydrographic survey ID** (e.g. H13311 “Hydrographic Survey of Mackinac Straits”), not one Mackinac-only file.  
   - Product overview: [NCEI NOS Hydrographic Survey](https://www.ncei.noaa.gov/products/nos-hydro-survey)  
   - Map/search: [NOS hydro dynamic MapServer](https://gis.ngdc.noaa.gov/arcgis/rest/services/web_mercator/nos_hydro_dynamic/MapServer)  
   - Example survey: [H13311 metadata](https://www.ncei.noaa.gov/access/metadata/landing-page/bin/iso?id=gov.noaa.ncei:H13311_EM2040C)

2. **Historic side-scan “450” target numbering** — 1992 bottomland survey (East Moran Bay + Mackinac Island Harbor) lists targets as **`450 XX.XXXXXX depth'`** with Yes/No wreck flags, e.g.:
   - `450 50.847840 36.67'` — Shipwreck in Mackinac Island Harbor (new)  
   - `450 52.527840 43.10'` — Known wreck in pieces  
   - Full report: [govinfo 1992 bottomland survey](https://www.govinfo.gov/content/pkg/CZIC-gc87-m5-g3-1992/html/CZIC-gc87-m5-g3-1992.htm)

## Repo GT already aligned

- `scripts/known_wrecks_straits.json` — dive-verified boxes (Cedarville, Burns, etc.)  
- Forge calibration: Cedarville 45.7873,-84.6708; Burns 45.87127,-84.58642  
- Optical/temporal pipeline: `data/missions/straits_local_run.json`, NFS `straits_optical_*`

## Forge / physics tune (this job)

| Priority | Action |
|----------|--------|
| Now | Phase corr + LOO in `temporal.rs`; Z-cap in `poc.rs` |
| Measure | `sat-run` → `temporal_persistence_map.json` peak ≥ 0.30 near anchors |
| Later | Ingest NCEI BAG for bathy fusion bonus (Lane B / fusion.rs) |

BAG survey numbers feed **corroboration**, not the current Sentinel-2 clarity POC pass.
