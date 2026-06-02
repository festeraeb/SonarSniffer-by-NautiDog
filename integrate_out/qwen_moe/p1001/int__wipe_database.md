# integrate/unmapped/laptopdump_wreckhunter_build/wipe_database.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/tools/wreckhunter/wipe_database.py

## Steps
1. Replace hardcoded `DB_PATH` with `os.getenv("WRECKHUNTER_DB_PATH", Path("wreckhunter2000/LAKE_MICHIGAN_CENSUS_2026.db"))`.
2. Fix unreachable `conn.close()` by wrapping connection in `with sqlite3.connect(...) as conn:` or adding `finally: conn.close()`.
3. Add `sqlite_sequence` reset for `swot_passes` to match the `DELETE` statement.
4. Add `--dry-run` CLI flag to verify counts without executing `DELETE`.
5. Register CLI entry point in `pipeline/tools/wreckhunter/pyproject.toml` under `[project.scripts]`.
6. Write unit tests mocking `sqlite3.connect` and `cursor.execute` to verify count verification, rollback behavior, and missing DB path handling.
7. Add to pipeline manifest under `ops/maintenance/` with explicit `require_confirmation=True` guard.

## Risks
- Destructive operation: accidental execution on production/staging DBs without confirmation guard.
- Unreachable `conn.close()` causes file locks and WAL journal accumulation.
- `swot_passes` auto
