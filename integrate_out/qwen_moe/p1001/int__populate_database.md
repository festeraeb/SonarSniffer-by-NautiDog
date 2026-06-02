# integrate/unmapped/laptopdump_wreckhunter_build/populate_database.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/tools/populate_database.py

## Steps
1.  **Move File**: Copy `populate_database.py` to `/codebase/projects/pipelines/wreckhunter/tools/populate_database.py`.
2.  **Refactor Config**:
    *   Remove hardcoded `DB_PATH`. Accept `db_path` as a CLI argument or config parameter.
    *   Update `DB_PATH.parent.mkdir` to ensure output directory exists.
3.  **Schema Management**:
    *   Replace inline `CREATE TABLE` logic in `init_db` with a migration check or reference to a `schema.sql` file to prevent drift.
    *   Add version column to tables if schema evolves.
4.  **Pipeline Integration**:
    *   Register as a tool in the pipeline runner (e.g., `forge` or `run_pipeline.py`).
    *   Wire `full_lake_michigan_run.py` output JSON to this tool's input.
5.  **Testing**:
    *   Add unit tests for `populate_from_results` using `unittest.mock` and temporary SQLite DBs.
    *   Verify `anomaly_hits` insertion logic with sample JSON.

## Risks
*   **Schema Drift**: `init_db` creates tables on the fly; this is fragile for production pipelines.
*   **Hardcoded Paths**: `DB_PATH` is relative; needs to be configurable for different environments.
*   **JSON Schema**: No validation of input JSON structure; may fail silently or crash on malformed results.
*   **Duplicate Runs**: No idempotency check; re-running may duplicate data if not handled.
