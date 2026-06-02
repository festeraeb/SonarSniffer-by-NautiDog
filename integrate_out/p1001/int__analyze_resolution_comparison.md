# integrate/unmapped/laptopdump_wreckhunter_build/analyze_resolution_comparison.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/tools/analysis/analyze_resolution_comparison.py

## Steps
1. Move script to `tools/analysis/` directory.
2. Refactor `DB_PATH` and `OUTPUT_DIR` to use `argparse` for CLI input (e.g., `--db` and `--out`) instead of hardcoded Windows-style relative paths.
3. Replace Windows path separators (`.\`) with `pathlib` compatible logic for Linux compatibility.
4. Add error handling for `sqlite3.OperationalError` in case the database is locked or the schema is missing.
5. Add a check to ensure `full_res_runs` and `reduced_res_runs` are not empty before attempting division to prevent `ZeroDivisionError`.
6. Verify SQL query compatibility with the production `detections` table schema.

## Risks
* **Pathing:** Current hardcoded paths use Windows backslashes; will fail on T440 Linux environment without refactor.
* **Schema Drift:** Script assumes specific column names (`chunking_enabled`, `pixel_row`, `pixel_col`) which must be verified against the live `cesarops_runs.db` schema.
* **Data Integrity:** The "Position Match Rate" logic relies on exact pixel coordinate matches; if resizing/interpolation is used during the 512x512 downsample, this metric will report 0% match despite high accuracy.
