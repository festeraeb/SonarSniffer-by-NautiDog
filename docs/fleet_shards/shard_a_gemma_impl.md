## Files to touch
- `cesarops-satellite/src/chip.rs`
- `cesarops-satellite/src/poc.rs`
- `cesarops-satellite/src/mission.rs`

## Code changes

### 1. `chip.rs`: Fix GDAL API (Step 1)
The current `decode_local_band` likely fails on `read_as` or `spatial_ref` signatures.
- **`bbox_to_pixel_window`**: Ensure it uses `gdal::Dataset::geo_transform` to convert WGS84 $\to$ UTM $\to$ Pixel.
- **`decode_local_band`**: 
    - Update `read_as` call to match GDAL 0.17: `dataset.read_as::<f32>(window_origin, window_size, out_size, resample)`.
    - Ensure `out_size` is `(rows, cols)` and `window_origin` is `(x, y)` (or `(col, row)` depending on your GDAL wrapper's convention).
    - Apply DN $\to$ Reflectance scaling: `val * scale_factor` (Sentinel-2 scale is typically 0.0001).
    - Return `Result<Vec<f32>, Error>`.

### 2. `poc.rs`: Implement `run_poc_aoi_local` (Step 2)
- **Function Signature**: `pub fn run_poc_aoi_local(bbox: BBox, scene_dir: PathBuf, knobs: &Knobs, known_wrecks: &KnownWrecks) -> Result<Vec<Candidate>, Error>`
- **Logic**:
    1. **Globbing**: Use `glob::glob` to find `*.blue.tif` and `*.green.tif` in `scene_dir`.
    2. **Parallelism**: Use `rayon`:
       ```rust
       let scene_results: Vec<SceneData> = scene_paths.par_iter().map(|path| {
           let b02 = decode_local_band(path.blue(), bbox, ...)?;
           let b03 = decode_local_band(path.green(), bbox, ...)?;
           Ok(SceneData { b02, b03, timestamp: ... })
       }).collect::<Result<Vec<_>, _>>()?;
       ```
    3. **Concept Adaptation**: Since target is deep, implement/call a `blue_green_clarity` concept (using B02/B03) instead of the standard B02/B04 (Red) version.
    4. **Temporal Stack**: Aggregate `scene_results` into a 3D array (Time $\times$ Y $\times$ X) to calculate z-scores/plume advection.
    5. **Cross-Ref**: Call `cross_reference()` against `known_wrecks`.

### 3. `mission.rs`: Wire the branch (Step 2)
- In `stage_poc_aoi`:
  ```rust
  if knobs.use_local_scenes {
      run_poc_aoi_local(bbox, paths.download_dir, knobs, known_wrecks)
  } else {
      run_poc_aoi(bbox, knobs, known_wrecks) // Existing STAC path
  }
  ```

## Compile / test commands
1. **Build & Verify GDAL**:
   ```bash
   cargo build --release -p cesarops-satellite --features gdal
   ```
2. **Calibration Run (Cedarville)**:
   ```bash
   /data/cargo-target/release/sat-run --spec data/missions/cedarville.json --root /data/cesarops/satellite_data
   ```
   *Note: Ensure `cedarville.json` has `use_local_scenes: true`.*

## Pitfalls
1. **GDAL Coordinate Mismatch**: If `bbox_to_pixel_window` doesn't account for the UTM projection of the Sentinel-2 tile, `decode_local_band` will return empty or offset data.
2. **Rayon/GDAL Thread Safety**: GDAL handles are not always thread-safe. Ensure `decode_local_band` opens the dataset *inside* the `par_iter` closure, not once outside.
3. **Memory Exhaustion**: 9 scenes $\times$ 120M pixels $\times$ 4 bytes (f32) $\approx$ 4.3GB. With 2 bands, this is ~8.6GB. Rayon threads might spike memory if many scenes are loaded simultaneously; consider `par_iter().map(...).collect()` carefully.
4. **Band Mapping**: Ensure `B02` is mapped to Blue and `B03` to Green. Swapping them will break clarity/glint math.
5. **Scale Factors**: Forgetting the 0.0001 scale factor on Sentinel-2 DNs will result in massive reflectance values, breaking z-score calculations.