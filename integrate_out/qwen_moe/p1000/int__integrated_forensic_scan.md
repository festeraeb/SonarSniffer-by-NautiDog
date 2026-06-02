# integrate/unmapped/laptopdump_wreckhunter_build/integrated_forensic_scan.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/integrated_forensic_scan.py

## Steps
1. Copy `integrated_forensic_scan.py` to `/codebase/projects/pipelines/integrated_forensic_scan.py`.
2. Verify `dual_scan_downloader` package is installed and importable in the T440 P100 environment.
3. Register `execute_integrated_scan` as a pipeline entry point in `pipeline_registry.yaml`.
4. Add unit tests for `ForensicDetection.apply_depth_scaling`, `apply_straits_offset`, and all 5 filter functions.
5. Replace synthetic anomaly generation in `run_curvelet_sharpener` with actual B10 thermal band processing logic before production rollout.
6. Update pipeline documentation with Lake Michigan bounds, tile counts, and threshold tuning notes.

## Risks
- `dual_scan_downloader` dependency may be missing or version-mismatched in the fleet environment.
- `run_curvelet_sharpener` currently returns deterministic synthetic anomalies; real B10 processing must be wired before live use.
- Full-basin scan loads all detections into memory; chunking or streaming may be required for >186 tiles.
- Hardcoded thresholds (`ZION_CONSTANT`, `STRAITS_LAT_THRESHOLD`, `DEPTH_THRESHOLD_FT`) require validation against historical wreck data.
- `hash()`-based seeding in `run_curvelet_sharpener` is non-portable across Python versions; replace with `random.Random(seed)` for reproducibility.
