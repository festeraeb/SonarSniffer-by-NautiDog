# integrate/unmapped/laptopdump_wreckhunter_build/cesarops_engine.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/engine/compute_ops.py

## Steps
1. **Extract Compute Logic**: Move `gpu_process_tile` (the CuPy Z-score implementation) into `compute_ops.py`.
2. **Refactor TPU Integration**: Move `tpu_glint_check` into the pipeline's pre-processing module, ensuring `TPUClient` uses environment-based URLs instead of hardcoded `localhost`.
3. **Standardize Schema**: Map the `anomaly_hits` SQLite schema to the production PostgreSQL/TimescaleDB schema used in the T440 fleet.
4. **Decouple IO**: Remove all hardcoded Windows paths (`C:\Users\...`) and replace with a configuration provider (e.g., `config.yaml` or env vars).
5. **Dependency Management**: Add `cupy-cuda12x` (or appropriate version) to `requirements.txt` to support the CUDA backend.
6. **Unit Test**: Create a test suite using `pytest` that mocks the `TPUClient` and validates the Z-score math against known `numpy` outputs.

## Risks
* **Hardware Dependency**: The `cupy` implementation requires specific NVIDIA drivers; fallback to `numpy` must be robustly handled in the production pipeline.
* **Path Fragility**: The original script relies heavily on local Windows paths; failure to fully strip these will break containerized execution.
* **Database Schema Mismatch**: The local SQLite schema is a "minimal" version; direct migration of `write_detection` logic without mapping to the production schema will cause ingestion errors.
