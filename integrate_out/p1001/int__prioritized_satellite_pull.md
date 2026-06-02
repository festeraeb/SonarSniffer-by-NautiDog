# integrate/unmapped/laptopdump_wreckhunter_build/prioritized_satellite_pull.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/prioritized_satellite_pull.py

## Steps
1. Port `daily_satellite_pull.py` and `find_swot_dates.py` to `/codebase/projects/pipelines/wreckhunter/`.
2. Refactor `GREAT_LAKES_BBOXES` into a `constants.py` or `config.yaml` within the pipeline directory.
3. Update `SWOT_DATES_FILE` and `OUTPUT_BASE` to use relative paths based on the new project root.
4. Refactor `find_all_swot_dates` to eliminate the global state mutation hack (`find_swot_dates.LAKE_MICHIGAN_BBOX = lake['bbox']`); pass the bbox as an argument to `query_swot_dates` instead.
5. Update imports in `prioritized_satellite_pull.py` to use absolute project paths (e.g., `from wreckhunter.daily_satellite_pull import ...`).
6. Validate integration by running `python prioritized_satellite_pull.py --find-swot-dates`.

## Risks
* **Global State Mutation:** The current method of overriding `find_swot_dates.LAKE_MICHIGAN_BBOX` is highly unstable and will cause race conditions if parallelized.
* **Hardcoded Paths:** `OUTPUT_BASE` uses a hardcoded directory structure that will fail in a standard CI/CD or containerized pipeline environment.
* **Dependency Chain:** The script is a high-level orchestrator; its success is entirely dependent on the stability of the underlying `daily_satellite_pull` functions.
* **Auth Dependency:** Requires valid Earthdata credentials to be present in the environment.
