# integrate/unmapped/laptopdump_wreckhunter_build/extract_oil_spills_kmz.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/leaking_boat/extract_oil_spills_kmz.py

## Steps
1.  **Move file**: Copy to `/codebase/projects/pipelines/leaking_boat/extract_oil_spills_kmz.py`.
2.  **Parameterize paths**: Replace hardcoded `RESULTS_DIR` and `OUTPUT_KMZ` with CLI arguments (`--input`, `--output`) or environment variables (`CESAROPS_INPUT`, `CESAROPS_OUTPUT`).
3.  **Dynamic coordinates**: Replace hardcoded `(-87.0, 42.5)` tile centers with values from `TILE_CENTERS` config or tile metadata to ensure accurate placement.
4.  **Dependencies**: Add `simplekml` to the `leaking_boat` pipeline `requirements.txt` or `pyproject.toml`. Verify `numpy` is already present.
5.  **Pipeline integration**: Add to `leaking_boat/pipeline.yaml` (or equivalent config) as a `post_process` step or separate pipeline triggered by `full_results.json` availability.
6.  **Tests**: Add unit test for `extract_oil_pixels` and integration test for KMZ generation with mock JSON data.

## Risks
*   **Dependency**: `simplekml` must be installed in the fleet environment.
*   **Hardcoded geometry**: Tile centers are approximated; dynamic lookup is required for accuracy.
*   **Performance**: KMZ generation is generally fast, but verify behavior with large `oil_pixel_count` values.
*   **Path coupling**: Script currently assumes a specific run directory structure; parameterization is critical.
