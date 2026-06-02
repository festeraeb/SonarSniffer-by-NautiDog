# integrate/unmapped/laptopdump_wreckhunter_build/full_basin_scan.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/wreckhunter/full_basin_scan.py

## Steps
1. Create directory `/codebase/projects/pipelines/wreckhunter/`.
2. Copy `full_basin_scan.py` to target path.
3. Patch `__main__` block:
   - Replace `Path(r"C:\Users\thomf\...")` with `Path(__file__).parent / "outputs"`.
   - Add `mkdir(exist_ok=True)` to output dir.
4. Verify imports: `json`, `math`, `pathlib`, `datetime`, `typing` are standard.
5. Run `python -m wreckhunter.full_basin_scan` to validate output generation.
6. Add `wreckhunter/` to `sys.path` or install as package if needed by other modules.

## Risks
* Mock Data: `run_full_basin_scan` uses hardcoded `new_discoveries` list; not connected to live sensor data.
* Hardcoded Constants: `ZION_CONSTANT`, `LAKE_MICHIGAN_BOUNDS` are magic numbers; consider moving to config.
* Windows Path: `__main__` had Windows-specific path; patched to relative.
* No External Deps: Safe, but verify no missing `cesarops` internal libs if this is meant to be a core module.
