# integrate/unmapped/laptopdump_wreckhunter_build/diagnose_gpu.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/diagnostics/diagnose_gpu.py

## Steps
1. Move file to `/codebase/diagnostics/diagnose_gpu.py` and set `chmod +x`.
2. Replace hardcoded `target/release/cesarops-gpu.exe` with a configurable lookup: `Path(__file__).resolve().parent.parent / "target" / "release" / "cesarops-gpu"`, appending `.exe` only on `sys.platform == "win32"`.
3. Add `CESAROPS_GPU_BIN` environment variable override to allow external binary injection.
4. Add `pytest` stub in `/codebase/tests/test_diagnostics.py` to verify graceful fallback when `nvidia-smi`, `vulkaninfo`, or the Rust binary are missing.
5. Register in `/codebase/Makefile` as `make diagnose-gpu` and document in `/codebase/README.md` under "Hardware Diagnostics".

## Risks
- Hardcoded `.exe` extension breaks Linux/macOS execution and CI runners.
- Depends on untracked Rust build artifacts (`target/`); will fail on clean clones or fresh agents.
- Requires specific NVIDIA drivers and Vulkan SDK; unsuitable for standard fleet/CI environments.
- Tightly coupled to Quadro M2200; lacks abstraction for multi-GPU or future hardware support.
- `subprocess.run` with `timeout=30` may hang if the Rust engine deadlocks; consider adding `signal` handling on POSIX.
