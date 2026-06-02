# integrate/unmapped/laptopdump_wreckhunter_build/smart_daily_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/smart_daily_scan.py

## Steps
1. Relocate file to target path under `wreckhunter/` pipeline directory.
2. Replace hardcoded `LAKE_BBOXES`, `DB_PATH`, `OFFLOAD_DIR`, and `DEEP_ANALYSIS_DIR` with CESAROPS config loader (`cesarops.config.get_config()`).
3. Wire `DetectionSorter` and SQLite DB connection to Forge tool registry; remove direct `Path` instantiation for DB.
4. Refactor `run_smart_scan()` to accept `config: Dict`, suppress `print()` statements, and return a structured `ScanResult` dict for pipeline telemetry.
5. Write `test_smart_daily_scan.py` covering `DateSweepScheduler._generate_dates()`, leap-year handling, and `RepeatableDetectionFinder` 50m clustering logic.
6. Register `smart_daily_scan` as a cron-triggered step in `wreckhunter_pipeline.yaml` with `SCAN_SEASON_START`/`END` and `SWEEP_YEARS` as pipeline variables.
7. Remove `sys.argv` CLI handling from `__main__` or gate it behind `if __name__ == "__main__"` to prevent pipeline execution interference.

## Risks
- `_get_anomalies_for_date` is currently mocked/simulated; requires real satellite data API integration before production use.
- Static SQLite DB path will fail in distributed/worker execution; must route through Forge DB client or shared object storage.
- Hardcoded lake BBOXes lack geospatial validation; risk of scanning land/water mismatch or outdated coastlines.
- `sys.argv` parsing in `__main__` conflicts with pipeline runner; must be isolated or removed.
- `DEEP_ANALYSIS_DIR` and `OFFLOAD_DIR` creation via `mkdir` may race in parallel workers; use pipeline-managed temp dirs.
