# integrate/unmapped/laptopdump_wreckhunter_build/validate_detection.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/tests/validate_detection.py

## Steps
1. Create directory `/codebase/projects/pipelines/wreckhunter/tests/`.
2. Refactor `TILE_DIR` and `OUTPUT_DIR` to accept command-line arguments via `argparse` instead of hardcoded `Path` objects.
3. Replace the simplified `brightness_temp` calculation with a standard calibration constant application if `HLS` metadata is available in the pipeline.
4. Update `latlon_to_pixel` to use `rasterio` for actual geotransform handling to ensure the Z-score check is spatially localized to the target rather than just checking the tile maximum.
5. Add `numpy` and `Pillow` to `wreckhunter/requirements.txt`.
6. Add a test entry in the project's `forge.yaml` to allow running this validation as a post-processing check.

## Risks
* **Data Dependency**: Script will fail if the specific HLS `.tif` files are not present in the mapped volume.
* **Mathematical Imprecision**: The current `(b10 + b11) / 2` logic is a placeholder and may not match the original detection's Z-score if the original used proper thermal calibration.
* **Spatial Drift**: The "approximate" pixel conversion may miss the target if the geotransform is not correctly implemented.
