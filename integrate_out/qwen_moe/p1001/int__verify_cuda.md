# integrate/unmapped/laptopdump_wreckhunter_build/verify_cuda.py

## Verdict
KEEP_LIVE

## Target path
/codebase/projects/pipelines/scripts/verify_cuda.py

## Steps
1. Create directory `/codebase/projects/pipelines/scripts/` if it does not exist.
2. Copy `verify_cuda.py` to the target path.
3. Set executable permissions: `chmod +x /codebase/projects/pipelines/scripts/verify_cuda.py`.
4. Add `cupy` to the pipeline's dependency manifest (e.g., `requirements.txt` or `environment.yml`) to ensure availability for verification runs.
5. Document the script in the fleet's README under "Environment Verification" to guide engineers on running it post-installation.

## Risks
- **Dependency Availability**: `cupy` and `nvidia-smi` are not guaranteed in all environments; the script assumes a CUDA-capable setup.
- **Hardware Specificity**: The M2200 check is a warning, not a hard fail, but may mislead users on other GPU architectures.
- **Path Assumptions**: `nvidia-smi` relies on PATH configuration; failures may occur if drivers are installed in non-standard locations.
