# integrate/unmapped/laptopdump_wreckhunter_build/test_gpu.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/tests/test_gpu.py

## Steps
1. Move `test_gpu.py` to `/codebase/tests/test_gpu.py`.
2. Add `wgpu` to `requirements.txt` or `pyproject.toml` dev dependencies.
3. Ensure `cesarops-gpu` binary is built and accessible in CI environment (e.g., via `target/release/cesarops-gpu.exe` or artifact).
4. Add `test_gpu.py` to CI test matrix (Windows, GPU-enabled runner).
5. Update `README.md` or `CONTRIBUTING.md` to document GPU testing prerequisites (NVIDIA drivers, Vulkan runtime).

## Risks
- **Hardware Dependency**: Requires NVIDIA GPU (Quadro M2200 or compatible); will fail on CPU-only runners.
- **Build Dependency**: Relies on `cesarops-gpu.exe` being built; must ensure Rust toolchain and build step exist in CI.
- **Platform Specificity**: Uses `.exe` extension and `build_gpu.bat`; may need cross-platform adjustments for Linux/macOS CI.
- **Driver/Runtime**: Requires Vulkan runtime and NVIDIA drivers; environment setup must be verified.
