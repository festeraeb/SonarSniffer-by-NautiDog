# integrate/unmapped/laptopdump_programming_root/analyze_crossref.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/analysis/analyze_crossref.py

## Steps
1. Create `analysis/` directory within the pipeline root.
2. Refactor hardcoded coordinate lists (`HIGH`, `MED`, `STRAITS`) into a `config/anomaly_targets.yaml` file.
3. Replace hardcoded file paths (`known_wrecks.json`, `outputs/`) with `argparse` arguments to allow integration into automated CI/CD validation loops.
4. Extract `haversine_km` and `wreck_coords` into a `utils/geo.py` module if other spatial tools are planned.
5. Add a test suite verifying the Haversine calculation against known test vectors.
6. Integrate as a post-processing validation step in the Forge pipeline execution.

## Risks
* **Brittle Constants:** The script is highly specialized for the Straits of Mackinac; moving it to a general pipeline requires parameterization of the bounding box and anomaly targets.
* **Schema Dependency:** The script assumes a specific JSON structure for `v3_scan` files (e.g., `depth_m_corrected`, `zscore`); any change in the detection pipeline output will break this analysis.
* **File System Coupling:** Relies on a specific `outputs/` directory structure which may not exist in all execution environments.
