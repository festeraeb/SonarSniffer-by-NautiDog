# integrate/unmapped/laptopdump_wreckhunter_build/run_zero.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/benchmarks/run_zero.py

## Steps
1.  **Move file**: Copy `run_zero.py` to `/codebase/projects/pipelines/benchmarks/run_zero.py`.
2.  **Refactor paths**:
    *   Replace `DATA_DIR` with `os.environ.get('CESAROPS_DATA_DIR', Path('data/cache/census_raw/2021_low_water'))`.
    *   Replace `OUTPUT_BASE` with `os.environ.get('CESAROPS_OUTPUT_DIR', Path('outputs/run_zero'))`.
    *   Replace `DB_PATH` with `Path(tempfile.gettempdir()) / "cesarops_run_zero.db"` to avoid conflicts.
3.  **Fix binary path**:
    *   In `run_scanner`, replace `r".\target\release\cesarops-search.exe"` with a lookup strategy: check `os.environ.get('CESAROPS_BINARY', 'target/release/cesarops-search')` or fallback to `cargo run --release -- scan ...`.
4.  **Clean `get_system_info`**:
    *   Remove `wmic` fallback. Use `psutil` or standard library only.
    *   Return `None` for GPU info if unavailable instead of hardcoded defaults.
5.  **Add tests**:
    *   `test_parse_kml`: Mock KMZ content, verify detection extraction.
    *   `test_log_run`: Verify SQLite schema and data integrity.
    *   `test_run_scanner`: Mock `subprocess.run` to verify command construction.
6.  **Pipeline config**: Add `run_zero` to `pipeline.yaml` under `benchmarks` section, ensuring `CESAROPS_DATA_DIR` and build artifacts are available.

## Risks
*   **Binary dependency**: Script assumes `cesarops-search.exe` is built. Pipeline must ensure build step runs first.
*   **KMZ parsing**: Regex-based parsing is fragile; KMZ structure changes could break detection extraction.
*   **Environment vars**: Missing `CESAROPS_DATA_DIR` will cause silent failures or incorrect paths.
*   **SQLite locking**: Parallel pipeline runs may conflict on DB file; temp file approach mitigates this.
