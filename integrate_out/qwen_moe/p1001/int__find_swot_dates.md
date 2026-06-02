# integrate/unmapped/laptopdump_wreckhunter_build/find_swot_dates.py

## Verdict
PORT_TO_PIPELINES

## Target path
`/codebase/projects/pipelines/satellite/swot/find_swot_dates.py`

## Steps
1.  **Create Directory**: `mkdir -p /codebase/projects/pipelines/satellite/swot/`
2.  **Move File**: Move `find_swot_dates.py` to the target path.
3.  **Fix Token Path**: Replace hardcoded `TOKEN_PATH` with environment variable lookup:
    ```python
    TOKEN_PATH = Path(os.environ.get('EARTHDATA_TOKEN_PATH', Path.home() / '.earthdata' / 'token.json'))
    # Or better:
    token = os.environ.get('EARTHDATA_TOKEN')
    if not token:
        # fallback to file if env var not set
        ...
    ```
4.  **Update Dependencies**: Add `requests` to `requirements.txt` if not present.
5.  **Pipeline Integration**:
    *   Add a pipeline step in `satellite_pipeline.yaml` (or equivalent) to run this script.
    *   Pass `--start` and `--end` via pipeline config or environment variables.
    *   Ensure output JSON is written to a standard pipeline output directory (e.g., `/codebase/output/swot/`).
6.  **Tests**: Add unit tests mocking `requests.get` to verify date parsing and range grouping logic.
7.  **Documentation**: Add docstring updates for fleet usage and required env vars (`EARTHDATA_TOKEN`).

## Risks
*   **Token Security**: Hardcoded Windows path is a critical security/compatibility issue. Must be resolved immediately.
*   **API Rate Limits**: NASA CMR may throttle frequent requests. Consider caching or pagination handling.
*   **BBOX Accuracy**: Lake Michigan BBOX should be verified against actual lake boundaries to avoid missing granules.
*   **Date Parsing**: `time_start[:10]` assumes ISO format; verify CMR response consistency.
