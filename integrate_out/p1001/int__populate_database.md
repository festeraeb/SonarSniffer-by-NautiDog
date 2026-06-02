# integrate/unmapped/laptopdump_wreckhunter_build/populate_database.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/populate_database.py

## Steps
1. Move script to `/codebase/projects/pipelines/wreckhunter/`.
2. Refactor `DB_PATH` to use an environment variable (e.g., `os.getenv('WRECKHUNTER_DB_PATH')`) instead of a hardcoded relative path.
3. Decouple `init_db` and `populate_from_results` into a `DatabaseManager` class to facilitate unit testing.
4. Update `main()` to handle CLI arguments via `argparse` for better integration with pipeline orchestrators.
5. Add a `tests/test_populate.py` file using `pytest` and a mock JSON input to verify schema creation and data insertion.
6. Register the script as a downstream task in the pipeline manifest (DAG) following the `full_lake_michigan_run` task.

## Risks
* **Hardcoded Paths:** The original script uses a hardcoded relative path for the SQLite DB which will fail in a containerized/pipeline environment.
* **Schema Evolution:** `init_db` only checks for the existence of one table; adding new tables later requires a migration strategy rather than just an `init` call.
* **Upstream Dependency:** The script is tightly coupled to the specific JSON output format of the `full_lake_michigan_run.py` script.
