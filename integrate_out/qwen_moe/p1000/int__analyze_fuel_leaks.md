# integrate/unmapped/laptopdump_wreckhunter_build/analyze_fuel_leaks.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/line5_fuel_leak_detection/analyze_fuel_leaks.py

## Steps
1. Create directory `/codebase/projects/pipelines/line5_fuel_leak_detection/`.
2. Move file, update imports, add type hints (`typing` module).
3. Refactor `main()` to accept `--input` and `--output` CLI args or config file.
4. Implement `load_detections(input_path)` to parse JSON/KMZ outputs.
5. Implement `save_results(results, output_path)` for JSON/GeoJSON export.
6. Add unit tests for `calculate_leak_index` and `classify_pixel`.
7. Verify detection schema matches current scanner output (fields: `aluminum_ratio`, `thermal_delta`, etc.).
8. Update pipeline config to include this script in the post-processing stage.

## Risks
* Scanner output schema may change, breaking `analyze_detection`.
* Large dataset performance without chunking or vectorization.
* Hardcoded paths in original script require careful replacement.
* Missing SAR correlation logic (future enhancement).
