# integrate/unmapped/laptopdump_wreckhunter_build/analyze_resolution_comparison.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/post_processing/analyze_resolution_comparison.py

## Steps
1. Create directory `/codebase/projects/pipelines/post_processing/` if absent.
2. Copy file to `analyze_resolution_comparison.py`.
3. Replace hardcoded `DB_PATH` and `OUTPUT_DIR` with `argparse` parameters or environment variables (`CESAROPS_DB_PATH`, `CESAROPS_OUTPUT_DIR`).
4. Add `logging` module initialization and replace `print` statements.
5. Add validation: check DB existence, verify schema (runs, detections tables), handle empty result sets.
6. Add error handling for JSON export (permissions, disk space).
7. Register CLI entry point in pipeline manifest or `__main__.py`.
8. Run integration test with sample `cesarops_runs.db` to verify metrics and JSON output.

## Risks
*   DB schema changes break SQL queries.
*   Large DBs may cause performance issues during analysis.
*   Path permissions on fleet nodes for output directory.
*   Missing error handling for missing DB or empty runs.
