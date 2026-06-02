# integrate/unmapped/laptopdump_wreckhunter_build/zion_trench_squeeze.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/missions/zion_trench_squeeze.py

## Steps
1. **Extract Configuration**: Move `ANDASTE_UTM` and `SENSORS` constants into a mission-specific YAML configuration file.
2. **Refactor Spatial Logic**: Move `generate_trench_grid` to `core/spatial/grid_utils.py` and implement proper `rasterio` geotransform logic to replace the "simplified" lat/lon approximation.
3. **Abstract Data Discovery**: Replace the hardcoded Windows paths in `find_sensor_tiffs` with a pipeline-native `DataDiscoveryService` call that queries the project's data lake/cache.
4. **Standardize Execution**: Replace the `subprocess` call in `process_tiff_gpu` with a standard `GPUComputeTask` call via the Forge orchestrator to ensure the `cesarops-gpu` engine is invoked within the containerized environment.
5. **Implement Result Sink**: Replace local `json.dump` with the pipeline's standard `ResultStore` API to ensure detections are indexed in the central database.

## Risks
* **Path Fragility**: The source contains hardcoded Windows-specific absolute paths (`C:\Users\thomf\...`) that will fail in a Linux-based pipeline.
* **Spatial Inaccuracy**: The current grid generation uses a simplified math approximation for lat/lon which will cause significant drift at high resolutions.
* **Binary Dependency**: The script relies on a relative path to a local `.exe` which is not portable without containerization.
* **Subprocess Overhead**: Using `subprocess.run` for GPU tasks bypasses the pipeline's resource scheduler and telemetry.
