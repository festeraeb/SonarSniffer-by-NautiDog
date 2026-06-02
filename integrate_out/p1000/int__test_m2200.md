# integrate/unmapped/laptopdump_wreckhunter_build/test_m2200.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/gpu_engine/tests/test_m2200_hardware.py

## Steps
1. Move script to `gpu_engine/tests/` directory.
2. Refactor `create_test_tiff` to remove the `subprocess.run(["pip", "install"...])` block; add `Pillow` and `numpy` to `requirements.txt`.
3. Update `run_gpu_test` to resolve the binary path dynamically using `pytest` or `os.path` relative to the project root instead of the hardcoded `target/release/` path.
4. Wrap the `main()` logic into a `pytest` function to allow integration into the standard CI/CD pipeline.
5. Add a fixture for the synthetic TIFF generation to ensure cleanup of `test_thermal.tif` after test execution.

## Risks
* **Path Fragility**: The hardcoded path to `cesarops-gpu.exe` will fail in containerized or non-cargo environments.
* **Dependency Bloat**: Adding `numpy` and `Pillow` to the core pipeline requirements if not scoped correctly to the test environment.
* **Hardware Dependency**: This test requires a physical M2200/Nvidia GPU to pass; must be tagged as `@pytest.mark.gpu` to avoid failing standard CPU-only CI runners.
