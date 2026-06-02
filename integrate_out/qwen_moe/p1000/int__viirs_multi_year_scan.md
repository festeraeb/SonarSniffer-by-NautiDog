# integrate/unmapped/laptopdump_wreckhunter_build/viirs_multi_year_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/forensics/viirs_multi_year_scan.py

## Steps
1.  **Refactor Paths**: Replace hardcoded `c:\Users\thomf\...` with `argparse` arguments or environment variables (`VIIRS_DATA_DIR`, `OUTPUT_DIR`).
2.  **GPU Management**: Remove `cp.cuda.Device(1).use()`. Implement dynamic GPU selection via fleet config or `os.environ` to avoid hardware lock-in.
3.  **Dependencies**: Add `simplekml` to `requirements.txt` if not present. Verify `rasterio` and `cupy` versions match fleet.
4.  **CLI Interface**: Add `argparse` for `--input-dir`, `--output-dir`, `--zscore-tolerance`, `--location-tolerance`.
5.  **Testing**: Create `test_viirs_multi_year_scan.py` with mock `rasterio` data and `cupy` fallback verification.
6.  **Pipeline Definition**: Create `pipeline.yaml` defining the job, resource requirements (GPU), and data mounts.
7.  **Documentation**: Update `GPU_WORKFLOW.md` and `README.md` for the new pipeline.

## Risks
*   **GPU Lock-in**: Script forces GPU1; fleet may have different GPU topologies.
*   **Data Volume**: Multi-year processing may exceed memory/time limits; consider chunking or distributed processing.
*   **Dependency Conflicts**: `simplekml` may not be in base fleet image.
*   **Path Mapping**: Windows paths must be fully abstracted for Linux fleet execution.
