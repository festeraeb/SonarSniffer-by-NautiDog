# integrate/unmapped/laptopdump_wreckhunter_build/run_straits_fox.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/straits_fox_runner.py

## Steps
1. Locate and port the companion scripts: `straits_south_fox_historical_pull.py` and `straits_south_fox_engine_runner.py`.
2. Refactor `run_straits_fox.py` to remove `subprocess.run` calls; import the core logic of the companion scripts as modules instead.
3. Replace the `WH2K` relative path logic with pipeline-compliant environment variables (e.g., `PIPELINE_DATA_DIR`).
4. Move the `check_deps` requirements into the pipeline's `requirements.txt` or Dockerfile.
5. Wire the new workflow into the Forge orchestration tool as a regional processing task.

## Risks
* **Dependency Chain:** The runner is useless without the two specific scripts it calls; if they are missing from the dump, the task is dead on arrival.
* **Path Fragility:** The script relies heavily on `Path(__file__).resolve().parent`, which will fail in a containerized/orchestrated environment without refactoring.
* **Subprocess Overhead:** Using `subprocess` for internal logic bypasses the pipeline's error handling and logging telemetry.
