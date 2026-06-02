# integrate/unmapped/laptopdump_wreckhunter_build/process_tiles.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/tools/process_tiles.py

## Steps
1. Relocate file to `/codebase/projects/pipelines/tools/process_tiles.py`.
2. Replace hardcoded `INVENTORY_FILE` and `OUTPUT_DIR` with `os.environ` fallbacks or pipeline context variables (e.g., `PIPELINE_INVENTORY_PATH`, `PIPELINE_OUTPUT_DIR`).
3. Refactor `__main__` block to use `argparse` with `--inventory` and `--output-dir` flags; remove implicit `machine_name` positional dependency.
4. Add `pytest` suite covering `process_tile` edge cases: missing files, zero-variance bands, corrupt PIL payloads, and empty inventory.
5. Register in `pipeline.yaml` as a standalone stage or CLI tool; ensure `numpy` and `Pillow` are in the pipeline runtime image.

## Risks
- `numpy`/`Pillow` may be missing from the pipeline base image; requires explicit dependency declaration.
- Relative path resolution (`Path("outputs/...")`) will fail in containerized runners; must be fully qualified or env-driven.
- `machine_name` CLI arg may conflict with pipeline job-ID naming conventions; pipeline context should inject runtime identifiers.
- Z-score division by `std_val + 1e-6` masks zero-variance bands silently; consider explicit warning or skip logic.
