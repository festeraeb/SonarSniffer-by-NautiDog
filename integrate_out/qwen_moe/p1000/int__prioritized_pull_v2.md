# integrate/unmapped/laptopdump_wreckhunter_build/prioritized_pull_v2.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/wreckhunter/prioritized_pull.py

## Steps
1.  **Move file**: Copy `prioritized_pull_v2.py` to `/codebase/projects/pipelines/wreckhunter/prioritized_pull.py`.
2.  **Refactor `find_all_swot_dates`**:
    *   Remove dynamic modification of `find_swot_dates.LAKE_MICHIGAN_BBOX`.
    *   Update `query_swot_dates` signature to accept `bbox` parameter directly.
    *   Pass `lake['bbox']` from `GREAT_LAKES_BBOXES` to `query_swot_dates`.
3.  **Dependency Verification**:
    *   Ensure `daily_satellite_pull` is a package/module in the repository or installed dependency.
    *   Verify exports: `pull_buoy_data`, `pull_swot`, `pull_icesat2`, `pull_landsat_thermal`, `pull_sentinel1_sar`, `pull_sentinel2`.
    *   Ensure `find_swot_dates` is available as a module or integrated into this script.
4.  **Logging**: Replace `print` statements with `logging` module calls for better observability in pipelines.
5.  **Configuration**:
    *   Make `SWOT_DATES_FILE` and `OUTPUT_BASE` configurable via environment variables or config file.
    *   Default `OUTPUT_BASE` to `/codebase/projects/pipelines/wreckhunter/outputs`.
6.  **Testing**:
    *   Add unit tests for `prioritized_pull` logic (date separation, loop structure).
    *   Mock `daily_satellite_pull` functions to verify call sequences.
    *   Add integration test for `find_all_swot_dates` with mocked Earthdata token.
7.  **CLI Update**:
    *   Update `argparse` help text.
    *   Ensure `--lakes` argument validates against `GREAT_LAKES_BBOXES` keys.

## Risks
*   **Dependency Missing**: `daily_satellite_pull` or `find_swot_dates` may not be available as modules.
*   **Earthdata Token**: Token loading may fail in non-interactive environments; ensure fallback or error handling.
*   **Performance**: Date range calculation and loop may be slow for large ranges; consider chunking or parallelization.
*   **State Mutation**: Dynamic modification of `find_swot_dates` globals is fragile; refactoring is critical.
*   **Hardcoded BBoxes**: `GREAT_LAKES_BBOXES` should be externalized to a config file for maintainability.
