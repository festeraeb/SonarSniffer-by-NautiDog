# integrate/unmapped/laptopdump_wreckhunter_build/cuda_direct.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/anomaly_detection/cuda_processor.py

## Steps
1. Create new module `cuda_processor.py` in the anomaly detection pipeline directory.
2. Refactor `process_tiff_cuda` to accept a `Config` object (replacing hardcoded thresholds).
3. Remove `install_cupy()` and `main()` function; move dependency management to `requirements.txt` or environment setup.
4. Replace hardcoded Windows paths with a configurable `input_dir` parameter.
5. Update `wreckhunter2000.scripts.tools.cuda_env` import to use the pipeline's internal environment utility.
6. Implement a `test_cuda_logic` unit test using a synthetic `numpy` array to verify Z-score math before GPU upload.
7. Wire the module into the main pipeline execution loop.

## Risks
* **VRAM Exhaustion:** M2200 has limited memory; large TIFF files may trigger Out-of-Memory (OOM) errors during `cp.asarray(data)`.
* **Dependency Fragility:** `cupy-cuda13x` is highly specific; mismatch with fleet CUDA drivers will break the module.
* **Pathing Errors:** The source contains hardcoded Windows-style absolute paths that must be stripped.
* **Synchronicity:** The `cp.cuda.Stream.null.synchronize()` call is necessary but must be monitored for performance bottlenecks in high-throughput loops.
