# integrate/unmapped/laptopdump_wreckhunter_build/test_repeatability.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/tests/test_repeatability.py

## Steps
1. Sanitize paths: Replace hardcoded `C:\Users\...` with `Path(os.environ.get("CESAROPS_TILE_DIR", "data/cache/census_raw"))` and `Path(os.environ.get("CESAROPS_OUTPUT_DIR", "outputs/repeatability_test"))`.
2. Fix missing logic: Implement GPU temperature retrieval in `execute_run` (e.g., via `pynvml` or `nvidia-smi` parsing) or remove the `gpu_temp` parameter from `log_run` if unavailable on target hardware.
3. Update CLI invocation: Replace `sys.executable, "-m", "cesarops_search"` with the live package entry point or ensure the live `cesarops_search` module is installed in the test environment.
4. Convert to pytest: Wrap `run_repeatability_test` in a `test_repeatability()` function. Use `pytest` fixtures for tile data availability and `tmp_path` for output directories.
5. Register in CI: Add to `pipeline_tests/` and update `pyproject.toml` / CI config to run this test against a staging tile before merge.

## Risks
- Hardcoded Windows paths and `nvidia-smi` dependency will fail on Linux/HPC nodes without adaptation.
- `cesarops_search` CLI interface may diverge from the live version; requires version pinning or live package installation in test env.
- SQLite logging in concurrent CI runners may cause file lock conflicts; switch to in-memory DB or unique temp paths per runner.
- Real tile data availability in CI/CD may require mounting or downloading from S3/GCS before test execution.
