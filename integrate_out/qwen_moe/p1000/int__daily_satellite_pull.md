# integrate/unmapped/laptopdump_wreckhunter_build/daily_satellite_pull.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/daily_satellite_pull.py

## Steps
1. Relocate file to `/codebase/projects/pipelines/wreckhunter/daily_satellite_pull.py`.
2. Refactor imports: replace `wreckhunter2000.*` with `cesarops.wreckhunter.*` and `tools.*` with `cesarops.tools.*`.
3. Register as a CESAROPS pipeline entry point using `@pipeline.register("daily_satellite_pull")` or equivalent framework decorator.
4. Add unit tests for `main()` argument parsing, date range calculation, and skip-flag logic.
5. Document required environment variables (`USGS_USER`, `USGS_PASS`, `EARTHDATA_TOKEN`) in the pipeline config/manifest.
6. Implement a max `--days` limit or chunking mechanism to prevent unbounded storage/network exhaustion.

## Risks
- Downstream modules (`wreckhunter2000.*`) may be deprecated or relocated; verify existence before merge.
- Unbounded date ranges will trigger excessive downloads; enforce strict `--days` caps.
- Credential paths (`EARTHDATA_TOKEN`) must be injected by the pipeline runner, not resolved via `Path(__file__)`.
- Sparse data sources (SWOT, ICESat-2) have high latency; add retry/backoff logic to avoid pipeline timeouts.
