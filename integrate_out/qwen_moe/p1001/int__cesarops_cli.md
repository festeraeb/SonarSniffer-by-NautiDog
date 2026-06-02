# integrate/unmapped/laptopdump_wreckhunter_build/cesarops_cli.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/cesarops/cesarops_cli.py

## Steps
1.  **Create Pipeline Directory**: Create `/codebase/projects/pipelines/cesarops/`.
2.  **Refactor `main()`**:
    *   Replace `argparse` or config loading for `data_dir`, `output_dir`, `buoy_id`, `max_days`.
    *   Remove hardcoded `C:\Users\thomf\...` paths.
    *   Remove `tiffs[:5]` limit; make it configurable or default to all.
3.  **Platform Handling**:
    *   Replace `cesarops-gpu.exe` with platform-aware binary resolution (e.g., `cesarops-gpu` on Linux, `.exe` on Windows).
    *   Ensure `subprocess` calls handle path separators correctly.
4.  **Dependency Management**:
    *   Verify `wreckhunter2000.scripts.tools.gpu_batch_runner` and `buoy_weather_checker` are available in the pipeline environment or move their logic if they are project-specific.
    *   Add `wreckhunter2000` to `requirements.txt` or `pyproject.toml` if external.
5.  **Rust Engine Integration**:
    *   Add a check or build step for `cesarops-gpu` in the pipeline setup.
    *   Ensure `target/release/cesarops-gpu` is accessible or copy it to a known location.
6.  **Testing**:
    *   Add unit tests for `find_tiff_files` (mock `Path.glob`).
    *   Add integration test for `run_rust_gpu_engine` (mock `subprocess.run`).
    *   Test `select_glint_windows` with mock buoy data.

## Risks
*   **Rust Binary**: Pipeline environment may not have Rust toolchain or the binary built.
*   **Dependencies**: `wreckhunter2000` might not be installed in the pipeline runner.
*   **Platform**: Hardcoded `.exe` extension fails on Linux runners.
*   **Buoy Data**: `select_glint_windows` relies on external NDBC API; network access required.
*   **Path Resolution**: `Path(__file__).parent` may not work as expected in all pipeline execution contexts.
