# integrate/unmapped/laptopdump_wreckhunter_build/verify_cuda.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/utils/verify_cuda.py

## Steps
1. Move script to `wreckhunter/utils/`.
2. Refactor `test_cuda_toolkit` to remove hardcoded "M2200" strings; replace with generic "NVIDIA GPU" or detect device model dynamically to avoid false warnings on P100 hardware.
3. Update `print` statements to reflect T440 fleet standards.
4. Add `cupy` to `requirements.txt` or `environment.yml` in the project root.
5. Add a `verify-cuda` entry to the project `Makefile` or `README.md` for easy access.
6. Run tests on a T440 node to ensure `nvidia-smi` subprocess call and CuPy operations function correctly under the current driver stack.

## Risks
* **Hardware Mismatch:** The current script specifically looks for "M2200"; running this on the P100 fleet will trigger `[WARN]` messages unless refactored.
* **Dependency Overhead:** CuPy version must strictly match the installed CUDA toolkit version on the T440 nodes to prevent `ImportError`.
* **Subprocess Dependency:** Relies on `nvidia-smi` being present in the system `$PATH`.
