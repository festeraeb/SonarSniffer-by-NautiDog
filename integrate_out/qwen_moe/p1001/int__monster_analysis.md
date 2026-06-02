# integrate/unmapped/laptopdump_wreckhunter_build/monster_analysis.py

## Verdict
PORT_TO_PIPELINES

## Target path
`/codebase/projects/pipelines/analysis/monster_site_analysis.py`

## Steps
1. Create directory `/codebase/projects/pipelines/analysis/` if missing.
2. Copy file to `/codebase/projects/pipelines/analysis/monster_site_analysis.py`.
3. Replace hardcoded Windows path `Path(r"C:\Users\thomf\...")` with `Path(__file__).parent / "outputs"`.
4. Ensure `analyze_monster_site()` and `generate_kml()` are exposed at module level for import.
5. Update `if __name__ == "__main__":` block to use the relative output path and print success/failure status.
6. Verify `haversine_distance` precision matches fleet standards (currently uses standard Earth radius 6371000.0m).
7. Add module docstring referencing T440 P100 fleet context and SS Andaste historical data source.

## Risks
- Original script contains absolute Windows paths; must be sanitized to avoid runtime errors on Linux/CI.
- Analysis is site-specific ("Monster"); ensure it doesn't pollute general pipeline namespace.
- KML output format is legacy; verify downstream consumers (e.g., Google Earth, GIS tools) accept the generated structure.
- No error handling for file I/O; add `try/except` around output directory creation and file writing.
