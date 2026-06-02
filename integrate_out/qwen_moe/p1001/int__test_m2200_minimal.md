# integrate/unmapped/laptopdump_wreckhunter_build/test_m2200_minimal.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/tests/gpu/test_m2200_minimal.py

## Steps
1. **Move file**: Relocate from `integrate/unmapped/...` to `tests/gpu/test_m2200_minimal.py`.
2. **Fix binary path**: Replace hardcoded `target/release/cesarops-gpu.exe` with a resolved path (e.g., via `PATH` env var or repo root lookup) to ensure portability.
3. **Fix side effects**: Replace `Path("small_test.tif")` with `tempfile` or add cleanup logic to prevent polluting the working directory.
4. **Add dependencies**: Ensure `numpy` and `Pillow` are listed in `requirements.txt` or `pyproject.toml`.
5. **Document**: Add header comment noting M2200 hardware requirement and CUDA dependency.

## Risks
- **Binary dependency**: Script assumes `cesarops-gpu.exe` is built and discoverable; will fail in clean environments.
- **Hardware dependency**: Explicitly targets M2200; will fail or be irrelevant on non-NVIDIA or integrated GPU systems.
- **Side effects**: Current code writes `small_test.tif` to CWD; must be sanitized.
- **PIL compatibility**: `mode='I;16'` may have platform-specific TIFF writer behavior.
