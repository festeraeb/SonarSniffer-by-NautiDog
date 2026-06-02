# Local agent task: Aeromagnetic data audit

Project root: `/codebase/repos/wreckhunter2000-1`.

## Your job
1. Use `run_command` to list `pipelines/mag/*.py` and grep for NCEI, harvester, dipole, huron, erie, KML in repo.
2. Use `read_file` on top mag tools: wh2k_harvester, wh2k_ncei_fetch, erie_scanner_pipeline, final_magnetic_fusion, generate_kml (or equivalents if names differ).
3. Find aeromagnetic data paths hardcoded to Windows — note Linux fixes needed.
4. Write `pipelines/mag/MAG_INTEGRATION_AUDIT.md` with:
   - Table: path | purpose | completeness 1-5 | integrate? yes/no
   - Top 5 files for main CESAROPS codebase
   - Proposed `pipelines/mag/forge_tools.json` entries

Use write_file. Be concise under 400 lines total output.
