# integrate/unmapped/laptopdump_wreckhunter_build/fast_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/processing/tiff_anomaly_scanner.py

## Steps
1.  **Refactor Logic**: Extract `process_tiff_fast` into a standalone utility function.
2.  **Sanitize Paths**: Remove all hardcoded Windows local paths (`C:\Users\...`) and replace with a configurable input directory or pipeline argument.
3.  **Standardize Interface**: Implement a `run_task(input_dir: Path, threshold: float)` entry point compatible with the Forge orchestration layer.
4.  **Optimize Memory**: Add a check for image dimensions before casting to `float32` to prevent OOM on large Sentinel-2 tiles.
5.  **Integrate Logging**: Replace `print` statements with `logging` module calls for T440 telemetry.
6.  **Test**: Verify Z-score math against known anomaly datasets using `pytest`.

## Risks
* **Memory Exhaustion**: Converting large TIFFs directly to `float32` NumPy arrays can trigger OOM on standard worker nodes.
* **Path Fragility**: The original script relies on specific local directory structures that do not exist in the containerized environment.
* **Dependency Overhead**: Requires `numpy` and `Pillow` to be present in the pipeline environment.
