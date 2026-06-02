# integrate/unmapped/laptopdump_wreckhunter_build/find_swot_dates.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/utils/find_swot_dates.py

## Steps
1. Create directory `/codebase/projects/pipelines/wreckhunter/utils/`.
2. Refactor `load_earthdata_token()`: Remove hardcoded Windows path `c:/Users/thomf/...`. Replace with `os.environ.get('EARTHDATA_TOKEN')` or a path defined in a central `config.yaml`.
3. Refactor `LAKE_MICHIGAN_BBOX`: Move bounding box coordinates to a configuration file or pass as CLI arguments to allow for other target areas (e.g., Great Lakes expansion).
4. Update `requirements.txt` in the wreckhunter project to include `requests`.
5. Implement a mock response test in `tests/utils/test_find_swot_dates.py` using `responses` or `unittest.mock` to validate the CMR parsing logic without hitting NASA servers.
6. Wire the script into the main wreckhunter workflow as a pre-acquisition discovery step.

## Risks
* **Hardcoded Paths:** The original script contains absolute Windows paths which will fail in a Linux-based pipeline/container.
* **API Dependency:** The script relies entirely on the NASA CMR API; failure of the NASA service will break the discovery phase.
* **Authentication:** The current token loading mechanism is insecure and brittle; must be migrated to environment variables or a secret manager.
