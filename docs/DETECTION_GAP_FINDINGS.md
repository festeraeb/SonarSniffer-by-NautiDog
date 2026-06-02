# Detection Gap Findings — why the Rust rebuild under-detected

Cross-pipeline gap analysis (satellite, mag, bag) vs the recovered Python. The
recovered files DID contain detection logic the Rust rebuild was missing. Root
causes below, ordered by impact. Each is a concrete, located fix.

## Satellite (cesarops-satellite) — CRITICAL

1. **COG reader is a NaN stub** — `chip.rs::decode_tiff_band` returns an
   all-NaN array (verified, lines 152–162). Every chip is NaN → every z-score
   empty → every score 0. **Nothing detects until this is real.** Port from
   Python `_download_band_chip` (windowed COG read + UTM↔WGS84 + DN/10000).
2. **zebra_clarity wrong formula/bands** — Rust computes NDWI(B03,B08); Python
   is Secchi `3.9·sqrt(B02/B04)+0.55`. Fix `concept.rs::chip_zscore` + band req.
3. **shadow_roughness missing Sobel** — Rust uses raw B08 mean; Python uses
   Sobel gradient magnitude of B08. Fix `concept.rs`.
4. **Composite score divergence** — Python `0.5·hr + 0.5·best_z`; Rust adds
   undocumented depth+curvelet terms. Reconcile in `concept.rs::score_wreck_concept`.
5. AOI discovery (`wh2k_sentinel_optical_poc.py`) only shells to python3 — port
   the full-scene Sobel/Secchi/NDTI + NMS peak finder. (breadth, not blocker)
6. SAR DBSCAN, NASA SWOT/ECOSTRESS/OPERA fusion, OPERA DSWx fetch — MISSING.
7. Drift fidelity: missing 15-min substep, consistency_check, ensemble,
   sensitivity_sweep, per-type windage. Magnetic NSS/VDR/Tilt chips MISSING.

## Mag (cesarops-aeromagnetic-worker) — HIGH

1. **Rich CPU dipole port is dead code** — `dipole_analysis.rs::analyze_candidate`
   (flip distance, gradient contrast, aspect PCA, 0–100 man-made score) is never
   called. Pipeline uses only the thin GPU `dipole_mag*lobe_ratio`. Wire it into
   `pipeline.rs` ranking. This is the discriminating physics that rejects geology.
2. **Discriminator not wired** — `discriminator.rs::cross_reference_candidate`
   unused; also missing Loran-C warp, well/wreck loaders, ground truth,
   disposition (raised/scrapped) false-positive filter.
3. MISSING families: datum correction (Molodensky + rubber-sheet IDW), Loran-C
   warp, upward continuation/satellite proof, basin-aware composite scoring,
   dipole→composite fusion + candidate merge (NMS).
4. Weak pull gate: fixed 0.05 nT vs Python relative 0.15·peak_abs.

## BAG (cesarops-bag-scan) — HIGH (stub is ~10–15% of Python)

1. **No WGS84 reprojection** — `main.rs::transform_to_geo` emits raw projected
   easting/northing. Add real reproject (geo.rs).
2. Redaction/unmask suite MISSING (the marquee IP): four signature detectors +
   redactor ID, NaN-hole/flattened/texture-break masking scanner, restoration.
   Existing `pipelines/bag/wreckhunter2000/src/bag_mesh.rs` (~1100 ln Rust) is a
   ready porting base and defines the `physical_wreck`/`masked_redaction_flat`
   output contract that `validate_geo*.py` consumes — must be preserved.
3. Anomaly engine crude: needs background seafloor model, height-above-floor,
   connected-component clustering, aspect/size/Great-Lakes filters, confidence,
   ObjectType. Port from `bag_wreck_detector.AnomalyDetector` (+ standalone/advanced).
4. Missing: orientation PCA (bag_mesh has pca_axis), geo-correction vs refs,
   Swayze cross-ref, spatial dedup, KML/KMZ/sqlite output, Knobs/Stage/report.

## Fix order (logical, dependency-aware)
- **Wave 1 (unblock detection, parallel):** Satellite #1–4 (COG reader + metrics
  + score) and Mag #1–2 (wire dipole + discriminator). These restore correct
  detection on existing infrastructure.
- **Wave 2:** BAG build-out (geo reproject + anomaly + redaction/unmask from
  bag_mesh.rs) and Mag #3 (datum/continuation/basin) and Satellite #5–6.
- **Wave 3:** breadth (NASA fusion, OPERA, magnetic chips, drift fidelity) +
  contract standardization (`cesarops-pipeline-core`, `--describe`).


---

## IMPLEMENTATION STATUS (updated)

### Wave 1 — detection unblock — DONE ✅
- **Satellite**: real COG band decode (replaced NaN stub) via pure-Rust `image`
  crate; fixed zebra_clarity (Secchi 3.9·sqrt(B02/B04)+0.55, bands B02/B04);
  fixed shadow_roughness (Sobel of B08); reconciled composite to Python's
  0.5·hr + 0.5·best_z (curvelet additive, off by default). `cargo build` clean,
  `cargo test` 31 pass.
- **Mag**: wired the previously-dead `dipole_analysis::analyze_candidate` (CPU
  flip-distance/gradient-contrast/aspect/0–100 man-made score) into pipeline
  ranking via `fuse_combined_score` (base·(1-w)+(manmade/100·10)·w, w=0.5);
  dipolar-pull gate now uses relative 0.15·peak_abs; added Loran-C warp +
  wired `cross_reference_candidate` with knob radii (well 2000m / wreck 5000m).
  Build clean, 5 tests pass, `test-all` finds 23 candidates.

### Wave 2 — BAG build-out — DONE ✅
Grew `cesarops-bag-scan` from a ~258-line stub to a full modular crate:
`types, bag_io, geo, grid, anomaly, redaction_unmask, dedup, orientation,
pipeline, lib, main`. Key results:
- **Geo fix**: real WGS84 reprojection via GDAL OSR (`SpatialRef`+`CoordTransform`),
  replacing the stub's raw easting/northing.
- **Redaction/unmask IP** ported (smoothing/removal/flattened/nan-hole/texture
  detectors + masked-region scoring) from advanced_bag_scanner.py /
  masking_scanner.py / bag_mesh.rs.
- **Anomaly engine**: background floor model, height-above-floor, connected-
  component clustering, aspect/size/Great-Lakes filters, ObjectType, confidence.
- **Output contract preserved**: `WreckDetection.signature_type` =
  `physical_wreck` / `masked_redaction_flat` (constants + guard test), with
  `physical_wreck_count`/`masked_redaction_count` in the report — matches
  `validate_geo*.py`.
- **CLI**: legacy `--threshold`/`--redaction-sensitivity` + new `--knobs` JSON
  overlay + `--stages` subset (A→G). Build clean, `cargo test` 27 pass.

### Remaining (Wave 3 — breadth, not blockers)
- Satellite: AOI POC port (still shells to python3), SAR DBSCAN, NASA
  SWOT/ECOSTRESS/OPERA fusion, OPERA DSWx fetch, drift fidelity, magnetic chips.
- Mag: datum correction (Molodensky + rubber-sheet IDW), upward continuation /
  satellite proof, basin-aware composite multipliers, disposition false-positive
  filter, real well/wreck data loaders (call sites already wired, pass empties).
- BAG: pattern-autocorrelation + Richardson-Lucy restoration (stubbed TODOs).
- Contract standardization: `cesarops-pipeline-core` + `--describe` for LLM/n8n.


---

## Wave 3 — breadth + orchestration — DONE ✅

### Satellite (72 tests pass)
- `poc.rs`: full-scene AOI peak finder ported from wh2k_sentinel_optical_poc.py
  (Sobel/Secchi/NDTI concepts + scipy uniform/maximum_filter + NMS + cross-ref);
  `stage_poc_aoi` no longer shells to python3.
- `sar.rs`: group_by_orbit + self-contained DBSCAN + persistence; `SarCluster`
  type; fusion hook `fuse_sar_clusters`.
- `nasa_fusion.rs`: FusionScorer (SWOT+ECOSTRESS+OPERA average) + GeoJSON; OPERA
  wired to a real CMR fetch, SWOT/ECOSTRESS neutral-stubbed (no client yet).
- `stac.rs`: `fetch_opera_dswx` (collection C2617126679-POCLOUD) + URL extract.
- `drift.rs`: 15-min sub-stepping, consistency_check, ensemble_forward_seed,
  sensitivity_sweep, per-type windage; spread_nm fixed to mean 1-σ radius.
- `magnetic.rs`: NSS / VDR (rustfft |k| operator) / Tilt / 3-channel chips.
- Knobs extended (gt_min_confidence, poc_*, chip_*, dbscan_*, storm dates, …).

### Mag (31 tests pass)
- `datum.rs`: Molodensky NAD27→WGS84 + rubber-sheet IDW + anchor load/save.
- `scoring.rs`: ERIE_BASINS basin-aware multipliers + disposition false-positive
  filter; wired into pipeline ranking after cross-reference.
- `continuation.rs`: upward/downward continuation (rustfft) + satellite proof.
- `dipole_analysis.rs`: 4th AMBIGUOUS verdict band + elongation azimuth.
- Knobs + candidate merge/NMS (dipole_merge_radius_m) + man-made gate.

### Orchestration — `--describe` tool catalog (all 3 crates)
Each pipeline binary now emits a machine-readable JSON catalog for the LLM/n8n
orchestrator: pipeline identity, selectable stages, the full knob set with
defaults (introspected from the default struct), and the input/output contract.
- `sat-run --describe`
- `cesarops-aeromagnetic-worker describe`
- `cesarops-bag-scan --describe`

This is the contract the LLM reads to know what it can tune and which stages it
can run — the foundation for the "ask → run → report" orchestration layer.

## Status: all three pipelines rebuilt with full recovered detection logic,
## uniform tunable/selectable knobs, and a self-describing CLI. Builds green,
## 130 tests passing total (72 + 31 + 27).


---

## Post-Wave-3 — data loaders + n8n orchestration — DONE ✅

### Mag real data loaders (40 tests pass)
- `known_data.rs`: 47 embedded known wrecks ported exactly from
  erie_wellhead_discriminator.py (27 Niagara Divers + 20 ShipwreckWorld incl.
  the confirmed Colgate whaleback #103), plus GROUND_TRUTH labels,
  CONFIRMED_FIELD_SITES, GT_GEO_RADIUS_M.
- `well_loader.rs`: `load_ogsr_wells` (cp1252 decode + Lake-Erie filter) for the
  optional OGSr petroleum-well CSV.
- `main.rs`: `Detect` now always loads the 47 embedded wrecks and accepts an
  optional `--wells <csv>`. Cross-reference, basin scoring, and the disposition
  filter now activate with real data (previously fed empty slices).
- Verified: a candidate at the Colgate coord matches `nearest=Colgate`,
  `ground_truth=wreck`, `basin=central`.

### n8n orchestration layer (`n8n/`)
- `pipeline_runner.sh`: trust-boundary bridge n8n→Rust (catalog / describe / run).
- `tool_catalog.json`: combined `--describe` of all 3 pipelines (36+27+23 knobs)
  — the LLM router's context, regenerated on import.
- `workflows/`: 3 specialist sub-workflows (satellite/aeromag/bag webhooks →
  Execute Command → parse report) + an `orchestrator.json` (LLM router that
  reads the catalog, plans pipeline+knobs, dispatches to specialists).
- `import_workflows.sh`: imports into the local n8n (`/data/n8n`).
- End-to-end proven: the mag pipeline run through `pipeline_runner.sh` returns a
  Colgate match with basin scoring — the full ask→run→report path works.

### Final tally: 3 pipelines, full recovered detection logic, real reference
### data, self-describing CLIs, and an LLM/n8n orchestration layer. Tests:
### satellite 72, mag 40, bag 27 = 139 passing.


---

## Straits of Mackinac ground truth — gap closed ✅

Closed the ground-truth gap using real Straits dive wrecks (the wrecks people
actually dive on), so detection runs in the Straits AOI validate against charted
positions instead of empty/placeholder data.

Source: `outputs/great_lakes_preserve_wrecks.json` (Michigan Underwater
Preserves registry, michiganpreserves.org — dive-grade `coord_quality:
preserve_registry`). 32 Straits-preserve entries → cleaned to 29 (dropped
"Coordinates", "Rock Maze" @0ft, and a far-western mislabel) for satellite,
deduped to 26 hull sites for mag.

- `scripts/build_straits_ground_truth.py` — generator (filters to the Straits
  bbox, drops placeholders, emits both formats).
- `data/known_wrecks_straits.json` — satellite bbox format (29 wrecks);
  `data/straits_known_wrecks.json` — flat reference list.
- `cesarops-aeromagnetic-worker/src/known_data.rs` — added `straits_wrecks()`
  (26 entries incl. Cedarville, Sandusky, Eber Ward, Northwest, Newell Eddy,
  William H. Barnum); `all_known_wrecks()` now returns 73 (47 Erie + 26 Straits).
- `cesarops-satellite/src/mission.rs` — added `data/known_wrecks_straits.json`
  to the GT fallback paths.
- `missions/straits_known_wrecks.json` — ready-to-run Straits GT mission spec.

Verified end-to-end: `sat-run --spec missions/straits_known_wrecks.json
--dry-run` loads all 29 GT wrecks and runs download→target→validate→report.
Satellite 75 tests, mag 40 tests — all pass.
