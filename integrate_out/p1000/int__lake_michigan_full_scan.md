# integrate/unmapped/laptopdump_wreckhunter_build/lake_michigan_full_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/scanners/lake_michigan_multi_sensor.py

## Steps
1. **Extract Calibration Logic**: Move `ANCHOR_POINTS` and `apply_anchor_calibration` to `core/utils/geo_calibration.py`.
2. **Port Spectral Processors**: Port `process_thermal_band` and `process_optical_band` into `core/processors/spectral_analysis.py`, ensuring they utilize the standard `GlobalScannerSettings` object.
3. **Integrate Fusion Engine**: Refactor the `fuse_multi_sensor_detections` logic into `core/fusion/multi_sensor_engine.py` to allow for standardized cluster-based detection.
4. **Implement Pipeline Task**: Create the new task file at the target path, replacing hardcoded `wreckhunter2000` paths with the pipeline's `DataRegistry` and `AssetLoader`.
5. **Wire Forge Tool**: Add the new task to the Forge orchestration manifest for automated execution on the T440 fleet.

## Risks
* **Hardcoded Paths**: The source relies on local laptop paths (`wreckhunter2000\...`) which must be mapped to the pipeline's data lake.
* **Hardware Dependency**: The script's performance relies heavily on `cupy` (GPU); fallback to CPU must be verified for standard nodes.
* **Calibration Accuracy**: The "Anchor-Lock" math is a simple distance-weighted blend; it requires validation against high-precision GPS benchmarks before production use.
* **Truncated Logic**: The provided source is truncated; the full fusion logic must be recovered from the original dump.
