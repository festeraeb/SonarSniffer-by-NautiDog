# integrate/unmapped/laptopdump_wreckhunter_build/zion_trench_squeeze.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/missions/zion_trench_squeeze.py

## Steps
1. Create directory `/codebase/projects/pipelines/missions/` if not present.
2. Copy `zion_trench_squeeze.py` to `/codebase/projects/pipelines/missions/zion_trench_squeeze.py`.
3. Refactor `find_sensor_tiffs` to replace hardcoded Windows paths with pipeline data catalog queries or mounted volume paths (e.g., `/data/sentinel_hunt/`, `/data/magnetic_data/`).
4. Refactor `process_tiff_gpu` to use the fleet's GPU service endpoint or containerized `cesarops-gpu` binary instead of local `target/release/cesarops-gpu.exe`.
5. Add `pytest` unit tests for `generate_trench_grid` to verify grid bounds and cell count for the 5km radius.
6. Add `pytest` unit tests for sensor configuration validation (ensure all required patterns are present).
7. Update pipeline manifest to request GPU resources and configure `cesarops-gpu` dependency.
8. Run integration test with sample TIFFs to verify anomaly detection pipeline.

## Risks
* GPU resource availability on target nodes for `cesarops-gpu` execution.
* TIFF format compatibility with `cesarops-gpu` (metadata, compression).
* Performance of `generate_trench_grid` (nested loops over 5km radius may be slow).
* Data latency for downloading large TIFFs from pipeline storage.
* Hardcoded `ANDASTE_UTM` limits reusability for other targets.
