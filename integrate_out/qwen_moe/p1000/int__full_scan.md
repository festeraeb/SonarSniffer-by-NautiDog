# integrate/unmapped/laptopdump_wreckhunter_build/full_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/full_scan.py

## Steps
1.  Create directory `/codebase/projects/pipelines/wreckhunter/`.
2.  Copy `full_scan.py` to `/codebase/projects/pipelines/wreckhunter/full_scan.py`.
3.  **Refactor Paths**:
    *   Replace hardcoded `C:\Users\thomf\...` in `find_all_tiffs` with `os.environ.get('SCAN_INPUT_DIR', '.')`.
    *   Replace `target/release/cesarops-gpu.exe` in `process_tiff_gpu` with `os.environ.get('CESAROPS_GPU_BIN', 'cesarops-gpu')`.
4.  **Geolocation Accuracy**:
    *   Add `try: import pyproj` block.
    *   Update `utm_to_wgs84` to use `pyproj.Transformer` if available, else warn and use simplified math.
5.  **Configuration**:
    *   Move `ANCHOR_POINTS` to a JSON config file `pipelines/wreckhunter/anchors.json` or environment variable `ANCHOR_POINTS_JSON` to allow mission-specific updates.
6.  **Execution**:
    *   Add `#!/usr/bin/env python3` shebang.
    *   `chmod +x` the file.
    *   Add `pip install pyproj` to pipeline requirements if `pyproj` is used.
7.  **Documentation**:
    *   Add `README.md` in `wreckhunter/` explaining the Anchor-Lock calibration logic and dependency on `cesarops-gpu`.

## Risks
*   **Binary Dependency**: Script fails silently if `cesarops-gpu` is not built or in PATH.
*   **Geolocation Drift**: Simplified UTM/WGS84 math may introduce significant errors over large areas; `pyproj` is critical for production.
*   **Platform Compatibility**: Original code assumes Windows paths; refactoring to env vars is essential for fleet deployment.
*   **Anchor Lock**: Calibration logic is currently a placeholder; real drift correction requires precise transformation matrices.
