# integrate/unmapped/laptopdump_wreckhunter_build/three_tile_offset_analysis.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/wreckhunter/tools/three_tile_offset_analysis.py

## Steps
1.  Create directory `/codebase/projects/pipelines/wreckhunter/tools/`.
2.  Copy `three_tile_offset_analysis.py` to target path.
3.  Verify `Pillow` and `numpy` are in project requirements.
4.  Update `TILES` config in script to use a configurable path (e.g., `os.environ.get('CESAROPS_DATA_DIR', '...')`) instead of hardcoded `wreckhunter2000/...`.
5.  Add unit test for `process_tile` using mocked numpy arrays to verify z-score and anomaly logic.
6.  Add script to `tools/README.md` with usage instructions and data requirements.

## Risks
*   **Hardcoded Paths**: `TILES` dict contains absolute/local path `wreckhunter2000/data/cache/census_raw/2025_rossa`. This breaks portability.
*   **PIL TIFF Support**: `PIL` reads pixel data but may strip GeoTIFF metadata (geotransform, projection). If precise geolocation is needed later, `rasterio`
