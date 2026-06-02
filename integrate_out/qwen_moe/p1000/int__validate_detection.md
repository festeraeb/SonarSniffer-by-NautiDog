# integrate/unmapped/laptopdump_wreckhunter_build/validate_detection.py

## Verdict
ARCHIVE_STUB

## Target path
/archive/legacy/wreckhunter2000/validate_detection.py

## Steps
1. Move file to `/archive/legacy/wreckhunter2000/validate_detection.py`.
2. Add header comment: `# LEGACY: Validation script for wreckhunter2000 build. Do not import.`
3. Comment out hardcoded paths in `TILE_DIR` and `OUTPUT_DIR`.
4. Verify no imports are needed by live code.

## Risks
- Hardcoded paths may cause confusion if not clearly marked.
- Validation logic may be outdated for current builds.
