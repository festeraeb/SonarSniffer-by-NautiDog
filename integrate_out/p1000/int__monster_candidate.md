# integrate/unmapped/laptopdump_wreckhunter_build/monster_candidate.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/analysis/monster_candidate.py

## Steps
1. Create directory `/codebase/projects/pipelines/analysis/`.
2. Refactor the script to use `argparse` for input parameters (target length, mass, coordinates, and target name) instead of hardcoded constants.
3. Replace the hardcoded `candidates` list with a call to the `cesarops.data.milled_anomalies` module to fetch real-time anomaly data.
4. Implement the "Zion Constant" depth correction as a formal function within the script.
5. Wrap the print statements into a structured `AssessmentReport` class that can output both CLI text and JSON for pipeline logging.
6. Add a test suite in `/tests/analysis/` to verify pixel-to-foot conversion logic.

## Risks
* **Data Stale-ness**: The current script contains a hardcoded "snapshot" of anomalies; if ported without data-layer integration, it will provide outdated results.
* **Manual Logic**: The "Zion Constant" and "Thermal Mass" assumptions are heuristic-based and require validation against the core physics engine.
* **Dependency Gap**: The script assumes "milled data" exists; integration requires a functional connection to the T440 data ingestion layer.
