# integrate/unmapped/laptopdump_wreckhunter_build/inventory_geotiffs.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/tasks/inventory_geotiffs.py

## Steps
1. Refactor `SEARCH_DIRS` and `OUTPUT_FILE` to be passed as arguments via a `PipelineConfig` object or environment variables.
2. Replace the brittle `parts.split('.')` logic with a regex-based parser to handle variations in Landsat/Sentinel naming conventions.
3. Wrap the `inventory_geotiffs` function into a standard CESAROPS task wrapper.
4. Replace `print` statements with `logging.info` and `logging.error` for integration with the Forge telemetry system.
5. Implement a `validate_paths` check to ensure `SEARCH_DIRS` are mounted/available before execution.
6. Add a unit test in `/tests/pipelines/wreckhunter/test_inventory.py` using a mock filesystem.

## Risks
* **Path Fragility**: The current script relies on hardcoded relative paths which will fail in a containerized pipeline environment.
* **Parsing Errors**: The filename parsing logic (`parts[5]`) is highly sensitive to index shifts if the filename format deviates slightly.
* **Data Integrity**: The script assumes any file >100KB is valid data; it lacks checksum or header validation.
