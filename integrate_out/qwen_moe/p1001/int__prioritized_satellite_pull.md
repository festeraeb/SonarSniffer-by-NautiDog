# integrate/unmapped/laptopdump_wreckhunter_build/prioritized_satellite_pull.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/prioritized_satellite_pull.py

## Steps
1. Create directory `/codebase/projects/pipelines/wreckhunter/`.
2. Copy `prioritized_satellite_pull.py` to target path.
3. Update imports: Change `from daily_satellite_pull import ...` to `from pipelines.wreckhunter.daily_satellite_pull import ...` (or verify `daily_satellite_pull` is a package in `sys.path`).
4. Update `find_swot_dates` import similarly: `from pipelines.wreckhunter.find_swot_dates import ...`.
5. Fix `SWOT_DATES_FILE` and `OUTPUT_BASE` paths to use `Path(__file__).parent` or config-driven paths to avoid hardcoded relative paths breaking in new location.
6. Add `wreckhunter` to pipeline registry/manifest if required by fleet config.
7. Verify `daily_satellite_pull` and `find_swot_dates` modules exist and are compatible.
8. Run CLI test: `python prioritized_satellite_pull.py --find-swot-dates --lakes MICHIGAN`.
9. Validate Earthdata token handling and SWOT date cache generation.

## Risks
*   `daily_satellite_pull` and `find_swot_dates` dependencies may be missing or have incompatible signatures.
*   Earthdata token configuration may need to be centralized or mapped to fleet secrets.
*   Hardcoded paths in `SWOT_DATES_FILE` and `OUTPUT_BASE` may break if run from different working directories.
*   SWOT date query logic modifies global state (`find_swot_dates.LAKE_MICHIGAN_BBOX`), which is fragile and should be refactored to pass BBOX as argument.
*   Large date ranges may cause excessive API calls or memory issues without pagination or chunking.
