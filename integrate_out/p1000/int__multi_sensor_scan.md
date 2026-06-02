# integrate/unmapped/laptopdump_wreckhunter_build/multi_sensor_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/lake_michigan_multi_sensor_scan.py

## Steps
1. **Abstract Configuration**: Move `SENSOR_CONFIGS` and `ANCHOR_POINTS` into a `config.yaml` or a dedicated `PipelineConfig` class to remove hardcoded constants.
2. **Parameterize Paths**: Refactor `find_sensor_tiffs` to accept a `root_search_path` argument via CLI, replacing the hardcoded `C:\Users\thomf\...` Windows paths.
3. **Modularize Geolocation**: Move `pixel_to_coordinates` to `/codebase/core/geo_utils.py` to allow reuse across the fleet.
4. **Decouple Binary Execution**: Update `process_tiff_gpu` to accept the path to `cesarops-gpu` as an argument rather than assuming a relative `target/release/` path.
5. **Implement CLI**: Use `argparse` to allow the pipeline to be triggered with `--input-dir`, `--output-dir`, and `--gpu-bin`.
6. **Validation**: Add unit tests for the `pixel_to_coordinates` function to ensure the "simplified" math meets precision requirements for the T440 fleet.

## Risks
* **Environment Mismatch**: The current script is heavily tied to a Windows local environment; failure to abstract paths will break the pipeline in Linux/Docker containers.
* **Coordinate Drift**: The `pixel_to_coordinates` function uses a "simplified" math model; this may cause significant geolocation errors if not replaced with a proper PROJ-based transformation.
* **Binary Dependency**: The pipeline relies on a specific compiled binary (`cesarops-gpu.exe`) which must be present in the execution environment.
