# Agent Handoff → cesarops2

**Path:** `docs/AGENT_HANDOFF_cesarops2.md` (this file is the single source of truth.)

Date: 2026-06-02. From: T440 primary agent. To: cesarops2 (ML350e) agent.

## Agent: read this first

1. **Repo:** `/data/codebase/repos/wreckhunter2000-1` (rsynced from T440).
2. **Do not touch:** `cesarops-satellite/` and `cesarops-bag-scan/` — T440 owns those.
3. **Your scope only:** `nauticuvs/` (Task A) and new `cesarops-export/` (Task B).
4. **Build:** `CARGO_TARGET_DIR=/data/cargo-target` and `cargo build --release -p <your-crate>` (scoped; don’t rebuild the workspace).
5. **Git:** commit locally with clear messages; **do not push**.
6. **Everything else** (repro, file boundaries, data paths, acceptance tests) is below.

## Coordination rule (avoid collisions)
The T440 agent is actively editing **`cesarops-satellite/`** (poc.rs, mission.rs,
temporal.rs, poc/spectral GPU work) and **`cesarops-bag-scan/`**. DO NOT touch
those two crates. Your two tasks below are in **`nauticuvs/`** and a **new
`cesarops-export/`** crate — fully isolated, no overlap.

Build target dir is shared: `CARGO_TARGET_DIR=/data/cargo-target`. The repo on
cesarops2 is at `/data/codebase/repos/wreckhunter2000-1` (rsynced). Commit
locally only; do NOT push. Run `cargo build --release -p <crate>` scoped to your
crate so you don't rebuild the world.

---

## TASK A (HIGH): Fix the nauticuvs curvelet_inverse NaN bug

### Symptom
`curvelet_inverse` returns all-NaN even on a clean forward→inverse roundtrip.
Reproduce:
```
# 256x256 f32 raw input (8-byte header rows_u32_le cols_u32_le, then row-major f32)
/data/cargo-target/release/curvelet_bandpass \
  --input /tmp/nw_demeaned_256.raw --output /tmp/rt.raw \
  --scales 4 --keep-coarse --keep-fine
# => "Output: 256x256, std=NaN, peak=-inf"  (should be the input, near-identical)
```
The `curvelet_bandpass` binary (`nauticuvs/src/bin/curvelet_bandpass.rs`) is a
thin wrapper; the bug is in the library `nauticuvs/src/inverse.rs` (and possibly
`forward.rs`/`fft.rs`/`wrapping.rs`).

### What to do
1. Add a unit test in `nauticuvs/src/tests.rs`: forward then inverse of a known
   256x256 array (e.g. a 2D gaussian bump + constant) must reconstruct within
   relative L2 < 1e-4. The lib docs in `lib.rs` CLAIM "< 1e-6 for unmodified
   coefficients" — so this should already pass; find why it produces NaN.
2. Likely culprits: a divide-by-zero in the window normalization
   (`windows.rs`), an FFT plan size mismatch for non-power-of-2 or specific
   scale counts, or a `0/0` in the wrapping (`wrapping.rs`) when a subband is
   empty. Check for `NaN` introduced by `/ norm` where `norm == 0`.
3. Verify the fix with `scales` in {3,4,5} and sizes {128,256,512}.
4. This is the NATIVE full-precision build (f64 complex internally) — do NOT
   reduce precision. The published/detuned 32-bit version is separate; this is
   the real one.

### Why it matters
The BAG unmask reconstruction wants a curvelet band-pass to isolate hull-scale
structure (wreck) from the broad shoal (low freq) and noise (high freq). We
proved the wreck is there via direct elevation analysis (a ~110-130ft, 32ft-
relief intact wreck at 45.87127°N, -84.58642°W in a NOAA-masked zone), but the
curvelet sharpening pass is blocked by this NaN bug.

---

## TASK B (MED): Build `cesarops-export` — detections → Google Earth + auto-sync DB

### Goal
A new Rust crate that turns detection outputs into a field-ready target map:
- **KMZ for Google Earth**: one placemark per candidate at its lat/lon, with a
  popup showing metrics (size, depth, relief, confidence, signature_type,
  source) AND an embedded PNG thumbnail (the hillshade/recon image).
- **SQLite auto-sync DB**: a `candidates` table that upserts on (lat,lon,source)
  so re-runs update rather than duplicate. This is the persistent target list.
- Re-generate the KMZ from the DB on each sync so Google Earth (auto-refresh
  network link) always shows the current set.

### Inputs (these files ALREADY EXIST — read them, don't regenerate)
- BAG detections JSON: `cesarops-bag-scan` MissionReport — array of
  `detections[]` each with `latitude, longitude, signature_type, size_sq_feet,
  long_side_ft, short_side_ft, depth_meters, height_above_floor_m, confidence,
  object_type, metadata{mask_type, depth_anomaly_ft, ...}`. Example on disk:
  `/data/cesarops/bathymetry/scan_results/H13255_4m_full.json`
- Unmask rasters (for thumbnails): `/data/cesarops/bathymetry/unmask_out/H13255_forced/*_hillshade.tif`
  and `*_recon.tif` (georeferenced GeoTIFF, UTM 16N / EPSG:6345).
- Ground truth: `/home/cesarops/wreckhunter2000-1/scripts/known_wrecks_straits.json`
  (dict keyed by id; each has name, lat_min/max, lon_min/max, type, confidence, source).
- Satellite candidates (when ready): `cesarops-satellite` MissionReport
  `candidates[]` with `lat, lon, composite_score, best_concept, signals{}, notes`.

### Crate layout
```
cesarops-export/
  Cargo.toml          # deps: serde, serde_json, rusqlite (bundled), gdal (0.17,
                      #   for reading hillshade tif), image (PNG thumbnail), clap
  src/main.rs         # CLI: ingest <json...> --db <path> --kmz <path> [--thumbs-dir <dir>]
  src/db.rs           # rusqlite schema + upsert
  src/kml.rs          # KMZ writer (KML + embedded PNGs in a zip)
  src/thumb.rs        # GeoTIFF window -> colored/scaled PNG thumbnail
  src/model.rs        # unified Candidate struct both pipelines map into
```

### Unified Candidate (the contract — match these field names)
```rust
pub struct ExportCandidate {
    pub id: String,            // stable: e.g. "bag:H13255_mask064" or "sat:elva"
    pub lat: f64,
    pub lon: f64,
    pub source: String,        // "bag_physical" | "bag_masked" | "satellite" | "ground_truth"
    pub confidence: f64,
    pub long_ft: Option<f64>,
    pub short_ft: Option<f64>,
    pub depth_ft: Option<f64>,
    pub relief_ft: Option<f64>,
    pub signature: Option<String>,   // physical_wreck | masked_redaction_flat | concept name
    pub notes: String,
    pub thumb_png: Option<String>,   // path to thumbnail in the kmz
    pub metrics_json: String,        // full original metrics as JSON string
}
```

### SQLite schema (db.rs)
```sql
CREATE TABLE IF NOT EXISTS candidates (
  id TEXT PRIMARY KEY,
  lat REAL, lon REAL, source TEXT, confidence REAL,
  long_ft REAL, short_ft REAL, depth_ft REAL, relief_ft REAL,
  signature TEXT, notes TEXT, thumb_png TEXT, metrics_json TEXT,
  first_seen TEXT, last_seen TEXT
);
```
Upsert on `id`: update last_seen + metrics on conflict, keep first_seen.

### KMZ specifics
- Use folders by source (Physical / Masked / Satellite / GroundTruth) so they
  toggle in Google Earth's sidebar.
- Color placemarks by source: ground_truth=green, bag_physical=yellow,
  bag_masked=red (deliberate hides = highest interest), satellite=blue.
- Popup `<description>` = an HTML table of metrics + `<img src="files/<thumb>.png">`.
- KMZ = zip with `doc.kml` at root + `files/*.png`.
- Also emit a `network_link.kml` that points at the kmz with
  `<refreshMode>onInterval</refreshMode><refreshInterval>60</refreshInterval>`
  so Google Earth auto-syncs.

### Thumbnail (thumb.rs)
For a candidate with a hillshade tif, read an ~256x256 window centered on its
lat/lon (convert lat/lon -> pixel via the tif geotransform), apply a grayscale
(hillshade) or viridis-ish ramp (recon), save PNG. If no tif, skip thumb.

### Acceptance test
```
cesarops-export ingest /data/cesarops/bathymetry/scan_results/H13255_4m_full.json \
  --db /data/cesarops/targets.db \
  --kmz /data/cesarops/straits_targets.kmz \
  --thumbs-dir /data/cesarops/bathymetry/unmask_out/H13255_forced
# Open straits_targets.kmz in Google Earth -> placemarks at each detection,
# red for masked, with metric popups. Re-run -> no duplicates (upsert).
```

### Notes
- `rusqlite = { version = "0.31", features = ["bundled"] }` avoids needing
  system sqlite.
- Add `cesarops-export` to the workspace `members` in root `Cargo.toml`
  (that root file IS shared — append one line to `members`, that's the only
  shared edit; coordinate timing if needed, it's a trivial append).
- Commit locally with a clear message. Do NOT push.

---

## Current state (T440 side, for context)
- Downloads complete: 17 Sentinel-2 scenes (2022/23/24, tile 16TFR Straits),
  6 Landsat LC08/09 bundles, 120 SWOT + 120 ICESat-2 ATL13, 1 Sentinel-1 RTC.
- P100s freed (llama-servers killed) — 32GB GPU available for satellite compute.
- BAG pipeline: perf-fixed (∞→17s), unmask engine built, found intact wreck at
  45.87127°N -84.58642°W in a NOAA-masked zone east of the Elva.
- T440 agent now: adding local-scene loader + rayon to cesarops-satellite poc.rs
  so it runs on the on-disk tiles, then CUDA temporal stack.
