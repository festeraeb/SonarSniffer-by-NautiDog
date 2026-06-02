# integrate/unmapped/laptopdump_wreckhunter_build/daily_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/daily_scan.py

## Steps
1. Move file to `/codebase/projects/pipelines/wreckhunter/daily_scan.py`.
2. Update imports: `from cesarops_engine import process_tile, init_db`.
3. Replace relative paths (`Path(__file__).parent`) with fleet config/env vars for `DB_PATH` and `DATA_DIR`.
4. Remove unused `DetectionSorter` import.
5. Remove hardcoded tile limit `tiles[:10]` or make it a configurable parameter.
6. Add unit tests for `run_daily_scan` logic (mock `process_tile` and `init_db`).
7. Verify `detection_sorter` dependency availability in fleet.

## Risks
- `detection_sorter` may not be available in the fleet environment.
- DB path conflicts if multiple instances run concurrently.
- Hardcoded tile limit may need review for production scale.
- Log file paths are local and may not persist in fleet containers.
