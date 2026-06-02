# integrate/unmapped/laptopdump_wreckhunter_build/global_controls.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/global_controls.py

## Steps
1. Copy file to `/codebase/projects/pipelines/global_controls.py`.
2. Add `cupy` to `requirements.txt` and configure Forge tool wire to inject `GLOBAL_SETTINGS` into pipeline entry points (`scan_pipeline.py`, `tile_processor.py`).
3. Remove `__main__` block to prevent CLI namespace collisions; expose `parse_args` only for standalone debugging.
4. Write unit tests for `GlobalScannerSettings` preset application, `generate_overlapping_tiles` coordinate bounds, and `stitch_tiles_back` weight normalization.

## Risks
- `stitch_tiles_back` feathering uses explicit Python loops; will bottleneck on large tiles. Vectorize or port to `cupy`/`numba`.
- `argparse` in utility may clash with pipeline CLIs; restrict scope or namespace.
- `cupy` context init may fail on CPU workers; add explicit device selection or fallback.
- `json` I/O in `GLOBAL_SETTINGS` assumes writable FS; mount config volume or use temp dir.
- Edge tile stride logic in `generate_overlapping_tiles` may yield negative dimensions if `chunk_size` > image dim; add clamp validation.
