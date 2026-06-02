# integrate/unmapped/laptopdump_wreckhunter_build/fetcher.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/fetcher.py

## Steps
1. Relocate file to `/codebase/projects/pipelines/wreckhunter/fetcher.py`.
2. Strip Nuitka/Windows-specific comments and `sys.argv` parsing; replace with fleet-compatible YAML/JSON config loading.
3. Replace `DEFAULT_DATA_DIR` (`C:\Users\...`) with fleet storage paths (e.g., `/data/cache/census_raw`).
4. Implement real USGS EarthExplorer and Sentinel Hub API clients; remove `time.sleep` mocks and mock SEAGULL current data.
5. Migrate credential handling from raw env vars to fleet secrets manager injection.
6. Add unit tests for `USGSFetcher` and `SentinelFetcher` using `unittest.mock` to validate pagination, rate limiting, and error paths.
7. Register in pipeline manifest for scheduled execution (e.g., cron or Airflow DAG) with retry logic.

## Risks
- Hardcoded Windows paths and Nuitka references will cause immediate runtime failures on Linux fleet nodes.
- Mock implementations obscure real API rate limits, auth token refresh logic, and pagination boundaries.
- Credential handling must be migrated to fleet secrets to prevent leakage in logs or version control.
- Large `.tar.gz` downloads may exceed pipeline timeout limits; requires chunked/streaming validation and disk quota checks.
