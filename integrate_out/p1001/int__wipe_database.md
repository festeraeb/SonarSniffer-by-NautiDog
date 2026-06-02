# integrate/unmapped/laptopdump_wreckhunter_build/wipe_database.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/utils/wipe_database.py

## Steps
1. Create directory structure: `projects/pipelines/wreckhunter/utils/`.
2. Refactor `DB_PATH` to accept a path via CLI argument or environment variable to avoid hardcoded relative paths.
3. Replace `print` statements with standard `logging` module calls.
4. Fix logic error: Move `conn.close()` before `return` statements or use a `with sqlite3.connect(...) as conn:` context manager.
5. Add `try...except sqlite3.Error` blocks to handle database locks or corruption.
6. Update verification logic to include the `swot_passes` table.
7. Add a test suite using `pytest` and an in-memory `:memory:` SQLite database to verify table clearing.

## Risks
* **Data Loss**: Script is destructive; accidental execution on production DB will result in irreversible data loss if no backup exists.
* **Unreachable Code**: Current implementation has `conn.close()` after `return` statements, which will leak connections if not refactored.
* **Hardcoded Path**: The current `DB_PATH` is brittle and will fail if run from a different working directory.
