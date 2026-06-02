# integrate/unmapped/laptopdump_wreckhunter_build/inventory_geotiffs.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/inventory_geotiffs.py

## Steps
1. Move file to `/codebase/projects/pipelines/wreckhunter/inventory_geotiffs.py`.
2. Refactor `SEARCH_DIRS` to accept `--search-dirs` or config file input; remove hardcoded `wreckhunter2000` paths.
3. Refactor `OUTPUT_FILE` to accept `--output` argument.
4. Add `argparse` for CLI interface; ensure `--help` works.
5. Update pipeline manifest to include this step; define input/output bindings for data volume.
6. Create unit tests with mock directory structure to verify parsing logic and aggregation.
7. Run integration test against fleet data mount to validate path resolution and JSON output.

## Risks
- Hardcoded paths in `SEARCH_DIRS` must be fully removed; fleet data layout may differ.
- Performance impact if scanning extremely large directories without progress indicators.
- Filename parsing logic assumes specific HLS naming convention; may fail on non-compliant files.
- Missing error handling for permission denied or corrupted files during `stat()`.
- JSON output schema may need versioning if downstream consumers depend on it.
