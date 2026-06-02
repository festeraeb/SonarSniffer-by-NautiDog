# integrate/unmapped/laptopdump_programming_root/analyze_crossref.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/analysis/analyze_crossref.py

## Steps
1. Create directory `/codebase/projects/pipelines/analysis/`.
2. Move `analyze_crossref.py` to `/codebase/projects/pipelines/analysis/analyze_crossref.py`.
3. Refactor script:
    - Wrap all execution logic in `if __name__ == '__main__':`.
    - Add `argparse` to accept `--wrecks` (path to `known_wrecks.json`) and `--scan` (path to v3 JSON file).
    - Remove hardcoded `known_wrecks.json` and `outputs/` paths; use arguments or environment variables with defaults.
    - Add error handling for missing files and malformed JSON.
    - Ensure `HIGH` and `MED` lists are defined at module level but clearly documented as reference data.
4. Add a test script `tests/test_analyze_crossref.py` to verify `haversine_km`, `wreck_coords`, and `nearest_wrecks` with mock data.
5. Update `README.md` in `/codebase/projects/pipelines/analysis/` to document usage and dependencies.

## Risks
- Hardcoded coordinates in `HIGH` and `MED` lists may become stale if reference sites change.
- Glob pattern for v3 scan file is fragile; explicit path argument
