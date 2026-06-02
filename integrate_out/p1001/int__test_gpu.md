# integrate/unmapped/laptopdump_wreckhunter_build/test_gpu.py

## Verdict
PORT_TO_PIPELINES

## Target path
`/codebase/projects/pipelines/tests/hardware/test_gpu_validation.py`

## Steps
1.  **Refactor Hardware Checks**: Replace hardcoded `"Quadro M2200"` strings with configurable environment variables or a regex pattern to support different T440/P100 configurations.
2.  **Path Generalization**: Update `rust_exe` logic to resolve relative to the project root or via an environment variable `CESAROPS_BIN_DIR` instead of a hardcoded `target/release` path.
3.  **Dependency Management**: Add `wgpu` to `requirements-dev.txt` or the pipeline's dependency manifest.
4.  **Integration**: Register the script as a hardware validation hook in the Forge tool suite to run before heavy compute tasks.
5.  **CI/CD Update**: Ensure the Rust build step (`cargo build --release`) precedes this test in the pipeline execution order.

## Risks
* **Hardware Specificity**: The current script is too specific to a single laptop's GPU (Quadro M2200); it will report failure on other valid NVIDIA hardware if not refactored.
* **Binary Dependency**: The test fails if the Rust engine isn't pre-compiled; requires strict orchestration in the pipeline.
* **Environment Drift**: `wgpu-py` and Vulkan drivers must be consistent across the fleet to avoid false negatives.
