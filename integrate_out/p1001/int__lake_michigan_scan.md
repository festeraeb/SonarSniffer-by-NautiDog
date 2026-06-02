# integrate/unmapped/laptopdump_wreckhunter_build/lake_michigan_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
`/codebase/projects/pipelines/wreckhunter/tasks/anomaly_detection_gpu.py`

## Steps
1.  **Refactor Core Logic**: Extract `process_tiff_with_coords` and `apply_anchor_calibration` into a reusable class `AnomalyDetector` within the `wreckhunter2000.core.detection` module.
2.  **Parameterize Inputs**: Replace hardcoded Windows paths (`C:\Users\thomf\...`) and `ANCHOR_POINTS` with a configuration object or CLI arguments compatible with the T440 pipeline runner.
3.  **Standardize Dependencies**: Ensure `cupy`, `rasterio`, `pyproj`, and `simplekml` are added to the pipeline's `requirements.txt`.
4.  **Implement Pipeline Task**: Create a new task wrapper that iterates through input directories, executes the GPU-accelerated Z-score detection, and handles the `rasterio.warp` transformation.
5.  **Output Integration**: Redirect KMZ/JSON output to the pipeline's standard `artifacts/` directory instead of a local `outputs/` folder.
6.  **Validation**: Run a test pass using a sample TIFF to verify `warp_transform` correctly maps local UTM coordinates to WGS84.

## Risks
* **VRAM Exhaustion**: Large TIFF files loaded via `cp.asarray(data)` may exceed M2200 memory; implement tiled processing if necessary.
* **Path Fragility**: The original script relies on specific local Windows paths which will fail in the Linux-based pipeline environment.
* **Dependency Drift**: The script's custom CUDA environment helper (`configure_cuda_environment`) must be reconciled with the pipeline's containerized CUDA runtime.
* **Coordinate Precision**: Reliance on `warp_transform` is robust, but the "Anchor Lock" logic is currently redundant/unused in the main loop and should be cleaned up to avoid confusion.
