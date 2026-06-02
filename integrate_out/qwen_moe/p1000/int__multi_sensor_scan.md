# integrate/unmapped/laptopdump_wreckhunter_build/multi_sensor_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/sensor_scan/multi_sensor_scan.py

## Steps
1.  **Refactor Path Resolution**: Replace hardcoded `C:\Users\thomf\...` paths in `find_sensor_tiffs` with pipeline configuration parameters (e.g., `config.sensor_roots`) and environment variables. Ensure cross-platform compatibility (Linux fleet nodes).
2.  **Wire GPU Engine**: Replace `subprocess.run(["cesarops-gpu.exe", ...])` in `process_tiff_gpu` with a Forge tool wire call to `cesarops-gpu` or a dedicated `gpu_engine` module. This decouples the scan logic from the binary location and handles error propagation properly.
3.  **Fix Geolocation**: Replace simplified `pixel_to_coordinates` math with `pyproj` or `rasterio` transformations for accurate UTM/WGS84 conversion. The current approximation introduces significant drift over Lake Michigan extents.
4.  **Add Logging**: Replace `print()` statements with `logging` module calls for fleet observability and log aggregation.
5.  **Pipeline Definition**: Create `/codebase/projects/pipelines/sensor_scan/multi_sensor_scan.yaml` to define the workflow:
    *   `input`: Sensor data manifests or glob patterns.
    *   `process`: Invoke `multi_sensor_scan.py` with threshold configs.
    *   `output`: JSON results to S3/GCS artifact store.
6.  **Testing**: Add unit tests mocking `cesarops-gpu` output to verify anomaly parsing, coordinate conversion, and JSON export logic.

## Risks
*   **Platform Dependency**: Original script targets Windows (`C:\`, `.exe`). Fleet runs on Linux; binary distribution and path handling must be updated.
*   **GPU Availability**: Hardcoded "Quadro M2200" check implies specific hardware. Pipeline must handle nodes without GPU or fallback to CPU mode if supported.
*   **Coordinate Drift**: Simplified UTM math is insufficient for forensic-grade geolocation. Must use standard projection libraries.
*   **Scope Creep**: "ALL sensor types" may exceed pipeline resource limits. Consider splitting into per-sensor pipelines or adding chunking.
*   **Binary Distribution**: `cesarops-gpu.exe` must be built and available in the pipeline environment (e.g., via Docker image or artifact cache).
