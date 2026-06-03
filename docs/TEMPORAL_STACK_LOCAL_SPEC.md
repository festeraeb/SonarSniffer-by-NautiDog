# Temporal Stack Local-Tile Spec (for fleet execution)

## What's done
The satellite pipeline (`sat-run`) runs offline on local Sentinel-2 tiles with
rayon parallelism. The `poc_aoi` stage works: it loads 9 scenes, runs
`blue_green_clarity(B02/B03)`, and confirmed Cedarville at 190m in 5.5s.

## What's needed
The `temporal_stack` stage still hits STAC (network) instead of reading local
tiles. It needs the same offline branch the POC stage got. The Burns target at
34m depth needs multi-year temporal persistence (not single-scene clarity) to
show the plume-advection signal.

## The ONE task
Wire `stage_temporal_stack` in `mission.rs` to use local tiles when
`knobs.use_local_scenes == true`, parallelize with rayon, and run the z-score
across ALL available dates (2022 + 2023 + 2024 = 17 scenes).

---

## File: `cesarops-satellite/src/temporal.rs`

### Current state
`temporal.rs` has a function that does a STAC search, downloads scenes, computes
per-pixel NDWI/NDVI across dates, and z-scores each pixel against the temporal
mean. Pixels with persistent anomaly (high |z|) across many dates = candidate.

### What to add
A `run_temporal_stack_local` function that:

1. **Discovers ALL local scenes** across multiple directories:
   ```
   data/straits_optical_clear/sentinel2_aws/   (9 scenes, Sept 2024)
   data/straits_optical_2022/sentinel2_aws/    (4 scenes)
   data/straits_optical_2023/sentinel2_aws/    (4 scenes)
   ```
   Glob for `*.blue.tif` in each, extract scene IDs + dates from filename
   (`S2X_TILE_YYYYMMDD_N_L2A`).

2. **Load B02 (blue) + B03 (green) per scene** using `chip::decode_local_band`
   (already works, proven in POC stage). Use rayon `par_iter` — each scene
   opens its own GDAL handle (thread-safe when opened inside the closure).

3. **Build a temporal cube**: `[n_dates × rows × cols]` for each band.
   At target_px=2048, each scene = 2048×2048×4 bytes = 16MB. 17 scenes × 2
   bands = 544MB — fits easily in RAM (96GB available).

4. **Compute per-pixel temporal z-score**:
   ```
   For each pixel (r, c):
     values_across_dates = cube[:, r, c]  (finite values only)
     if count(finite) >= 5:
       mean = mean(values)
       std = std(values)
       zscore[r, c] = (latest_value - mean) / std
   ```
   Actually for wreck detection: compute the CLARITY RATIO per date
   (`ln(B02)/ln(B03)`), then z-score that ratio across dates. A wreck location
   shows PERSISTENTLY LOW clarity (negative z across most dates) because the
   column disturbance is always there regardless of season/weather.

5. **Persistence score**: instead of just z-score of the latest date, count
   HOW MANY dates each pixel is anomalous (z < -1.5). A wreck plume is
   persistent (most dates); a cloud/weather event is transient (1-2 dates).
   ```
   persistence[r, c] = count(z_per_date[:, r, c] < -1.5) / n_dates
   ```
   Pixels with persistence > 0.5 (anomalous on >50% of dates) = strong signal.

6. **Peak extraction**: use `find_peak_clusters` on the persistence map
   (same as POC stage). Return candidates with `concept = "temporal_persistence"`.

7. **Cross-reference** against known wrecks (same `cross_reference()` call).

### Key insight (from user's physics)
The wreck "appears to move" between dates because the plume rises to the
thermocline and drifts with the current. In a STACK, these shifted appearances
all cluster near the wreck — they DON'T cancel, they REINFORCE in a fuzzy blob
around the true position. That's why persistence works: the wreck is ALWAYS
generating a plume, it just lands at slightly different surface positions.

---

## File: `cesarops-satellite/src/mission.rs`

In `stage_temporal_stack` (line ~570), add:
```rust
if knobs.use_local_scenes.unwrap_or(false) {
    // Collect all local scene dirs
    let scene_dirs = vec![
        paths.download_dir.clone(),
        // Also check for 2022/2023 dirs adjacent to download_dir
        paths.download_dir.parent().unwrap_or(&paths.download_dir)
            .join("straits_optical_2022/sentinel2_aws"),
        paths.download_dir.parent().unwrap_or(&paths.download_dir)
            .join("straits_optical_2023/sentinel2_aws"),
    ];
    let result = crate::temporal::run_temporal_stack_local(
        &scene_dirs, bbox, knobs, &known_wrecks, target_px
    )?;
    // Write result + return stage JSON
}
```

---

## File: `cesarops-satellite/src/types.rs`

No changes needed — `use_local_scenes` and `downsample_max_dim` already exist.

---

## Acceptance test

```bash
/data/cargo-target/release/sat-run \
  --spec data/missions/straits_local_run.json \
  --root /data/cesarops/satellite_data
```

Check `detection_runs/straits_local_2024/temporal_stack/` for:
- `temporal_persistence_map.json` (or similar) with per-pixel persistence scores
- Candidates near **45.87127°N, -84.58642°W** (the Burns target)
  - Any candidate within 300m with persistence > 0.3 = cross-sensor confirmation
  - The plume-advection drift means the centroid may be offset up to 200m from
    the BAG bottom position

## Build command
```bash
cargo build --release -p cesarops-satellite --features gdal
```

## Data paths (all on T440 raid0, accessible from c2 via /mnt/raid0)
- `/mnt/raid0/wreckhunter2000-1-data/data/straits_optical_clear/sentinel2_aws/` (9 scenes)
- `/mnt/raid0/wreckhunter2000-1-data/data/straits_optical_2022/sentinel2_aws/` (4 scenes)
- `/mnt/raid0/wreckhunter2000-1-data/data/straits_optical_2023/sentinel2_aws/` (4 scenes)

## Do NOT
- Don't hit the network / STAC for this stage (use local tiles only)
- Don't use red (B04) or NIR (B08) — they don't penetrate to 34m
- Don't use ndarray-npy (broken with current ndarray version)
- Don't expect a STRONG single-pixel hit — expect a fuzzy persistence blob
  centered ~within 300m of the BAG coordinate
