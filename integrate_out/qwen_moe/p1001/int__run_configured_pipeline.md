# integrate/unmapped/laptopdump_wreckhunter_build/run_configured_pipeline.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/wreckhunter/run_configured_pipeline.py

## Steps
1.  **Move file**: Copy to `/codebase/projects/pipelines/wreckhunter/run_configured_pipeline.py`.
2.  **Fix Rust binary resolution**: Replace hardcoded `Path(__file__).parent / "target" / "release" / "cesarops-gpu.exe"` with a lookup via `forge` tool or `CESAROPS_GPU_BIN` environment variable to ensure portability.
3.  **Remove test limits**: Remove `limit = min(len(tiffs), 10)` and replace with `config.get('limit', None)` or remove entirely to allow full dataset processing.
4.  **Config validation**: Add `pydantic` model for `pipeline_config.json` to validate schema (threshold, weights, etc.) at load time.
5.  **Implement exports**: Implement KML export using `simplekml` (or standard lib if available) and CSV export; remove `TODO` stubs.
6.  **Add tests**: Create `test_run_configured_pipeline.py` with unit tests for `calculate_detection_score` and `find_tiffs`.
7.  **Wire Forge**: Register as `forge pipeline run wreckhunter` command in `forge` config.

## Risks
*   **Rust binary path**: Hardcoded paths will break on other fleet machines; must use standard resolution.
*   **Config schema**: External JSON config is fragile; validation is critical.
*   **KML/CSV deps**: Adding `simplekml` may require dependency management updates.
*   **Performance**: Current `subprocess` per-TIFF approach may be slow for large datasets; consider batching if needed.
