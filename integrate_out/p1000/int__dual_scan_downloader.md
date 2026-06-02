# integrate/unmapped/laptopdump_wreckhunter_build/dual_scan_downloader.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/acquisition.py

## Steps
1.  **Extract Configuration**: Move `LAKE_MICHIGAN_BOUNDS` and `LOW_WATER_YEARS` into a `config.yaml` or a dedicated `constants.py` within the pipeline directory.
2.  **Refactor Quality Logic**: Move `calculate_tile_quality` and `wgs84_to_utm_zone` into a shared `utils/geospatial.py` module.
3.  **Implement Real API Clients**: Replace the simulated `search_landsat_collection` and `search_sentinel_collection` functions with actual implementations using `requests` for USGS EarthExplorer and Sentinel Hub APIs.
4.  **Sanitize Paths**: Replace the hardcoded Windows path (`C:\Users\thomf\...`) with a dynamic pathing system using `pathlib` and environment variables (e.g., `DATA_ROOT`).
5.  **Forge Integration**: Wrap the execution logic into a Forge-compatible task that accepts date ranges and bounding boxes as arguments.
6.  **Test**: Validate the quality scoring logic against known cloud-cover datasets.

## Risks
* **Simulated Logic**: The current script uses "simulated" API calls; it will not actually fetch data until real API integration is completed.
* **Authentication**: Real implementation requires handling USGS and Sentinel Hub API credentials/tokens.
* **Pathing**: The source file contains hardcoded Windows-specific paths which will fail in a Linux-based pipeline environment.
* **Data Volume**: The "Full Lake Michigan" scan is massive; without strict rate-limiting and error handling, it may hit API quotas or exhaust local storage.
