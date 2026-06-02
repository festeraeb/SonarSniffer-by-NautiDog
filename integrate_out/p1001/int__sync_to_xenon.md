# integrate/unmapped/laptopdump_wreckhunter_build/sync_to_xenon.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/deploy_xenon.py

## Steps
1. Create new deployment script in the pipelines directory.
2. Refactor `FILES_TO_SYNC` to be a configurable list or part of the build artifact manifest.
3. Replace hardcoded `XENON_HOST` and `XENON_USER` with environment variables (e.g., `XENON_TARGET_IP`, `XENON_DEPLOY_USER`).
4. Replace `subprocess.run(shell=True)` with a secure implementation using `paramiko` or the standard Forge deployment tool to handle SSH key injection.
5. Implement error handling for network timeouts and authentication failures.
6. Add a verification step to run `python database_connector.py` on the remote host after sync to confirm integrity.

## Risks
* **Security:** The original script contains hardcoded IP addresses and relies on manual password entry, which is unsuitable for automated pipelines.
* **Brittle Pathing:** The `XENON_PATH` uses `~`, which can resolve differently depending on the execution user in a CI/CD environment.
* **Shell Injection:** The use of `shell=True` in the original script is a security vulnerability.
