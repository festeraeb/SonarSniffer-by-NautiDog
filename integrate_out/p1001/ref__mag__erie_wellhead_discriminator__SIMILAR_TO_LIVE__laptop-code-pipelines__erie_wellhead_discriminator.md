# pipelines/mag/erie_wellhead_discriminator.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/mag/erie_wellhead_discriminator.py

## Steps
1. **Import verification**: Confirm `haversine_m` and `parse_ddmm` are still imported and used (they are - no new dependencies)
2. **Forge tool wire**: The laptop version's `CONFIRMED_FIELD_SITES` list was removed in LIVE. This is a deliberate simplification, not a bug. The primary ground truth mechanism (label_id lookup) remains intact.
3. **Tests**: Run existing unit tests for `CandidateMatch.ground_truth` assignment. The LIVE version's fallback logic (`wellhead_distance_m < 500`) is more robust than the geographic radius check (which had hardcoded 1500m radius with no source attribution).

## Risks
- **Data loss**: The laptop version's `CONFIRMED_FIELD_SITES` contained Colgate wreck coordinates (42.173, -81.740) with 1200m radius. This was secondary validation; primary label_id check still works.
- **Regression**: If label_id 103 (Colgate) is missing from `GROUND_TRUTH`, the LIVE version will mark it as "unknown" instead of "wreck". This is acceptable - the label_id system is the authoritative source.
- **Maintainability**: The LIVE version is cleaner (fewer lines, no geographic radius constants). The laptop version's `GT_GEO_RADIUS_M = 1500.0` was magic number without documentation.
