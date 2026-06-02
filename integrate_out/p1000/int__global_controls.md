# integrate/unmapped/laptopdump_wreckhunter_build/global_controls.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/utils/scanner_ops.py

## Steps
1. Create `pipelines/wreckhunter/utils/scanner_ops.py`.
2. Port `GlobalScannerSettings` class to handle lake/target presets and VRAM management.
3. Port `generate_overlapping_tiles` and `stitch_tiles_back` functions for image processing.
4. Extract the `argparse` logic and `if __name__ == '__main__':` block into a new `pipelines/wreckhunter/cli.py` entry point.
5. Verify `geotransform` math in `generate_overlapping_tiles` using a dummy 6-tuple.
6. Run unit tests to ensure `stitch_tiles_back` correctly handles the feathering/weighting mask.

## Risks
* **Dependency:** Requires `cupy` and `numpy` to be present in the environment.
* **Hardware Lock:** `vram_settings` are hardcoded for M2200 (4GB); may need parameterization for higher-end GPUs.
* **Incomplete Source:** The `argparse` section is truncated in the provided dump.
* **Precision:** Tiling/stitching logic relies on correct `geotransform` input; errors here will break spatial mapping.
