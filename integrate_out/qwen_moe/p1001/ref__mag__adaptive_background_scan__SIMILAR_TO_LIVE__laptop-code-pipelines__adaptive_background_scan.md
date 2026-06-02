# pipelines/mag/adaptive_background_scan.py

## Verdict
KEEP_LIVE

## Target path
/codebase/projects/pipelines/mag/adaptive_background_scan.py

## Steps
1. No integration required. The laptop dump is an older baseline that lacks the `use_vertical_derivative` parameter and the downstream dipole analysis documentation note present in LIVE.
2. Merging the laptop variant would regress the pipeline by stripping the vertical derivative preprocessing step and associated docstring guidance.
3. Retain LIVE as the canonical source. Update any external references to point to the LIVE path if they currently point to the laptop dump.

## Risks
- Merging the laptop dump would remove the `use_vertical_derivative` feature, breaking workflows expecting gradient-enhanced anomaly detection.
- Verify that no external CI/CD or data ingestion scripts hardcode the laptop dump path; update them to `/codebase/projects/pipelines/mag/adaptive_background_scan.py` if necessary.
