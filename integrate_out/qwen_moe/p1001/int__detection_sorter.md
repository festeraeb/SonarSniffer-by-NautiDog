# integrate/unmapped/laptopdump_wreckhunter_build/detection_sorter.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/cesarops/wreckhunter/detection_sorter.py

## Steps
1. Copy `detection_sorter.py` to the target path.
2. Replace the hardcoded `API_KEYS` dictionary with environment variable lookups or a centralized config loader (e.g., `os.getenv("CESAROPS_ADMIN_KEY")`).
3. Update the `__main__` DB path to resolve relative to the project root or use a config/env var for the live database location.
4. Add public exports to `/codebase/projects/pipelines/cesarops/wreckhunter/__init__.py` (`DetectionSorter`, `quick_filter`, `calculate_confidence`, `haversine_distance`).
5. Refactor existing pipeline runners or web UI endpoints to instantiate `DetectionSorter` instead of executing raw SQL queries directly.
6. Run the embedded test block against a local replica of `LAKE_MICHIGAN_CENSUS_2026.db` to verify schema compatibility and filter logic.

## Risks
- **Schema Dependency**: `DetectionSorter` assumes specific columns (`confidence_score`, `spatial_stddev_m`, `thermal_zscore`, `sar_coherence`, etc.). Verify live DB schema matches exactly.
- **Security**: Hardcoded API keys in `API_KEYS` must be externalized immediately to prevent credential leakage.
- **Performance**: `quick_filter` opens and closes a connection per invocation. For high-frequency web requests, implement connection pooling or a singleton sorter instance.
- **Numerical Stability**: `statistics.pstdev` on small `events` lists (<2) returns `0.0` per the guard, but spatial clustering metrics may need tighter bounds or fallback logic for sparse detections.
- **Test Block**: The `if __name__ == "__main__":` block will fail in production imports; ensure it's isolated or removed from the live module.
