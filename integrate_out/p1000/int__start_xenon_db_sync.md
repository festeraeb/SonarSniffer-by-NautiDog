# integrate/unmapped/laptopdump_wreckhunter_build/start_xenon_db_sync.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/sync_xenon_db.py

## Steps
1. Create directory `/codebase/projects/pipelines/wreckhunter/`.
2. Refactor hardcoded constants (`XENON_HOST`, `XENON_USER`, `XENON_PATH`) to use `os.getenv()` for environment-based configuration.
3. Update `DB_FILES` paths to use absolute paths relative to the project root rather than `__file__`.
4. Replace `print` statements with standard `logging` module for pipeline observability.
5. Replace `subprocess.Popen` with a more robust execution pattern that captures the remote PID or verifies the process state via `ssh`.
6. Add a requirements check for `scp` and `ssh` in the execution environment.

## Risks
* **Hardcoded Topology:** The `10.0.0.55` IP and `cesarops` user will fail in any environment without manual override.
* **Orphaned Processes:** `subprocess.Popen` is "fire and forget"; if the sync script terminates, there is no mechanism to monitor if the remote `init_database.py` actually succeeded or crashed.
* **Auth Dependency:** Script assumes passwordless SSH key-based authentication is already configured between the runner and Xenon.
* **Path Fragility:** Reliance on `~/cesarops-wreckhunter-build` assumes a specific home directory structure on the target.
