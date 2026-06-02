# integrate/unmapped/laptopdump_wreckhunter_build/test_m2200_minimal.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/cesarops-gpu/tests/test_m2200_minimal.py

## Steps
1. Move the script to the `cesarops-gpu` test directory.
2. Refactor `create_small_test_tiff` to use `pytest`'s `tmp_path` fixture instead of hardcoded local paths to ensure clean test execution.
3. Update `run_gpu_test` to resolve the binary path dynamically using `pytest`'s `rootdir` or an environment variable (e.g., `CESAROPS_BIN_PATH`) rather than a hardcoded `target/release/` relative path.
4. Wrap the `main()` logic into a standard `pytest` function decorated with `@pytest.mark.skipif(not gpu_available, reason="M2200 hardware not detected")`.
5. Add `numpy` and `Pillow` to the `requirements-dev.txt` of the GPU pipeline.

## Risks
* **Hardware Dependency:** The test will fail on standard CI runners lacking a Quadro M2200; must implement hardware detection logic to skip rather than fail.
* **Path Fragility:** The current script assumes a specific build directory structure (`target/release/`); integration requires a robust way to locate the compiled Rust artifact.
* **Environment Pollution:** The script currently writes `small_test.tif` to the working directory; must be moved to a temporary directory.
