# integrate/unmapped/laptopdump_wreckhunter_build/run_configured_pipeline.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/pipeline.py

## Steps
1. Create directory `/codebase/projects/pipelines/wreckhunter/`.
2. Port the orchestration logic from `run_configured_pipeline.py` to `pipeline.py`.
3. Refactor `run_rust_gpu` to use `forge.execute_tool` instead of `subprocess.run` to ensure compatibility with the T440 environment.
4. Replace the hardcoded `.exe` path and Windows-specific logic with a tool reference in the `tools/` manifest.
5. Standardize configuration loading by replacing `load_config()` with the standard `cesarops.config` module.
6. Implement the `TODO` sections for KML and CSV exporters using the standard `cesarops.io` exporters.
7. Add unit tests for the `calculate_detection_score` logic.

## Risks
* **Platform Dependency:** The source code uses `.exe` and Windows-style paths; must be neutralized for Linux-based pipeline runners.
* **Hardcoded Paths:** The Rust engine is expected in a relative `target/release` folder; this must be mapped via the Forge tool registry.
* **Incomplete Logic:** KML and CSV export functions are currently stubs.
* **Subprocess Fragility:** Direct `subprocess` calls bypass the pipeline's resource management and logging.
