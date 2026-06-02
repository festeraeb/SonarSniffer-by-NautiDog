# integrate/unmapped/laptopdump_wreckhunter_build/test_repeatability.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/tests/integration/test_repeatability.py

## Steps
1. Create `tests/integration/` directory in the pipeline repo.
2. Refactor `TEST_TILE_DIR` and `OUTPUT_DIR` to use `argparse` or `os.getenv` instead of hardcoded Windows paths.
3. Replace all `Path(r"C:\...")` instances with `pathlib` relative paths or environment-driven paths.
4. Update the `subprocess` call to ensure `sys.executable -m cesarops_search` correctly references the installed package in the pipeline environment.
5. Wrap the execution in a `pytest` compatible function to allow integration into the standard test suite.
6. Add a configuration file (YAML/JSON) to define the `TEST_TILE` and `MAX_DRIFT` parameters for different environments.

## Risks
* **Data Dependency**: The test requires specific HLS datasets; CI/CD pipelines must have a mechanism to fetch or mount these large files.
* **Compute/Time**: Running 6 heavy processing cycles (3 chunked, 3 full) will significantly increase test duration and may hit CI timeout limits.
* **Environment Mismatch**: The script relies on `numpy` and `sqlite3`; ensure the pipeline environment matches the local dev environment.
* **Pathing**: Failure to fully strip Windows-specific pathing logic will cause immediate execution failure on Linux runners.
