# integrate/unmapped/laptopdump_wreckhunter_build/test_ai_callable.py

## Verdict
PORT_TO_PIPELINES

## Target path
`/codebase/projects/pipelines/tests/test_ai_callable_demo.py`

## Steps
1. Move file to `tests/` directory and rename to `test_ai_callable_demo.py`.
2. Remove `sys.path.insert(0, ...)` hack; replace with standard absolute imports relative to the project root.
3. Update `GlobalScannerSettings` and `wreckhunter2000` imports to match the production package structure.
4. Wrap the demonstration logic in a `pytest` function or a `if __name__ == "__main__":` block to prevent execution during standard test discovery.
5. Ensure `outputs/` directory creation logic uses `Path(__file__).parent / "outputs"` to ensure portability.
6. Verify that `DetectionSettings` and `CurveletSettings` classes exist in the current `wreckhunter2000` implementation.

## Risks
* **Dependency Drift**: The script assumes `torch`, `rasterio`, and `scipy` are available; integration tests may fail in minimal CI environments.
* **Pathing**: The script uses relative pathing for `outputs/` which can cause issues if run from the project root vs the test directory.
* **API Mismatch**: The `GlobalScannerSettings.update_for_lake` and `update_for_target` methods must exist in the live codebase for this demo to be valid.
