# integrate/unmapped/laptopdump_wreckhunter_build/detailed_analysis.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/

## Steps
1. **Extract Geo Utilities**: Move `haversine_distance` to `core/utils/geo.py`.
2. **Extract Analysis Logic**: Move `assess_condition` to `core/analysis/wreck_assessment.py`.
3. **Implement Reporting Service**: Port `generate_detailed_kml` into a new `reporting/kml_generator.py` module, accepting a standardized `Detection` object schema.
4. **Create Pipeline Task**: Implement `tasks/wreck_analysis_task.py` to orchestrate the assessment and KML generation, replacing the hardcoded `ALL_DETECTIONS` with data from the detection stream/database.
5. **Standardize Schema**: Define a Pydantic model for the `Detection` and `Site` objects to replace the loose dictionary structure used in the script.

## Risks
* **Heuristic Fragility**: The `assess_condition` logic relies on string matching (e.g., `"strong_steel"`) which is highly sensitive to upstream sensor labeling.
* **Hardcoded Paths**: The source contains hardcoded Windows paths (`C:\Users\thomf\...`) which must be replaced by pipeline-compliant output handlers.
* **Data Schema Mismatch**: The current dictionary-based data structure in `ALL_DETECTIONS` lacks strict typing; a formal schema is required for pipeline stability.
