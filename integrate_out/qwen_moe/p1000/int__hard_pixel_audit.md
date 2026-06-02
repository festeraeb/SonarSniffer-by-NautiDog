# integrate/unmapped/laptopdump_wreckhunter_build/hard_pixel_audit.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/hard_pixel_audit.py

## Steps
1. Move file to `/codebase/projects/pipelines/wreckhunter/hard_pixel_audit.py`.
2. Replace hardcoded `C:\Users\thomf\...` paths with `os.environ.get('CESAROPS_WRECKHUNTER_DATA_DIR', Path.home() / 'data' / 'wreckhunter')`.
3. Update `load_tiff_band` to explicitly extract and store CRS via `rasterio` for future geospatial alignment.
4. Add `rasterio>=1.3`, `scipy>=1.10`, `numpy>=1.24` to `pipelines/wreckhunter/pyproject.toml` dependencies.
5. Create `tests/test_hard_pixel_audit.py` mocking `rasterio.open` and `ndimage` functions with synthetic 2D arrays.
6. Register entry point in `pipelines/wreckhunter/__main__.py` as `hard-pixel-audit`.

## Risks
- `rasterio` requires GDAL/C++ build tools; CI/CD agents may fail without pre-installed system libs.
- Windows-style paths will break on Linux/macOS runners until env var abstraction is enforced.
- "Curvelet Sharpener" uses a Laplacian pyramid approximation; verify if this matches the intended forensic sensitivity or if it underestimates high-frequency anomalies.
- Large TIFF bands may cause OOM if `load_tiff_band` loads full arrays into RAM without chunking or memory mapping.
- `Zion Constant` and `DEPTH_THRESHOLD_FT` are magic numbers; require documentation in `config.yaml` or docstrings.
- `two_date_lock` logic assumes identical pixel grids for 2021/2025 data; georeferencing drift could cause false negatives.
