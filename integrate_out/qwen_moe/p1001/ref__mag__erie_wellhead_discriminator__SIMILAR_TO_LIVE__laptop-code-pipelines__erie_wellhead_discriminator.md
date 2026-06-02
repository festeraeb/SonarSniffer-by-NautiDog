# pipelines/mag/erie_wellhead_discriminator.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/mag/erie_wellhead_discriminator.py

## Steps
1. Replace `/codebase/projects/pipelines/mag/erie_wellhead_discriminator.py` with the laptop variant.
2. Verify `GROUND_TRUTH` dictionary and `wellhead_distance_m < 500` fallback logic are preserved.
3. Run smoke test against a sample `adaptive_candidates_scored.csv` to confirm CSV I/O and feature extraction remain functional.
4. Update any internal documentation referencing `CONFIRMED_FIELD_SITES` or `GT_GEO_RADIUS_M`.

## Risks
- Removal of `CONFIRMED_FIELD_SITES` geographic fallback may reduce robustness for targets with missing or misaligned label IDs.
- Downstream scripts or notebooks may reference the removed constants; verify usage before final commit.
- No new dependencies or API changes introduced.
