# integrate/unmapped/laptopdump_wreckhunter_build/test_m2200.py

## Verdict
MERGE_INTO_LIVE

## Target path
`tools/hardware_validation/test_m2200.py`

## Steps
1. Create directory `tools/hardware_validation/` if it does not exist.
2. Move file to `tools/hardware_validation/test_m2200.py`.
3. Ensure shebang `#!/usr/bin/env python3` is present and file is executable (`chmod +x`).
4. Add `pillow` and `numpy` to `requirements-dev.txt` or `pyproject.toml` under `[project.optional-dependencies]`.
5. Update `tools/hardware_validation/README.md` with usage instructions:
   - Prerequisites: `cesarops-gpu.exe` built in `target/release/`.
   - Run: `python tools/hardware_validation/test_m2200.py`.
6. Add `test_thermal.tif` to `.gitignore` to prevent committing synthetic artifacts.

## Risks
- Hardcoded `cesarops-gpu.exe` path assumes Windows and standard Cargo layout; may break on cross-platform or custom build dirs.
- Synthetic data generation uses `np.random` without seed; results are non-deterministic.
- Specific to Quadro M2200; may not generalize to other GPU architectures.
- `pillow` installation via `subprocess` in `create_test_tiff` is fragile; better to rely on environment setup.
