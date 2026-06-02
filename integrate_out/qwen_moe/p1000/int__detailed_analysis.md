# integrate/unmapped/laptopdump_wreckhunter_build/detailed_analysis.py

## Verdict
ARCHIVE_STUB

## Target path
/codebase/projects/pipelines/archive/laptopdump_wreckhunter_build/detailed_analysis.py

## Steps
1. Move file to `/codebase/projects/pipelines/archive/laptopdump_wreckhunter_build/detailed_analysis.py`
2. Strip `__main__` block and replace hardcoded Windows path with `os.environ.get("CESAROPS_OUTPUT_DIR", Path.cwd() / "outputs")`
3. Extract `haversine_distance` and `assess_condition` to `/codebase/projects/pipelines/utils/geospatial.py` if not already present; add `@deprecated` decorator to the archived copy
4. Create `/codebase/projects/pipelines/archive/laptopdump_wreckhunter_build/README.md` documenting this as a legacy 1929 wreck analysis dump, noting static `ALL_DETECTIONS` and print-heavy output
5. Run `pytest tests/test_geospatial.py` to verify extracted utilities pass fleet standards
6. Update `.gitignore` to exclude `archive/laptopdump_wreckhunter_build/` from CI artifact generation

## Risks
- Hardcoded `C:\Users\thomf\...` path will cause `FileNotFoundError` in Linux/CI environments if executed
- `ALL_DETECTIONS` is static; pipeline integration would
