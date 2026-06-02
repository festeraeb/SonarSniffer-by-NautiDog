# integrate/unmapped/laptopdump_wreckhunter_build/inventory_all_files.py

## Verdict
ARCHIVE_STUB

## Target path
/codebase/projects/archives/wreckhunter_recovery/inventory_all_files.py

## Steps
1. Move the script to the `archives/wreckhunter_recovery/` directory.
2. Execute `python3 inventory_all_files.py` from the root of the recovered dataset.
3. Use the generated `outputs/file_inventory.json` to validate the integrity of the `laptopdump` against the `MASTER_FORENSIC_LEDGER.md`.
4. Cross-reference `geotiffs` count in the JSON output with the expected satellite data volume.

## Risks
* **Brittle Validation**: The `CORE_SCRIPTS` and `DOCUMENTATION` lists are hardcoded; any renaming during the recovery process will trigger false "MISSING" reports.
* **Context Sensitivity**: The script relies on `ROOT = Path(".")`, meaning it will fail or produce incorrect inventories if executed from a subdirectory.
* **Non-Operational**: This is a diagnostic utility, not a functional component of the T440 P100 flight software; it should not be integrated into active pipelines.
