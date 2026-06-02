# integrate/unmapped/laptopdump_wreckhunter_build/extract_oil_spills_kmz.py

## Verdict
PORT_TO_PIPELINES

## Target path
`/codebase/projects/pipelines/wreckhunter/post_processing/kmz_generator.py`

## Steps
1. **Refactor Configuration**: Replace hardcoded `RESULTS_DIR` and `OUTPUT_KMZ` with `argparse` arguments to allow the pipeline to pass input JSON paths and output destinations.
2. **Improve Geolocation**: Replace the hardcoded `TILE_CENTERS` approximation with a logic block that extracts actual tile centroids from the `tile_result` metadata if available in the JSON.
3. **Dependency Management**: Add `simplekml` to the project's `requirements.txt` or `environment.yml`.
4. **Integration**: Add a call to this script as the final step in the `wreckhunter` pipeline execution sequence (post-analysis).
5. **Validation**: Run with a sample `full_results.json` to ensure KMZ generation and icon styling work as expected.

## Risks
* **Hardcoded Coordinates**: The current script uses a single approximate coordinate `(-87.0, 42.5)` for all tiles; without actual centroid data from the JSON, the KMZ will be spatially inaccurate.
* **Dependency Overhead**: Introducing `simplekml` adds a new external dependency to the pipeline environment.
* **Data Schema Dependency**: The script assumes a specific nested JSON structure (`sensors -> leaking_boat`); any change in the upstream detection engine will break this utility.
