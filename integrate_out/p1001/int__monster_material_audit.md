# integrate/unmapped/laptopdump_wreckhunter_build/monster_material_audit.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/analysis/material_density_audit.py

## Steps
1. **Decouple Logic**: Extract `calculate_thermal_decay`, `analyze_magnetic_jitter`, and `compare_thermal_decay` into a core analytical module.
2. **Parameterize Constants**: Move `CARGO_PROFILES` and `HISTORICAL_SEARCH` into a configuration schema or a JSON parameter file.
3. **Refactor Runner**: Rewrite `run_material_density_audit` to accept `target_params` and `reference_params` as input arguments rather than using hardcoded global dictionaries (`TARGET_B`, `ANCASTE`).
4. **Generic KML Engine**: Refactor `generate_monster_kml` into a generic `generate_audit_kml` function that accepts any target/result object.
5. **CLI Implementation**: Implement a command-line interface using `argparse` to allow the pipeline to ingest target JSON files and output results to the standard `/outputs/` directory.
6. **Validation**: Run existing "Monster of Zion" data through the new pipeline to ensure mathematical parity with the original script.

## Risks
* **Data Leakage**: Hardcoded site-specific thresholds (e.g., the "Monster Designation" 10k ton/200ft rule) must be moved to a config file to prevent logic pollution.
* **Pathing Errors**: The original script uses a hardcoded Windows path (`C:\Users\thomf\...`); this must be replaced with relative paths or environment-based output directories.
* **Floating Point Precision**: Ensure `math.log10` and rounding logic remains consistent during refactoring to maintain audit integrity.
