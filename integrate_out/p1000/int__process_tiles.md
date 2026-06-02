# integrate/unmapped/laptopdump_wreckhunter_build/process_tiles.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/process_tiles.py

## Steps
1. Create directory `/codebase/projects/pipelines/wreckhunter/`.
2. Refactor `process_tile` to ensure it is a pure function accepting a `Path` object.
3. Replace hardcoded `INVENTORY_FILE` and `OUTPUT_DIR` constants with `argparse` implementation to allow pipeline orchestration.
4. Implement `argparse` to accept `--inventory`, `--output-dir`, and `--machine-name`.
5. Wrap the execution logic in a `main()` function to prevent side effects during testing.
6. Add a test suite in `/codebase/tests/pipelines/wreckhunter/test_process_tiles.py` verifying Z-score math and anomaly detection.
7. Register the script as a task in the Forge pipeline configuration.

## Risks
* **Memory Exhaustion:** Loading large GeoTIFFs as `np.float32` arrays can cause OOM errors on standard T440 nodes if tiles are large.
* **Path Fragility:** The original script relies on relative paths (`outputs/geotiff_inventory.json`) which will fail in a containerized pipeline without refactoring.
* **Statistical Sensitivity:** The hardcoded Z-score threshold (2.5) may produce high false-positive rates if sensor noise profiles vary across the fleet.
