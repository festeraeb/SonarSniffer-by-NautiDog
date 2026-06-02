# integrate/unmapped/laptopdump_wreckhunter_build/prioritized_pull_v2.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/tools/prioritized_pull.py

## Steps
1. **Dependency Audit**: Verify `daily_satellite_pull.py` and `find_swot_dates.py` are present in the pipeline core.
2. **Refactor Configuration**: Move `GREAT_LAKES_BBOXES` and `SWOT_DATES_FILE` from hardcoded paths to a centralized `config.yaml` or environment-based config.
3. **Clean Imports**: Replace the hacky `import find_swot_dates` inside `find_all_swot_dates` (which modifies global state) with a proper function call that accepts a bbox parameter.
4. **Path Normalization**: Replace `Path(__file__).parent` with a standard `PROJECT_ROOT` constant to ensure compatibility with pipeline execution environments.
5. **Integration Test**: Run with `--find-swot-dates` to verify Earthdata token retrieval and JSON caching.
6. **Execution Test**: Run a limited window (e.g., `--days 1`) to verify the "Fusion Mode" triggers all sensor pulls across the lake set.

## Risks
* **State Mutation**: The current script modifies `find_swot_dates.LAKE_MICHIGAN_BBOX` globally, which is dangerous for concurrent execution.
* **Resource Exhaustion**: "Maximum Fusion Mode" triggers massive data pulls (5 lakes $\times$ 5 sensors) based on a single SWOT hit; this could spike API usage/bandwidth.
* **Dependency Fragility**: The script is highly dependent on the specific implementation of `find_swot_dates` and `daily_satellite_pull`.
