# integrate/unmapped/laptopdump_programming_root/andaste_geometry_test.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/analysis/geometry/andaste_geometry_test.py

## Steps
1. Create directory: `mkdir -p /codebase/projects/pipelines/analysis/geometry/`
2. Move file: `mv integrate/unmapped/laptopdump_programming_root/andaste_geometry_test.py /codebase/projects/pipelines/analysis/geometry/andaste_geometry_test.py`
3. Update `__main__` block:
   - Replace `Path(r"C:\Users\thomf\programming\wreckhunter2000\cesarops-search\outputs")` with `Path(__file__).parent / "outputs"`
   - Ensure `output_dir.mkdir(exist_ok=True)` is present
4. Add unit tests for `haversine_distance`, `scan_island_count`, `analyze_tumblehome`, `verify_crane_root`
5. Verify imports: `json`, `math`, `pathlib`, `datetime` are standard library; no external deps
6. Add docstrings to `run_straight_back_sieve` and `generate_andaste_kml` for pipeline documentation
7. Run `python -m pytest tests/test_andaste_geometry.py` to validate logic

## Risks
- Hardcoded target data (TARGET_A, SS_ANDASTE_PROFILE) limits reusability for other sites
- Windows-specific path in `__main__` will fail on Linux/CI environments
- "Simulated" thermal data in functions indicates template status, not production-ready analysis
- No error handling for file I/O or missing directories
- Lack of input validation for coordinates and dimensions
- KML generation assumes fixed structure; may break with different target geometries
