# integrate/unmapped/laptopdump_wreckhunter_build/smart_daily_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/temporal_sweep/smart_daily_scan.py

## Steps
1. Create directory `/codebase/projects/pipelines/temporal_sweep/`.
2. Move `smart_daily_scan.py` to the target path.
3. Refactor `LAKE_BBOXES` and `DB_PATH` to load from the central `config.yaml` or environment variables instead of hardcoded constants.
4. Implement the actual logic for `_get_anomalies_for_date` (currently a stub) to interface with the live detection database/API.
5. Verify `detection_sorter` import path against the current pipeline environment.
6. Replace `print` statements with standard `logging` module for pipeline compatibility.
7. Add a test suite for `DateSweepScheduler` to ensure leap year handling and date range logic are robust.

## Risks
* **Stubbed Logic:** The `_get_anomalies_for_date` method is currently a mock; the script will not function without real data integration.
* **Hardcoded Config:** The bounding boxes and paths are hardcoded, which will cause failures if the environment or lake boundaries change.
* **Dependency Risk:** The script relies on `detection_sorter`, which must be correctly mapped in the pipeline's `PYTHONPATH`.
