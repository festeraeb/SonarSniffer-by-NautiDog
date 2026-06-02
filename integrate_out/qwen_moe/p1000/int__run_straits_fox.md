# integrate/unmapped/laptopdump_wreckhunter_build/run_straits_fox.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/run_straits_fox.py

## Steps
1. Copy file to target path.
2. Replace `WH2K = REPO / 'wreckhunter2000'` with fleet-configurable path or `Path(__file__).parent / 'outputs'`.
3. Remove all `input()` calls; replace with `logging.info()` or exit codes for automation.
4. Add `h5py`, `numpy`, `rasterio`, `requests` to fleet requirements manifest.
5. Register runner in `pipeline_registry.yaml` under `wreckhunter` domain.
6. Add unit tests for `check_deps()` and `run_script()` mocking `subprocess.run`.
7. Validate subprocess error propagation and logging output.

## Risks
- `input()` blocks will crash in CI/CD or fleet execution.
- Hardcoded `WH2K` path breaks in containerized fleet environments.
- `rasterio` and `h5py` may require system-level dependencies (GDAL, HDF5) not present in base fleet.
- Subprocess failures swallow stderr; add `capture_output=True` and logging.
- Missing validation of output directory existence before writing results.
