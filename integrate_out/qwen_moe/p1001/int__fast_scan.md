# integrate/unmapped/laptopdump_wreckhunter_build/fast_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/tools/fast_scan.py

## Steps
1. Move file to `/codebase/projects/pipelines/tools/fast_scan.py`.
2. Replace hardcoded Windows paths in `main()` with `argparse` arguments or fleet environment variables.
3. Add `Pillow` and `numpy` to pipeline `requirements.txt` or `pyproject.toml`.
4. Refactor `main()` to accept `--threshold` and `--output-dir` via CLI.
5. Add unit tests for `process_tiff_fast` using mock TIFF data.
6. Verify memory usage for large TIFFs; consider chunking if needed.

## Risks
- Large TIFFs may OOM on CPU; monitor memory limits.
- Path resolution must be cross-platform compatible.
- Dependencies must be pinned to fleet-compatible versions.
