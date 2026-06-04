# SWOT + ECOSTRESS Real-Client Spec — for the tuning engine

Hand this to your engine to jump-start the two real clients that replace the
neutral stubs. Conform to the EXISTING interface below so the result drops in
without refactoring. After the engine returns code, the agent reviews + adopts
selectively (same as the SIMD/jemalloc pass).

## Hard constraints (non-negotiable — match the existing build)
- **GDAL-FREE.** Pure-Rust only. NetCDF/HDF5 readers must NOT pull libgdal.
  SWOT = NetCDF4 (`.nc`), ECOSTRESS = HDF5/GeoTIFF. Use a pure-Rust reader
  (`netcdf` crate links libnetcdf-C — AVOID if it drags GDAL/HDF5 system libs;
  prefer `hdf5-metno` pure paths or a minimal NetCDF4/HDF5 byte reader, OR
  download the COG/GeoTIFF rendition and use our existing `crate::geotiff`).
- **Ivy Bridge safe.** No `target-cpu=native` assumptions; runtime SIMD only.
- **num-complex / ndarray 0.16 (features=["rayon"]).** Already deps.
- **Auth:** `NASA_EARTHDATA_TOKEN` env var (bearer). CMR search is the same
  pattern as `crate::stac::search_nasa_granules`. Download is auth-gated and
  OPTIONAL — the SCORE path must work from granule-search presence alone (like
  OPERA does today), so a missing token degrades to "search-only", not failure.
- **Never panic / never block the fusion.** On any error return
  `NEUTRAL_SENSOR_SCORE` (0.5) — fusion average must stay well-defined.

## Existing interface to conform to (cesarops-satellite/src/nasa_fusion.rs)
Replace these two stubs (keep the signatures; async is OK — see note):
```rust
pub const NEUTRAL_SENSOR_SCORE: f64 = 0.5;
pub fn fetch_swot_score(bbox: [f64; 4]) -> f64        // currently returns 0.5
pub fn fetch_ecostress_score(bbox: [f64; 4]) -> f64   // currently returns 0.5
```
`FusionScorer::score_clusters` calls them per SAR cluster with a ±0.1° bbox and
averages `(swot + ecostress + opera) / 3`. bbox order = [lon_min, lat_min,
lon_max, lat_max]. There is also a temporal window (start,end: chrono NaiveDate)
available — OPERA already takes it; SWOT/ECOSTRESS should too (add the param;
the agent will thread it through both call sites in nasa_fusion.rs).

CMR reference (already working for OPERA):
- endpoint: `https://cmr.earthdata.nasa.gov/search/granules.json`
- `crate::stac::search_nasa_granules(client, collection_concept_id, bbox, start, end, page_size)`
- OPERA collection id constant lives in stac.rs (`OPERA_DSWX_COLLECTION_ID`).

## SWOT — what we actually want from it (Great Lakes context)
SWOT = Surface Water and Ocean Topography (launched 2023, KaRIn, ~120 km swath,
~21-day repeat → SPARSE over any given lake point; expect frequent no-coverage).
- Collection: SWOT L2 LakeSP / Raster (PO.DAAC POCLOUD). Engine: find the right
  CMR concept-id for SWOT_L2_HR_Raster (or LakeSP) over inland water.
- SIGNAL we score on: water-surface-elevation (WSE) anomaly / slope over the
  cluster — a localized surface-height disturbance (seiche, current convergence
  over structure) is the wreck-relevant proxy. NOT bathymetry.
- SCORE 0..1: presence of a qualifying granule (coverage) → base 0.6; if the
  WSE field shows a local anomaly (|z| of WSE vs the surrounding water) above a
  threshold → up to 0.9; no coverage → NEUTRAL (0.5), NOT 0 (sparse ≠ negative).
- Because coverage is sparse, SWOT should be a BONUS signal, never a veto.

## ECOSTRESS — what we want (the MATERIAL/thermal corroborator)
ECOSTRESS = ISS thermal radiometer (~70 m, irregular ISS overpass, day+night).
- Collection: ECO2LSTE / ECO_L2T_LSTE (Land Surface Temp & Emissivity) via
  LP DAAC / AppEEARS. Engine: find the CMR concept-id; prefer the L2T tiled
  GeoTIFF rendition (reads through our `crate::geotiff` — GDAL-free).
- SIGNAL: surface-temperature anomaly at the cluster vs local background — the
  SAME cold-sink/heat-retention physics as our Landsat thermal concept, but a
  SECOND independent thermal instrument (cross-sensor thermal confirmation).
- SCORE 0..1: granule coverage → 0.6; local LST z-anomaly (cold OR hot) above
  threshold → up to 0.95; no coverage → NEUTRAL.
- This is the highest-value of the two for us: it can independently corroborate
  the Landsat thermal family at a candidate (true cross-sensor thermal lock).

## Output contract (both)
- Pure function-ish: `(bbox, start, end) -> f64 in [0,1]`, async OK.
- Log at debug on fallback; never unwrap/expect on network or parse.
- If you add a richer return (e.g. struct with the z-anomaly + granule id), put
  it behind a `_detailed` variant and keep the f64 entry point intact.

## Things to INCLUDE that we care about (domain, not just plumbing)
1. **Coverage-aware scoring** — sparse sensors (SWOT especially) must degrade to
   NEUTRAL on no-coverage, never to 0; absence of data ≠ absence of target.
2. **Local-anomaly z-score**, not absolute value — compute the cluster signal
   relative to a surrounding-water/background ring (same idea as our optical
   concepts' annular background). Scale-invariant, robust to calibration.
3. **Day/night tag for ECOSTRESS** — keep the overpass local solar time; deep
   targets read cold any time, shallow read hot-day/cold-night (see
   env_conditions::thermal_regime). Pass the regime hint if cheap.
4. **NoData/fill handling** — ECOSTRESS/SWOT carry fill values + QC layers; mask
   them before the z (our readers already turn sentinel/nonfinite → NaN).
5. **Temporal alignment** — accept the (start,end) window; for a multi-year
   stack, prefer the granule nearest the optical "perfect day", not just any.
6. **Cross-sensor thermal lock note** — if ECOSTRESS LST anomaly co-locates with
   the Landsat thermal_sink family within ~300 m, that's a genuine 2-instrument
   thermal agreement; expose enough (lat/lon + z) for the triple-lock to use it.

## What NOT to do
- No GDAL, no libgdal-linking NetCDF/HDF5 crates.
- No target-cpu=native.
- Don't make a missing token a hard error.
- Don't reproduce >30 consecutive words from NASA docs; paraphrase.
```
```
