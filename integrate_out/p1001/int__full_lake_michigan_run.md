# integrate/unmapped/laptopdump_wreckhunter_build/full_lake_michigan_run.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/

## Steps
1. Extract `detect_leaking_boat`, `process_thermal`, and `process_optical` logic into a new `modules/spectral_analysis.py`.
2. Refactor `process_tile_full` into a standard pipeline task/operator.
3. Replace hardcoded `SEARCH_DIRS` and `OUTPUT_DIR` with pipeline input parameters and `PipelineContext`.
4. Implement unit tests for the oil-index and wake-mask logic using synthetic `numpy` arrays to ensure Z-score stability.
5. Wire the task into the Forge tool for distributed tile processing across the fleet.

## Risks
* Heuristic-based detection (Z-scores/thresholds) may require calibration for different sensor noise profiles or atmospheric conditions.
* The script relies on strict band naming conventions (`.B04.tif`, etc.) which must be enforced in the pipeline.
* The "leaking boat" logic is highly specialized and may produce false positives in high-wake environments if the `wake_mask` is insufficient.
