# integrate/unmapped/laptopdump_wreckhunter_build/process_with_coordinates.py

## Verdict
PORT_TO_PIPELINES

## Target path
`/codebase/projects/pipelines/tools/process_tiff_coords.py`

## Steps
1. **Move & Rename**: Move `process_with_coordinates.py` to `codebase/projects/pipelines/tools/process_tiff_coords.py`.
2. **Refactor Coordinates**: Replace hardcoded UTM/WGS84 approximations with `pyproj` for accurate transforms. Remove Lake Michigan specific constants.
3. **Dependency Check**: Add `pyproj` to `requirements.txt` if not present. Ensure `cesarops-gpu` binary path is configurable via env var or config.
4. **Pipeline Integration**: Add to `codebase/projects/pipelines/configs/wreckhunter.yaml` (or general thermal pipeline) as a `process` step.
5. **Test**: Run against existing test TIFFs to verify coordinate accuracy and JSON output format.

## Risks
- **Coordinate Accuracy**: Hardcoded approximations will fail for non-Lake Michigan regions.
- **Binary Dependency**: Script assumes `cesarops-gpu.exe` is in `target/release/`. Needs robust path resolution.
- **Error Handling**: Minimal error handling for missing inputs or GPU failures.
- **Hardcoded Thresholds**: Thresholds are hardcoded in `main`; should be configurable.
