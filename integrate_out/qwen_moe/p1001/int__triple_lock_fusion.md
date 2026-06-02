# integrate/unmapped/laptopdump_wreckhunter_build/triple_lock_fusion.py

## Verdict
MERGE_INTO_LIVE

## Target path
`/codebase/projects/pipelines/wreckhunter/triple_lock_fusion.py`

## Steps
1. **Directory Setup**: Create `/codebase/projects/pipelines/wreckhunter/` and place `triple_lock_fusion.py` there.
2. **Dependency Injection**: Add `rasterio`, `simplekml`, `cupy`, and `numpy` to `requirements.txt` or `pyproject.toml`.
3. **Path Refactoring**: Replace hardcoded `wreckhunter2000\data\cache\census_raw` with a `Config` object or CLI args (`--data-dir`, `--output-dir`).
4. **Module Structure**: Convert to a package if needed (`wreckhunter/triple_lock_fusion/`) or keep as a single module with `__init__.py` exposing `run_triple_lock_scan`.
5. **Forge Wire**: Create `forge/wreckhunter/triple_lock.py` entry point to invoke `run_triple_lock_scan` with pipeline parameters.
6. **Testing**: Add `tests/test_triple_lock_fusion.py` with mocked `rasterio` datasets to verify `fuse_triple_lock` logic and Z-score calculations.
7. **Documentation**: Add docstring summary for "Triple Lock" theory and sensor requirements (Thermal B10/B11, SAR VV/VH, Optical B08/B04).

## Risks
*   **Hardcoded Paths**: Current script assumes local `wreckhunter2000` directory structure; breaks in CI/cloud environments.
*   **Dependency Weight**: `rasterio` and `cupy` are heavy; ensure environment supports them or provide CPU-only fallback clearly.
*   **Data Availability**: Requires specific Landsat/Sentinel TIFFs; pipeline may fail silently if files are missing.
*   **CRS Handling**: `rasterio.warp.transform` may raise errors on malformed inputs; needs robust try/except.
*   **Performance**: Processing large TIFFs with `numpy` can be memory-intensive; consider chunking or Dask integration later.
