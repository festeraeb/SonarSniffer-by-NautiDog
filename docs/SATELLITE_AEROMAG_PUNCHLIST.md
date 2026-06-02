# Satellite + Aeromag Pipeline — Master Punchlist
Generated: 2026-06-01. Source: SATELLITE_PIPELINE.md, SATELLITE_PIPELINE_GAPS.md, live code audit, laptop dump diff.

---

## PART 1 — Fix existing pipeline issues (from SATELLITE_PIPELINE_GAPS.md)

### G1 — HIGH: gt_wreck_names filter broken
**File:** `pipelines/satellite/sat_mission_orchestrator.py` lines 120–160
**Problem:** When `gt_wreck_names` is set in the mission spec, the bbox filter still loads ALL wrecks in the bbox, then warns if a named wreck is outside bbox but includes it anyway. The filter logic is inverted — it should load ONLY named wrecks, falling back to bbox if no names given.
**Fix:** In `_load_gt_wrecks()`, when `want_names` is non-empty, filter by name first, then validate bbox. Remove the "including anyway" warning path — if a named wreck is outside bbox, that's a spec error, raise it.
**Test:** Run dry-run with `straits_known_wreck_validation.json` — should return exactly Cedarville, Eber Ward, Cayuga, Gilcher, not all Erie wrecks.

---

### G2 — HIGH: dry_run skips target_known → validate_gt gets no_data
**File:** `pipelines/satellite/sat_mission_orchestrator.py` lines 282–295
**Problem:** `stage_target_known()` on dry_run calls `_write_dry_run_targeting_csv()` which writes a fixture CSV — this part is actually implemented. But `stage_validate_gt()` at line 337 also returns early on dry_run before reading the CSV. So validate_gt always returns `no_data` in dry runs.
**Fix:** In `stage_validate_gt()`, remove the early dry_run return. Let it read the fixture CSV that `_write_dry_run_targeting_csv` already wrote. The fixture CSV has `best_zscore: 3.5` and `score: 3.5` which is enough for validate_gt to produce a real report.
**Test:** Dry-run should produce a `validation_report.json` with hit/miss counts, not all `no_data`.

---

### G3 — HIGH: detection_scan tiles have empty image_b64
**File:** `pipelines/satellite/tile_image_fetch.py` lines 25–35
**Problem:** `to_b64()` returns `PLACEHOLDER_B64` (a 1x1 transparent PNG) when no chip file is found. The `stage_detection_scan()` in orchestrator calls `tile_image_fetch.to_b64()` per wreck but the download dir is empty in dry-run, so every tile gets the placeholder.
**Fix:** Two-part:
1. In `stage_detection_scan()`, pass the actual download dir from `paths["download_dir"]` to `tile_image_fetch.find_chip()`.
2. In dry-run mode, generate a synthetic 64x64 grey PNG per wreck lat/lon instead of the 1x1 transparent placeholder — enough for the detection worker to accept the payload without erroring.
**Test:** `detection_scan` stage should submit tiles with non-empty `image_b64` and get a job_id back from `:5580/scan`.

---

### G5 — MED: No combined Great Lakes runner script
**File:** Missing — needs to be created at `scripts/run_great_lakes_satellite.sh`
**Problem:** No single script runs Straits + Lake Michigan north + south in sequence.
**Fix:** Create `scripts/run_great_lakes_satellite.sh` that:
1. Runs `straits_known_wreck_validation.json`
2. Runs `lake_michigan_north_wreck_validation.json` (also needs to be created — see G10)
3. Aggregates reports into a combined summary JSON
**Depends on:** G10 (LM mission spec)

---

### G6 — MED: n8n workflow uses wrong spec_path
**File:** `pipelines/satellite/missions/n8n_satellite_optical_workflow.json`
**Problem:** The n8n executeCommand node references spec paths under the repo (`/codebase/repos/wreckhunter2000-1/...`) not the canonical live path (`/codebase/projects/pipelines/...`).
**Fix:** Update all `spec_path` references in the n8n JSON to use `/codebase/projects/pipelines/satellite/missions/`. Also update the forge webhook handler in `cesarops-forge-v2/src/tools.rs` `load_satellite_env()` to resolve paths from `CESAROPS_PIPELINES` env var.
**Test:** Trigger n8n webhook → should find the spec file without 404.

---

### G8 — MED: temporal_stack untested live
**File:** `pipelines/satellite/temporal_stack_engine.py` lines 100–143
**Problem:** v1 only catalogs STAC scenes, doesn't download B03/B08 band chips. The `design_note` in the output says "Wire overlay_grid stamps for sub-pixel align before stack" — this is unimplemented.
**Fix (v2 design):**
1. After STAC catalog, download B03 and B08 chips per scene per wreck center (use `wh2k_chip_extractor.py` as the chip fetcher)
2. Compute NDWI = (B03-B08)/(B03+B08) per chip
3. Stack ratios across scenes, compute persistence z-score per pixel
4. Wire `overlay_grid.rs` (in `backup/deploy/tools/`) for sub-pixel stamp alignment before stacking
**Note:** This is the highest-value unimplemented feature. Defer to Phase 2 after G1/G2/G3 are green.

---

### G10 — MED: No Lake Michigan mission spec or n8n webhook
**Files:** Missing `pipelines/satellite/missions/lake_michigan_north_wreck_validation.json`
**Fix:** Create LM north mission spec using bbox `[41.6, -87.5, 44.0, -85.5]` with same structure as `straits_known_wreck_validation.json`. Add LM webhook to n8n workflow.

---

## PART 2 — Import from laptop dump (not in live pipeline)

### D1 — IMPORT: nauticuvs_satellite_scan.py (139 lines)
**Source:** `/data/laptopdump/programming/cesarops-wreckhunter build/wreckhunter2000/nauticuvs_satellite_scan.py`
**What it does:** Processes Sentinel-2 imagery with real Nauticuvs curvelets. This is the missing curvelet rescore for satellite (currently satellite uses LoG proxy, mag uses real FDCT).
**Action:** Copy to `pipelines/satellite/nauticuvs_satellite_scan.py`. Wire into `sat_mission_orchestrator.py` as optional `curvelet_rescore` stage when `use_curvelet_rescore: true` in knobs.
**Priority:** HIGH — closes the satellite/mag parity gap on curvelet analysis.

---

### D2 — IMPORT: satellite_target_fetcher.py (783 lines)
**Source:** `/data/laptopdump/programming/cesarops-wreckhunter build/wreckhunter2000/satellite_target_fetcher.py`
**What it does:** Automated multi-sensor satellite data puller for any bounding box. Fetches from NASA Earthdata and generates KML/KMZ output. More complete than `universal_downloader.py` for the satellite-specific case.
**Action:** Copy to `pipelines/satellite/satellite_target_fetcher.py`. Review for overlap with `nasa_earthdata_client.py` — may supersede or complement it. Wire as alternative download backend in `stage_download()`.
**Priority:** MED — useful for Lake Michigan expansion.

---

### D3 — IMPORT: daily_satellite_pull.py (304 lines)
**Source:** `/data/laptopdump/programming/cesarops-wreckhunter build/daily_satellite_pull.py`
**What it does:** Scheduled daily satellite data pull — cron-style runner for keeping scene inventory current.
**Action:** Copy to `scripts/daily_satellite_pull.py`. Add as a Nomad periodic batch job (fits the rclone-state-sync pattern already in infra/nomad/jobs/).
**Priority:** MED — needed for production ops, not blocking dev.

---

### D4 — IMPORT: rossa_satellite_timing.py (199 lines)
**Source:** `/data/laptopdump/programming/cesarops-wreckhunter build/wreckhunter2000/rossa_satellite_timing.py`
**What it does:** Finds satellite passes over the Rossa search zone during/after sinking. Specific to one wreck case but the timing logic is reusable for any event-triggered search.
**Action:** Copy to `pipelines/satellite/event_timing.py` (rename for generality). Extract the pass-timing logic into a reusable function. Wire as optional `event_timing` stage in orchestrator.
**Priority:** LOW — specialized, but the timing logic is useful for SAR cases.

---

### D5 — REVIEW: wh2k_synthetic_tiles.py (dump has it, live has it — check divergence)
**Source:** `/data/laptopdump/programming/pipelines/satellite/wh2k_synthetic_tiles.py`
**Live:** `/codebase/projects/pipelines/satellite/wh2k_synthetic_tiles.py`
**Action:** Run `diff` — if dump is newer/different, review and merge. The dump also has `wh2k_synthetic_tiles_huron.py` and `wh2k_synthetic_tiles_v2.py` — check if v2 is in live.
**Priority:** LOW.

---

### D6 — MAG IMPORT: mag_pipeline_stage.py (from wreckhunter build)
**Source:** `/data/laptopdump/programming/cesarops-wreckhunter build/wreckhunter2000/bag_processor/mag_pipeline_stage.py`
**What it does:** Mag pipeline stage wired into the bag processor — cross-sensor fusion of mag + BAG data.
**Action:** Check if this is the same as `wrecks_api/stages/mag_pipeline_stage.py` in live. If different, merge the bag-processor-specific wiring.
**Priority:** MED — needed for triple-lock fusion.

---

## PART 3 — Aeromag pipeline gaps

### A1 — Wire aeromag orchestrator to forge
**File:** `pipelines/mag/erie_central_aeromag_orchestrator.py` (743 lines, live-only, no stubs)
**Problem:** The orchestrator exists and works standalone but is NOT wired as a forge tool. There's no `/tool/mag_mission` endpoint in `cesarops-forge-v2/src/tools.rs`.
**Fix:** Add `mag_mission` tool to `tools.rs` mirroring `sat_mission`. The orchestrator already accepts `--spec` and `--dry-run` flags.
**Priority:** HIGH — without this, aeromag can't be triggered from forge or n8n.

---

### A2 — Wire nauticuvs curvelet to aeromag
**File:** `pipelines/mag/nauticuvs_mag_curvelet.py` (94 lines, live-only)
**Problem:** File exists but is it called from `erie_central_aeromag_orchestrator.py`? Check the import chain.
**Fix:** Verify `nauticuvs_mag_curvelet.py` is imported and called in the orchestrator's curvelet pass. If not, wire it.
**Priority:** HIGH — this is the core differentiator of the mag pipeline.

---

### A3 — mag_rust_detect.py bridge to cesarops-aeromagnetic-worker
**File:** `pipelines/mag/mag_rust_detect.py` (58 lines, live-only)
**Problem:** This is the Python→Rust bridge that calls the `cesarops-aeromagnetic-worker` binary. Verify it correctly calls the compiled binary and passes the right args.
**Fix:** Check the binary path, arg format, and output parsing. The Rust worker has `discriminator.rs`, `dipole_analysis.rs`, `curvelet.rs`, `adaptive.rs` — make sure the Python bridge exercises all of them.
**Priority:** HIGH — this is the Python→Rust handoff point.

---

### A4 — Build and verify cesarops-aeromagnetic-worker
**File:** `cesarops-aeromagnetic-worker/` (Rust crate)
**Problem:** Unknown if this builds clean and produces correct output.
**Fix:** `cargo build --release -p cesarops-aeromagnetic-worker`. Run against a known Erie dipole test case. Compare output to `pipelines/mag/dipole_analysis.py` reference output.
**Priority:** HIGH — must be green before A3 can be validated.

---

## PART 4 — Rust conversion targets (Phase 2, parallel)

These are the Python files with clear Rust equivalents already partially started or well-scoped:

| Python file | Rust target | Status | Notes |
|---|---|---|---|
| `satellite/wh2k_chip_extractor.py` | `cesarops-inference/src/satellite_stitch.rs` | Partial | Chip extraction + stitch logic |
| `mag/dipole_analysis.py` | `cesarops-aeromagnetic-worker/src/dipole_analysis.rs` | Partial | Core dipole math |
| `mag/flight_line_physics.py` | `cesarops-aeromagnetic-worker/src/pipeline.rs` | Partial | Flight line normalization |
| `mag/adaptive_background_scan.py` | `cesarops-aeromagnetic-worker/src/adaptive.rs` | Partial | Adaptive background |
| `mag/nauticuvs_mag_curvelet.py` | `cesarops-aeromagnetic-worker/src/curvelet.rs` | Partial | Curvelet via nauticuvs |
| `satellite/temporal_stack_engine.py` | New: `cesarops-inference/src/temporal_stack.rs` | Not started | v2 chip stacking on P100 |
| `satellite/wh2k_sentinel_wreck_targeting.py` | New: `cesarops-detection/src/satellite_scorer.rs` | Not started | Scoring + z-score |
| `mag/mag_data_pipeline.py` | New: `cesarops-aeromagnetic-worker/src/ingest.rs` | Not started | Grid ingest + normalize |

**Rule:** Don't convert until the Python version is verified working. Convert = port logic, keep Python as reference test oracle.

---

## Execution order

```
Phase 1 (fix first, unblock everything):
  G1 → G2 → G3 (in order, each unblocks the next)
  A4 (build aeromag worker — independent)
  A3 (verify Python→Rust bridge — needs A4)

Phase 2 (import + wire, can be parallel):
  D1 (nauticuvs_satellite_scan — high value)
  A1 (wire aeromag to forge)
  A2 (verify curvelet wiring)
  G5 + G10 (combined runner + LM spec)
  G6 (fix n8n paths)
  D2, D3, D4 (remaining imports)

Phase 3 (Rust conversion, after Phase 2 green):
  temporal_stack.rs (highest value, GPU on P100)
  satellite_scorer.rs
  ingest.rs for mag
  Verify all Rust outputs match Python oracles

Phase 4 (ops):
  G8 (temporal stack v2 live test)
  D3 as Nomad periodic job
  Full Great Lakes end-to-end run
```

---

## Sub-agent review tasks (pending)

The following laptop dump files need a sub-agent to read and verify before import:
- `satellite_target_fetcher.py` (783 lines) — verify no Windows path hardcoding, check API key handling
- `nauticuvs_satellite_scan.py` (139 lines) — verify nauticuvs import path matches live install
- `daily_satellite_pull.py` (304 lines) — verify cron logic, check for hardcoded paths
- `rossa_satellite_timing.py` (199 lines) — verify generalizability beyond Rossa case
- `mag_pipeline_stage.py` (bag processor version) — diff against live `wrecks_api/stages/mag_pipeline_stage.py`


---

## PART X — Port experimental bathymetry mapper into satellite (NEW 2026-06-02)

### B1 — MED: Port bathymetry_mapper.py into cesarops-satellite (Rust)
**Source:** `recovery/laptopdump/cesarops_core/src/cesarops/bathymetry_mapper.py` (Phase 11.2, experimental, recovered intact from laptop dump).
**Goal:** Experimental multi-band / multi-angle bathymetric mapping from satellite passes — reconstruct a wreck/seafloor depth surface by combining the depth proxies derived from different Sentinel-2/Landsat bands and the differing solar/view geometry across the multi-date stack. User: "I can map the wrecks from the different bands and angles the satellites passed over."
**What the Python does (preserve these capabilities):**
- `BathymetryMapper`: grid-based interpolation from scattered (lat,lon,depth) trackpoints (scipy `griddata` linear/cubic/nearest; NN fallback).
- `compute_slope()` (np.gradient magnitude), `compute_curvature()` (sum of 2nd derivatives) — detect drop-offs / ridges.
- `find_drop_offs(slope_threshold)` — connected steep regions (scipy.ndimage.label).
- `generate_contours()` — depth contours at intervals.
- Exporters: GeoTIFF, KML, NetCDF, GeoJSON. Depth color map shallow→deep.
**Rust port plan:**
- New module `cesarops-satellite/src/bathymetry_map.rs`.
- Input = the per-band depth proxies the satellite pipeline already computes (Secchi/log-ratio depth from blue/green, plus the multi-date stack). Each scene/band/angle contributes scattered depth estimates → fuse into one grid.
- Reuse the windowed interpolation + gradient helpers; export GeoTIFF via the `image`/`tiff` crate already used in chip.rs; KML/GeoJSON as plain text writers.
- Wire as an optional satellite stage (e.g. `bathy_map`) gated by a knob; feed its drop-off/relief output as one more concept signal into fusion::SignalBundle (NOT an automatic wreck call).
**Cross-link:** pairs with the BAG uncertainty-unmask reconstruction (bag-scan `--unmask`) — both rebuild a hidden/derived depth surface; keep export formats compatible so they overlay in the same viewer.
**Status:** NOTED — not yet implemented. Build after BAG unmask engine lands.


### B1.1 — Multi-band satellite-derived bathymetry (SDB) application to the masked target
**Context (added 2026-06-02 during live Straits run):**
The masked target E of Elva (primary hyp: Robert Burns, 126ft wood barquentine)
sits at **113ft peak / 145ft floor (34m / 44m), 32ft relief**, per BAG unmask.

**Physics reality check — SDB depth limit:**
- Stumpf/Lyzenga log-ratio SDB (`log(B02)/log(B03)`) works to ~2-3× Secchi depth.
- Straits Secchi ~8-12m → SDB bottom detection ~20-30m max.
- **34m (113ft) is at/beyond the SDB limit — do NOT expect direct bottom
  reflectance off the hull.** A naive "depth from reflectance on the wreck"
  will fail at this depth. State this honestly in any output; do not overclaim.

**Where SDB IS useful for this target (build these into bathymetry_map.rs):**
1. **Shoal mapping** — the wreck sits on an elevated feature; map the shallower
   flanks (<30m) that ARE visible, to characterize the shoal it rests on.
2. **Plume/clarity column signal** — the detection is the water-column
   disturbance (cold-sink on steel; physical-obstruction clarity/current on
   WOOD) that rises toward the thermocline and reads optically at ~180ft
   APPARENT depth in blue-green. SDB band-ratio tracks this column turbidity
   change even when the true bottom is invisible. This is the real signal for
   a deep wood wreck — NOT bottom reflectance.
3. **Calibration via shallow known wrecks** — Elva and the diveable preserve
   wrecks (<30m) give control points to fit the SDB depth model (offset+gain),
   same approach used for the coordinate-offset correction.

**Material-awareness (from user, 2026-06-02):**
- Thermal cold/heat-sink signature is strong on STEEL, weak on WOOD.
- Wood wrecks still produce a signature via PHYSICAL OBSTRUCTION: zebra-clarity
  (sediment trap / biofouling / bottom-reflectance break), current-driven
  surface glint modulation, and shadow/texture.
- → Fusion should weight concepts by suspected hull material: down-weight
  thermal for wood targets, up-weight clarity + glint.

### B1.2 — NEW concept gap: blue-green glint / current-roughness detector
The Straits have strong currents. A 32ft-relief intact hull perturbs the flow,
modulating surface roughness → sun-glint pattern change, visible in blue-green.
Existing `concept_shadow_roughness` uses NIR (B08) which does NOT penetrate
water — useless for a deep target. **Build a blue-green (B02/B03) surface
glint / current-roughness concept** for non-thermal (wood) wreck detection.
This is the detector that should carry the Burns-type targets. Cross-link to
drift.rs current fields.
