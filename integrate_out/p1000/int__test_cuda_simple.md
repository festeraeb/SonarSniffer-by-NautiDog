# integrate/unmapped/laptopdump_wreckhunter_build/test_cuda_simple.py

## Verdict
PORT_TO_PIPELINES

## Target path
`wreckhunter2000/tests/hardware/test_cuda_smoke.py`

## Steps
1. Create directory `wreckhunter2000/tests/hardware/`.
2. Move file to target path and rename to `test_cuda_smoke.py`.
3. Refactor `print` statements into `logging.info` and replace success prints with `assert` statements for `pytest` compatibility.
4. Wrap the heavy workload (Test 3) in a `pytest.mark.skipif` decorator that checks for `cupy.cuda.runtime.getDeviceCount() > 0` to prevent CI failures on CPU-only runners.
5. Ensure `wreckhunter2000.scripts.tools.cuda_env` is included in the `PYTHONPATH` or installed as a package.
6. Add to `pytest` execution suite.

## Risks
* **Hardware Dependency:** Test will fail on any runner lacking an NVIDIA GPU/driver.
* **Execution Time:** The matrix multiplication loop adds latency; must be categorized as a hardware integration test, not a unit test.
* **Environment Mismatch:** `cupy` version must strictly match the CUDA toolkit version installed on the T440 P100 fleet.
