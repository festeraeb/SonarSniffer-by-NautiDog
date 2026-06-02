# pipelines/mag/adaptive_background_scan.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/mag/adaptive_background_scan.py

## Steps
1. **Import Forge tool wire**: The live version already has the `use_vertical_derivative` parameter wired in the function signature and conditional logic. No new imports needed.
2. **Merge diff**: Apply the unified diff to bring the live version to the reference state, but **keep** the `use_vertical_derivative` parameter and its conditional block (lines 98-102 in live).
3. **Tests**: Add a test case for `use_vertical_derivative=True` to ensure the vertical derivative path works correctly with the `mag_preprocess.vertical_derivative` import.

## Risks
- **Backward compatibility**: The `use_vertical_derivative` parameter defaults to `False`, so existing calls without this argument will behave identically to the reference version.
- **Dependency**: The `mag_preprocess` module is imported conditionally inside the function. Ensure `mag_preprocess.vertical_derivative` exists in the live environment (it's already used in the live codebase).
- **Performance**: The vertical derivative adds a computational step when enabled. No impact when disabled (default).
