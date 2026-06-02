# integrate/unmapped/laptopdump_wreckhunter_build/full_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/full_scan.py

## Steps
1. **Refactor Path Logic**: Replace hardcoded Windows paths (`C:\Users\thomf\...`) with `argparse` inputs or environment variables to support containerized execution.
2. **Upgrade Geolocation Math**: Replace the simplified `utm_to_wgs84` and `pixel_to_utm` functions with `pyproj` implementation to ensure sub-meter accuracy.
3. **Implement Anchor-Lock**: Convert the `apply_anchor_lock_correction` stub into a functional module that calculates the delta between detected land features and `ANCHOR_POINTS`.
4. **Standardize GPU Interface**: Wrap the `subprocess` call to `cesarops-gpu.exe` into a formal `Task` runner compatible with the T440 orchestration layer.
5. **Schema Alignment**: Update the JSON export schema to match the standard CESAROPS telemetry format for anomaly detections.
6. **Test Suite**: Create a test harness using a sample TIFF to verify the Z-score parsing and coordinate transformation.

## Risks
* **Inaccurate Geolocation**: The current "rough correction" math is insufficient for maritime wreck locating; requires `pyproj`.
* **Environment Dependency**: The script relies on a specific local binary (`cesarops-gpu.exe`) and specific hardware (M2200) which must be mapped in the pipeline environment.
* **Stubbed Logic**: The "Anchor-Lock" is currently a no-op; integrating it without completing the math will result in false positives/misplaced detections.
