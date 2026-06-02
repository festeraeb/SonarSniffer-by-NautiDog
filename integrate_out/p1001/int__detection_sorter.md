# integrate/unmapped/laptopdump_wreckhunter_build/detection_sorter.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/filters/detection_sorter.py

## Steps
1. Create directory `/codebase/projects/pipelines/filters/`.
2. Port `detection_sorter.py` to the target path.
3. Refactor `API_KEYS` to fetch from environment variables (e.g., `os.getenv('CESAROPS_ADMIN_KEY')`).
4. Move `BBOX_PRESETS` to a centralized configuration file (e.g., `/codebase/config/spatial_bounds.yaml`).
5. Validate `DetectionSite` class attributes against the production SQLite schema to ensure column alignment.
6. Integrate `DetectionSorter` into the output generation stage of the pipeline (KMZ, Web, and MBTiles tasks).
7. Implement unit tests for `calculate_confidence`, `haversine_distance`, and `calculate_spatial_stddev`.

## Risks
* **Hardcoded Credentials:** The source contains hardcoded API keys that must be removed before deployment.
* **Schema Drift:** The `DetectionSite` mapping assumes a specific SQLite schema; any discrepancy between the laptop dump and the live database will cause runtime errors.
* **Static Spatial Bounds:** `BBOX_PRESETS` are hardcoded and may not scale if new survey areas are added.
* **Dependency on Local Files:** The test block relies on a local directory structure that does not exist in the pipeline environment.
