# integrate/unmapped/laptopdump_wreckhunter_build/lake_michigan_full_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/lake_michigan_scan/lake_michigan_full_scan.py

## Steps
1. Relocate file to target path.
2. Refactor `run_full_lake_scan` to accept a `forge` config object; replace `wreckhunter2000\data\cache\census_raw` with `config.data_dirs.lake_michigan` and `config.output_dirs.scan_results`.
3. Wire `fuse_multi_sensor_detections` output to `forge` `detection_store` and `kmz_export` tools; replace manual `json.dump` with `forge` serialization.
4. Update `process_thermal_band` and `process_optical_band` to use `forge` `rasterio_wrapper` for consistent geotransform handling, nodata masking, and chunked reads.
5. Add unit tests for `apply_anchor_calibration` (edge distances, invalid anchors), `fuse_multi_sensor_detections` (grid clustering, confidence weighting), and mock `rasterio` reads for Z-score boundaries.
6. Add integration test with synthetic 512x512 float32 TIFFs to verify overlap stitching, threshold application, and multi-sensor fusion logic.
7. Audit `GlobalScannerSettings` API; align `update_for_lake`, `sensitivity`, and `vram_settings` with fleet standard or replace with `forge` pipeline config schema.

## Risks
- Hardcoded Windows paths (`r"..."`) will fail on Linux/Unix fleet nodes; requires cross-platform `pathlib` resolution.
- `fuse_multi_sensor_detections` truncates to top 1000 detections; may discard valid multi-sensor hits in high-density regions.
- Optional `cupy`/`rasterio` imports lack explicit fallback routing for pipeline scheduling; may cause silent failures or OOM on constrained nodes.
- KMZ export logic is missing/truncated; requires `forge` `kmz_builder` integration or removal from docstring promises.
- `GlobalScannerSettings` may not match fleet config schema; verify `to_dict` serialization compatibility with `forge` state tracking.
