# integrate/unmapped/laptopdump_wreckhunter_build/lake_michigan_scan.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/lake_michigan_scan.py

## Steps
1. Copy `lake_michigan_scan.py` to `/codebase/projects/pipelines/lake_michigan_scan.py`.
2. Replace `from wreckhunter2000.scripts.tools.cuda_env import configure_cuda_environment` with CESAROPS standard CUDA initialization (`cesarops.cuda.init()` or fleet-equivalent).
3. Replace hardcoded Windows paths in `main()` with `argparse` arguments (`--input-dir`, `--output-dir`) or environment variables (`CESAROPS_INPUT`, `CESAROPS_OUTPUT`).
4. Add `simplekml` to `requirements.txt` if not already present in the fleet base image.
5. Register as a pipeline entry point in `pipeline_registry.yaml` or equivalent fleet config.
6. Run integration test with a sample UTM-projected TIFF to verify `rasterio.warp` and `cupy` synchronization.

## Risks
- CUDA 13.2/11.8 fallback logic may conflict with fleet driver versions; verify `cupy` compatibility matrix.
- `simplekml` is an external dependency; ensure it is added to the fleet base image or `requirements.txt`.
- T440 GPU memory may be insufficient for large TIFFs compared to the M2200; consider chunking or streaming if OOM occurs.
- `rasterio.warp` coordinate transformation overhead could bottleneck processing for high-resolution scans.
- Hardcoded paths in `main()` will break execution until refactored to CLI/env inputs.
