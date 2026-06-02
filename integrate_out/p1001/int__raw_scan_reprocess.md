# integrate/unmapped/laptopdump_wreckhunter_build/raw_scan_reprocess.py

## Verdict
PORT_TO_PIPELINES

## Target path
`/codebase/projects/pipelines/wreckhunter/tools/raw_scan_reprocess.py`

## Steps
1. Move `raw_scan_reprocess.py` to the `tools/` directory under the wreckhunter pipeline project.
2. Refactor `subprocess.run([sys.executable, 'hard_pixel_audit.py'])` to use a relative path from the project root or an absolute path derived from `__file__` to ensure reliability.
3. Verify that `wreckhunter2000` is accessible in the `PYTHONPATH` within the pipeline environment.
4. Ensure `hard_pixel_audit.py` is also ported to the same `tools/` directory to satisfy the subprocess requirement.
5. Add a test case in the pipeline suite to verify the `fetch-only` and `audit-only` modes using mock data.

## Risks
* **Path Fragility**: The current `subprocess` call assumes `hard_pixel_audit.py` is in the same directory; if the file structure changes during porting, the script will fail.
* **Environment Dependency**: Relies heavily on the `wreckhunter2000` module being correctly installed/mapped in the pipeline environment.
* **Side Effects**: The use of `os.chdir(ROOT)` is a side effect that can interfere with other tools if run in the same process space.
