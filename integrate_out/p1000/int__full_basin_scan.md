# integrate/unmapped/laptopdump_wreckhunter_build/full_basin_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/lake_michigan_scan/full_basin_scan.py

## Steps
1. Create directory `/codebase/projects/pipelines/lake_michigan_scan/`.
2. Refactor `output_dir` logic: replace hardcoded Windows path (`C:\Users\thomf\...`) with `Path(__file__).parent / "outputs"`.
3. Modularize constants: Move `ZION_CONSTANT`, `LAKE_MICHIGAN_BOUNDS`, and `HARBOR_LIGHTS` to a `config.py` or `constants.json` within the pipeline folder.
4. Implement Data Ingestion: Replace the hardcoded `all_detections` list in `run_full_basin_scan` with a function to ingest real sonar/telemetry JSON/CSV data.
5. Standardize `Detection` class: Move the `Detection` class to a shared `models.py` if other pipelines require the same schema.
6. Wire to Forge: Add a `pipeline_manifest.yaml` to allow the Forge tool to trigger this scan via CLI.

## Risks
* **Pathing Errors:** The current script uses absolute Windows-style paths which will fail in the Linux-based pipeline environment.
* **Data Validity:** The script currently operates on simulated/mocked detection data; integration is useless without a real data ingestion layer.
* **Domain Logic Drift:** The "Zion Constant" and specific filter thresholds are highly specialized; they require verification against live sensor calibration to avoid false positives.
