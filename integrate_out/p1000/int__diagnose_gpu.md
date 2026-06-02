# integrate/unmapped/laptopdump_wreckhunter_build/diagnose_gpu.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/tools/diagnose_gpu.py

## Steps
1. Create `tools/` directory in the pipeline root.
2. Port `diagnose_gpu.py` to the new location.
3. Refactor `check_rust_gpu` path logic: replace hardcoded `.exe` and `.bat` references with platform-agnostic logic or environment variables to support potential Linux-based T440 nodes.
4. Update `check_rust_gpu` to use a relative path from the project root to the `target/release/` directory rather than `__file__.parent`.
5. Add the script to the `README.md` under "Hardware Troubleshooting/Setup".
6. Run `python tools/diagnose_gpu.py` on a test node to verify `vulkaninfo` and `nvidia-smi` integration.

## Risks
* **Platform Lock-in**: The script currently uses Windows-specific extensions (`.exe`, `.bat`) which will fail on Linux environments.
* **Hardware Brittleness**: The script hard-checks for "Quadro M2200"; any hardware revision in the T440 fleet will trigger a false failure.
* **Dependency Assumption**: Assumes `vulkaninfo` and `nvidia-smi` are in the system PATH.
