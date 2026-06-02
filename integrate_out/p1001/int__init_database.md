# integrate/unmapped/laptopdump_wreckhunter_build/init_database.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/cesarops/db/manager.py

## Steps
1. **Extract Schema**: Move the SQL schema from `create_minimal_schema` and the external `.sql` file into a dedicated `schema.sql` asset in the `/db/` directory.
2. **Refactor Logic**: 
    - Remove `DB_PATH.unlink()` to prevent accidental data destruction in production.
    - Implement `CREATE TABLE IF NOT EXISTS` logic for all tables.
    - Replace hardcoded `DB_PATH` with an environment variable lookup (e.g., `CESAROPS_DB_PATH`) with a fallback to a local `./outputs/` directory.
3. **Decouple Testing**: Move `test_database()` and the `main()` execution block to `/codebase/tests/test_db_init.py`.
4. **Integrate**: Import `init_database` into the main pipeline entry point to ensure the database is ready before the first scan run begins.
5. **Forge Wire**: Add a Forge task to run the database initialization check during the pipeline's `setup` phase.

## Risks
* **Data Loss**: The current script contains `DB_PATH.unlink()`, which will wipe existing telemetry if run on a live database.
* **Concurrency**: SQLite may encounter `database is locked` errors if multiple pipeline instances attempt to write to the same file simultaneously.
* **Path Fragility**: The reliance on `Path(__file__).parent` requires strict directory structure maintenance during deployment.
