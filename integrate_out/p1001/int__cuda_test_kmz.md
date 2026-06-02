# integrate/unmapped/laptopdump_wreckhunter_build/cuda_test_kmz.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/diagnostics/cuda_benchmark.py

## Steps
1.  **Refactor Paths**: Replace hardcoded Windows paths (`C:\Users\thomf\...`) with environment variables or a configurable `DATA_ROOT` parameter.
2.  **Update Imports**: Re-map `from database_connector import ...` to the canonical pipeline database module (e.g., `from cesarops.core.db import ...`).
3.  **Generalize Hardware**: Update the print statements and metadata to dynamically report the detected GPU (P100 vs M2200) rather than assuming M2200.
4.  **Dependency Management**: Add `cupy`, `simplekml`, and `Pillow` to the pipeline's `requirements.txt`.
5.  **Forge Integration**: Wire the script as an optional diagnostic task in the Forge tool for hardware validation post-deployment.

## Risks
* **Pathing**: Current script relies on local Windows directory structures which will fail in Linux-based pipeline environments.
* **CUDA Compatibility**: `cupy` installation is highly sensitive to the specific CUDA driver version installed on the T440 nodes.
* **Hardware Mismatch**: The script is tuned for a Quadro M2200; benchmark thresholds may need recalibration for the P100 fleet.
