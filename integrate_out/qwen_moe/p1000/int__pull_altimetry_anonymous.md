# integrate/unmapped/laptopdump_wreckhunter_build/pull_altimetry_anonymous.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/pull_altimetry_anonymous.py

## Steps
1.  **Move File**: Copy `pull_altimetry_anonymous.py` to `/codebase/projects/pipelines/wreckhunter/pull_altimetry_anonymous.py`.
2.  **Dependencies**: Add `requests` to the project's `requirements.txt` or `pyproject.toml` if not already present.
3.  **Configuration**:
    *   Replace `OUTPUT_BASE = Path(__file__).parent / ...` with a configurable path using `os.environ.get('CESAROPS_OUTPUT_DIR', ...)` or fleet config.
    *   Add `USER_AGENT` to FTP login if AVISO requires it (some anonymous FTPs block default `ftplib` user agents).
4.  **Logic Fix**: The current code accepts `bbox` arguments but passes `None` in `pull_all_altimetry` and does not implement bbox filtering in `download_aviso_file`. Implement bbox filtering logic (e.g., using `xarray` or `rasterio` to check file bounds) or add a post-processing step.
5.  **Pipeline Wire**: Create a pipeline definition (YAML/JSON) in `/codebase/projects/pipelines/wreckhunter/pipeline.yaml` (or equivalent) to invoke this script as a tool.
6.  **Test**: Run against AVISO FTP to verify connectivity and file listing.

## Risks
*   **Missing Filtering**: The script claims to filter by bbox but currently downloads all files. This will consume significant bandwidth and storage.
*   **FTP Stability**: Anonymous FTP can be slow or block connections; `ftplib` is fragile compared to HTTP-based APIs.
*   **Hardcoded Paths**: `OUTPUT_BASE` is relative to the script; must be parameterized for fleet execution.
*   **Dependency**: `requests` is an external dependency that must be managed.
