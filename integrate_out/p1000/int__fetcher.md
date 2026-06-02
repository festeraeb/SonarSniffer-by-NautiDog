# integrate/unmapped/laptopdump_wreckhunter_build/fetcher.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/acquisition/fetcher.py

## Steps
1. Create directory `/codebase/projects/pipelines/acquisition/`.
2. Port `fetcher.py` to the target path.
3. Refactor `DEFAULT_DATA_DIR` to use `os.getenv("CESAROPS_DATA_DIR", "/data/cache/census_raw")` to remove hardcoded Windows paths.
4. Complete the `SentinelFetcher` implementation (the provided source is truncated).
5. Replace all Windows-specific path strings with `pathlib.Path` objects to ensure Linux compatibility for the T440 fleet.
6. Add `requests` to the pipeline's `requirements.txt`.
7. Create a `test_fetcher.py` using `unittest.mock` to simulate USGS/Sentinel API responses for CI/CD validation.

## Risks
* **Incomplete Source:** The `SentinelFetcher` class is truncated in the provided dump; implementation details for the Sentinel Hub Processing API are missing.
* **Platform Dependency:** The source contains hardcoded Windows paths (`C:\Users\...`) and mentions Nuitka compilation for Windows; must be strictly refactored for Linux/Docker environments.
* **Credential Security:** The script accepts credentials via CLI arguments; must ensure the pipeline implementation strictly enforces `ENV` variable usage to prevent credential leakage in logs.
* **API Rate Limiting:** Automated execution of this fetcher against USGS/Sentinel Hub may trigger rate limits if not throttled.
