# Local agent task: Satellite data audit

Project root: `/codebase/repos/wreckhunter2000-1` (pipelines symlinked to `/mnt/data-external/projects/pipelines`).

## Your job
1. Use `run_command` to list and sample:
   - `pipelines/satellite/*.py`
   - `backup/LAPTOP_FULL_INVENTORY.md` (satellite section)
   - `grep -l earthdata\|CMR\|sentinel\|SWOT\|ingest wrecks_api/*.py pipelines/satellite/*.py` (limit output)
2. Use `read_file` on the 5 most important satellite scripts (harvesters, NASA client, ingest).
3. Identify **hidden gems** — complete download/ingest tools not wired to wrecks_api.
4. Write report to `pipelines/satellite/SATELLITE_INTEGRATION_AUDIT.md` with:
   - Table: path | purpose | completeness 1-5 | integrate? yes/no
   - Top 5 files to wire into main CESAROPS
   - Exact `forge_cli` subcommands to add (mirror pipelines/bag pattern)

Use write_file for the report. Do not ask the user questions.
