# integrate/unmapped/laptopdump_wreckhunter_build/cuda_direct.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter2000/scripts/tools/cuda_direct.py

## Steps
1.  **Directory Setup**: Create `wreckhunter2000/scripts/tools/` if not present.
2.  **File Placement**: Move `cuda_direct.py` to the target path.
3.  **Refactor `main`**:
    *   Remove hardcoded Windows paths (`C:\Users\...`).
    *   Replace with CLI arguments (`argparse`) for `input_dir`, `output_dir`, and `threshold`.
    *   Remove `install_cupy()` function; dependencies must be managed by the fleet environment.
4.  **Dependency Management**:
    *   Add `cupy-cuda13x` to `wreckhunter2000/requirements.txt` or fleet Dockerfile.
    *   Verify CUDA 13 compatibility with fleet driver versions.
5.  **Pipeline Integration**:
    *   Register `cuda_direct.py` as a CLI entry point in `wreckhunter2000/pyproject.toml` or `setup.py`.
    *   Create a pipeline step definition (e.g., `gpu_anomaly_scan`) that calls this script with appropriate inputs.
6.  **Testing**:
    *   Verify import of `wreckhunter2000.scripts.tools.cuda_env` resolves correctly.
    *   Test with a small dummy TIFF to ensure CuPy initialization works in the fleet environment.

## Risks
*   **Hardware Specificity**: Script targets "Quadro M2200". Fleet may have different GPUs; `cp.cuda.Device(0)` may fail or yield different capabilities.
*   **CUDA Version**: `cupy-cuda13x` requires CUDA 13 toolkit. Fleet must have compatible drivers and toolkit installed.
*   **Import Path**: `wreckhunter2000.scripts.tools.cuda_env` must exist and be importable in the target environment.
*   **Memory**: Large TIFFs may exceed M2200 VRAM (4GB/8GB); `cp.asarray` could OOM. Consider chunking or memory checks.
