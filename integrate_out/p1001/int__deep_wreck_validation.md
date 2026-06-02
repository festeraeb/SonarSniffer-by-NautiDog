# integrate/unmapped/laptopdump_wreckhunter_build/deep_wreck_validation.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/validation/profile_matchers.py

## Steps
1. Extract `MONSTER_THRESHOLDS` and `ANDASTE_THRESHOLDS` into a centralized configuration module or YAML.
2. Refactor `process_full_suite` and `squeeze_filter` into a `WreckProfileValidator` class within the new target path.
3. Replace the local `load_all_bands` function with the standard `T440.data.loader` interface to handle tile ingestion.
4. Remove hardcoded `TARGETS` and `tile_dir` logic; implement as a function accepting `tile_id` and `coordinates`.
5. Wire up Forge tool tests using synthetic NumPy arrays to validate Z-score threshold triggers.

## Risks
* **Heuristic Calibration:** The length estimation (`np.sqrt(anomaly_pixels) * 30`) is highly uncalibrated and prone to scale errors.
* **Path Fragility:** The source relies on specific local directory structures (`wreckhunter2000/data/...`) that do not exist in the production environment.
* **Statistical Sensitivity:** Z-score calculations are sensitive to local band noise; without standardized pre-processing, results may vary between tiles.
