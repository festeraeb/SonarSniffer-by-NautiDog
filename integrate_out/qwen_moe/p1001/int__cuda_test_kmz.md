# integrate/unmapped/laptopdump_wreckhunter_build/cuda_test_kmz.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/tests/benchmarks/cuda_test_kmz.py

## Steps
1.  Create directory `/codebase/projects/pipelines/tests/benchmarks/`.
2.  Move file to `/codebase/projects/pipelines/tests/benchmarks/cuda_test_kmz.py`.
3.  Refactor imports: Replace `from database_connector import ...` with `from cesarops.db.connector import ...` (or appropriate fleet module path).
4.  Remove hardcoded Windows paths (`C:\Users\thomf\...`). Replace with `argparse` arguments for `--input-dir` (TIFF search path) and `--output-dir`.
5.  Update `create_test_kmz` to accept `output_dir` as a parameter and handle path creation safely.
6.  Add `simplekml` to `requirements.txt` if not present.
7.  Verify `cupy` availability on target fleet nodes; add check in `main()` to exit gracefully if CUDA is unavailable.
8.  Run smoke test: `python tests/benchmarks/cuda_test_kmz.py --input-dir /tmp/test_tiffs --output-dir /tmp/test_output`.

## Risks
*   `cupy` version mismatch or missing CUDA drivers on fleet nodes.
*   `simplekml` dependency not installed in fleet environment.
*   Database schema changes may break `log_scan_run_to_census`.
*   Missing TIFF files in test environment will skip data processing block.
