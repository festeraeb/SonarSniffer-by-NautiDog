# integrate/unmapped/laptopdump_wreckhunter_build/small_batch_test.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/core/analytics/anomaly_detection.py

## Steps
1. **Extract Logic**: Move the z-score calculation and anomaly detection logic from `process_tile` into a standardized `AnomalyDetector` class in the new target path.
2. **Parameterize**: Replace the hardcoded threshold (`2.5`) and epsilon (`1e-6`) with configurable parameters in the class constructor.
3. **Schema Integration**: Port the `anomaly_hits` table schema to the main fleet database migration scripts rather than using the `init_db` local function.
4. **Pipeline Integration**: Replace the manual loop in `main()` with a call to the new `AnomalyDetector` within the existing T440 ingestion pipeline.
5. **Validation**: Run the original `small_batch_test.py` logic against the new module to ensure identical z-score outputs.

## Risks
* **Threshold Sensitivity**: The 2.5 z-score threshold is arbitrary; production deployment requires tuning to avoid false positives.
* **Memory Overhead**: Converting large `.tif` tiles to `np.float32` can cause OOM on high-res tiles if not managed via chunking.
* **Schema Drift**: The `anomaly_hits` table must be reconciled with existing fleet database schemas to prevent migration failures.
