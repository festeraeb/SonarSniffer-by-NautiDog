# integrate/unmapped/laptopdump_wreckhunter_build/test_ai_callable.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/wreckhunter2000/demos/test_ai_callable.py

## Steps
1. Create directory `/codebase/projects/pipelines/wreckhunter2000/demos/`.
2. Copy `test_ai_callable.py` to the new directory.
3. Replace `sys.path.insert(0, str(Path(__file__).parent))` with standard package-relative imports (e.g., `from global_controls import GlobalScannerSettings`).
4. Update `wreckhunter2000` imports to use absolute package paths (`from wreckhunter2000.gpu_curvelets import ...`).
5. Add a `demos/README.md` documenting the AI agent parameter adjustment workflow and required optional dependencies.
6. Run `python test_ai_callable.py` in a clean environment to verify `ImportError` handling for `torch` and `rasterio`.

## Risks
- Missing optional dependencies (`torch`, `rasterio`, `scipy`) will trigger `ImportError` blocks; ensure environment has them or keep `try/except` robust.
- `sys.path` manipulation is fragile; switching to package imports is mandatory for stability.

