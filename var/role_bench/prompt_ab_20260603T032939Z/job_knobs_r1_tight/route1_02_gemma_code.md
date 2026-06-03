The plan contains errors regarding knob locations and implementation logic. The knobs are defined in `src/types.rs`, not `knobs.pon.rs`. Additionally, the fix for the score bug requires capping the metric in `poc.rs` to ensure consistency with other concepts.

## Files touched
- `src/types.rs`: Update default values for `poc_zscore_threshold`, `poc_max_candidates`, `poc_min_separation_px`, `poc_downsample_max_dim`, and `min_score`.
- `src/poc.rs`: 
    - Modify `concept_blue_green_clarity` to cap z-scores at 10.0.
    - Modify `find_peak_clusters` to implement edge-case filtering.
    - Fix the `wreck_score` calculation to cap the raw metric between 0.0 and 10.0.

## Diff summary
- **`src/types.rs`**: Adjusting `Knobs` struct defaults to calibrate for clear-water environments (higher threshold, tighter separation, higher resolution).
- **`src/poc.rs`**: 
    - `concept_blue_green_clarity`: Added `z_score.min(10.0)` to prevent extreme outliers from skewing candidate selection.
    - `find_peak_clusters`: Added boundary check to ignore peaks within `poc_min_separation_px` of the image edge to reduce edge false positives.
    - `wreck_score`: Applied `clamp(0.0, 10.0)` to the raw metric to fix the bug where scores exceeded the standard 0-10 range.

## Verification commands
**Build:**
```bash
cargo build --release -p cesarops-satellite --features gdal
```

**Run (Calibration at Cedarville/Burns):**
*Ensure `data/missions/straits_local_run.json` contains the updated knob values and the target coordinates (45.7873,-84.6708 and 45.87127,-84.58642).*
```bash
/data/cargo-target/release/sat-run --spec data/missions/straits_local_run.json --root /data/cesarops/satellite_data
```

**Validation:**
1. Check logs for `poc_zscore` values; all should be $\le 10.0$.
2. Verify `wreck_score` in output JSON/logs is within $[0, 10]$.
3. Confirm candidate detection at Burns location within the 300m radius.