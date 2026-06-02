# integrate/unmapped/laptopdump_wreckhunter_build/database_connector.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/utils/database_connector.py

## Steps
1. Create directory `/codebase/projects/pipelines/utils/`.
2. Refactor `CENSUS_DB` and `RUNS_DB` paths to use environment variables (e.g., `os.getenv('CESAROPS_CENSUS_DB')`) instead of relative `Path(__file__)` logic to ensure portability.
3. Copy the file to the target path.
4. Update `cesarops_cli.py` and any processing scripts to import from `pipelines.utils.database_connector`.
5. Verify integration by running the `main()` function (renamed to `test_connection()`) against a local test SQLite instance.

## Risks
* **Path Fragility:** The current implementation relies on relative paths (`__file__.parent / "wreckhunter2000"`) which will fail in a containerized or structured pipeline environment.
* **SQLite Concurrency:** Using SQLite for both high-frequency "runs" logging and "census" updates may lead to `database is locked` errors during concurrent CUDA processing and CLI queries.
* **Schema Dependency:** The module assumes a specific schema (e.g., `anomaly_hits`, `stationary_anchors`) exists; integration requires a migration script or initialization step.
