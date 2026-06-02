# pipelines/mag/mag_data_pipeline.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/mag/mag_data_pipeline.py

## Steps
1. Apply the unified diff to `/codebase/projects/pipelines/mag/mag_data_pipeline.py`, updating `nrcan_ca_1km_rtf` and `nrcan_ca_200m_rtf` `urls` lists and descriptions.
2. Verify the new `index-eng.php` landing endpoints are accessible and return valid HTTP 200/302 responses.
3. Run targeted pipeline tests: `python scripts/mag_data_pipeline.py --stages download --sources nrcan_ca_1km_rtf,nrcan_ca_200m_rtf --grids-dir /tmp/test_grids` to confirm the download handler correctly follows redirects or extracts direct links from the landing pages.
4. If download logic requires parsing HTML/JS from the landing page, update the `download_file` or `fetch_url` helper to handle the new endpoint format, then retest.
5. Commit changes, push to main, and verify CI pipeline execution.

## Risks
- Landing page URLs (`index-eng.php`) may not serve direct file downloads or may require session cookies/JavaScript, breaking the existing `urllib`/`requests` download flow.
- Mixed `http`/`https` protocols could trigger redirect loops or TLS warnings depending on NRCan's current routing.
- NRCan DAP structure may have shifted entirely, requiring a new download strategy (e.g., API query or direct FTP/SFTP fallback).
