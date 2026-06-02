# integrate/unmapped/laptopdump_wreckhunter_build/test_cuda_minimal.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/tests/hardware/test_cuda_minimal.py

## Steps
1.  Relocate script to the hardware diagnostic directory within the pipelines repo.
2.  Update `wreckhunter2000.scripts.tools.cuda_env` import to point to the canonical CESAROPS `cuda_env` utility.
3.  Refactor hardcoded print statements (e.g., "M2200") to dynamically report the detected device name from `cp.cuda.runtime.getDeviceProperties`.
4.  Integrate into the Forge hardware validation suite to allow automated execution during node provisioning.
5.  Add `cupy` and `numpy` to the pipeline's hardware-test dependency manifest.

## Risks
*   **Hardware Dependency**: Test will fail on any node without a functional NVIDIA driver or CUDA-capable GPU; must be tagged as `hardware_required` in pytest.
*   **Library Mismatch**: `cupy` version must be compatible with the specific CUDA toolkit version installed on the T440 P100 fleet.
*   **Environment Pollution**: Ensure `configure_cuda_env()` does not modify global environment variables in a way that affects subsequent pipeline stages.
