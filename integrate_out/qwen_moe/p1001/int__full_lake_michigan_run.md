# integrate/unmapped/laptopdump_wreckhunter_build/full_lake_michigan_run.py

## Verdict
PORT_TO_PIPELINES

## Target path
`/codebase/projects/pipelines/wreckhunter/full_lake_michigan_run.py`

## Steps
1. **Refactor Module Structure**
   - Move `process_thermal`, `process_optical`, `detect_leaking_boat`, and `process_tile_full` into a dedicated module `wreckhunter/processors/sensor_fusion.py`.
   - Create `wreckhunter/pipelines/full_lake_michigan_run.py` as the entry point.
   - Replace hardcoded `SEARCH_DIRS` with a `Config` dataclass or environment variables (`CESAROPS_DATA_LAKE`, `CESAROPS_OUTPUT_DIR`).

2. **Pipeline Integration**
   - Implement `run_pipeline(config: Config) -> dict` to handle tile discovery, batching, and result aggregation.
   - Replace `Path.glob` with a tile catalog query or manifest-based iteration for scalability.
   - Direct outputs to `CESAROPS_OUTPUT_DIR/wreckhunter/full_lake_run/{timestamp}/`.
   - Add progress logging via `logging` module instead of `print`.

3. **Testing**
   - Add unit tests for `detect_leaking_boat` with synthetic arrays (oil vs. wake vs. water signatures).
   - Add integration test using a mock tile set in `tests/fixtures/`.
   - Verify threshold sensitivity (`zscore > 2.5`, `oil_mask` logic).

4. **Dependencies & Config**
   - Ensure `numpy`, `Pillow`, `rasterio` (if GeoTIFF metadata is needed later) are in `requirements.txt`.
   - Add `pydantic` or `dataclasses` for configuration validation.

5. **Documentation**
   - Add `README.md` for the pipeline: usage, expected input schema, output JSON structure.
   - Document the oil detection heuristic and false positive mitigation strategies.

## Risks
- **Performance**: Processing all tiles in a single pass may exceed memory/time limits. Implement chunking or parallel processing (e.g., `concurrent.futures` or Dask).
- **Path Mapping**: Hardcoded `wreckhunter2000` paths will fail in the fleet. Ensure data lake paths are correctly mapped via configuration.
- **Heuristic Accuracy**: The oil detection logic relies on simple thresholds and ratios. May produce false positives/negatives; requires validation against ground truth data.
- **Dependency Bloat**: `PIL` for GeoTIFFs is suboptimal; consider `rasterio` for better geospatial support and performance.
