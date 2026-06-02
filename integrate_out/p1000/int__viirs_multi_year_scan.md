# integrate/unmapped/laptopdump_wreckhunter_build/viirs_multi_year_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/forensics/viirs_multi_year_scan.py

## Steps
1. **Refactor Paths**: Replace hardcoded Windows paths (`c:\Users\thomf\...`) with `argparse` arguments for `input_dir` and `output_dir`.
2. **Abstract Hardware**: Remove hardcoded `cp.cuda.Device(1).use()`. Implement a device selector that defaults to `Device(0)` or uses an environment variable `CESAROPS_GPU_ID`.
3. **Modularize**: Extract the core logic (parsing, processing, clustering) into a `VIIRSScanner` class to allow for easier integration into larger orchestration workflows.
4. **Dependency Management**: Add `cupy`, `rasterio`, and `simplekml` to the pipeline's `requirements.txt`.
5. **Validation**: Create a test suite using small synthetic `.tif` files to verify the Z-score clustering logic and KMZ generation.

## Risks
* **Path Fragility**: The current script relies on absolute Windows paths; it will crash immediately on the T440 Linux environment without refactoring.
* **Hardware Mismatch**: Hardcoded `Device(1)` will fail if the target machine only has one GPU or if the Quadro M2200 is indexed differently.
* **Memory OOM**: GPU memory management (`free_all_blocks`) is present but may not be sufficient for extremely large VIIRS tiles if the `data_gpu` allocation exceeds VRAM.
* **Dependency Complexity**: `cupy` requires specific CUDA toolkit versions matching the T440 driver stack.
