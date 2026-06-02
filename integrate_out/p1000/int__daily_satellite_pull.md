# integrate/unmapped/laptopdump_wreckhunter_build/daily_satellite_pull.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/daily_satellite_ingestion.py

## Steps
1. **Dependency Audit**: Verify all `wreckhunter2000.*` modules (swot, icesat2, landsat, sar) are present in the codebase and mapped to their respective library paths.
2. **Decomposition**: Refactor the `pull_...` functions into discrete, atomic Pipeline Tasks.
3. **Parameterization**: Replace hardcoded `LAKE_BBOX` and `OUTPUT_BASE` with pipeline configuration parameters/environment variables.
4. **Secret Integration**: Replace local file checks (`EARTHDATA_TOKEN`) and `os.environ` lookups with the CESAROPS Secret Vault interface.
5. **Orchestration**: Implement the task sequence (Buoy -> SWOT -> ICESat -> Landsat -> S1 -> S2) using the Forge tool's DAG/sequence runner.
6. **Validation**: Run a test cycle with `--days 1` using mock data to ensure the orchestration logic holds.

## Risks
* **Dependency Fragmentation**: The script relies on a large number of sub-modules; if any `wreckhunter2000` component is missing, the entire pipeline fails.
* **Hardcoded Geometry**: The `LAKE_BBOX` is hardcoded, limiting the pipeline's utility for other regions without code changes.
* **Credential Fragility**: Transitioning from local `.json` tokens and env vars to a formal Secret Vault requires careful mapping to avoid auth failures during automated runs.
