# integrate/unmapped/laptopdump_wreckhunter_build/triple_lock_fusion.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/analysis/fusion/triple_lock.py

## Steps
1. **Modularize Detectors**: Extract `process_thermal_for_coldsink`, `process_sar_for_steel`, and `process_optical_for_aluminum` into a `detectors.py` utility module.
2. **Implement Fusion Logic**: Reconstruct the missing `fuse_triple_lock` function using `scipy.spatial.KDTree` to perform efficient $O(N \log N)$ spatial proximity searches within the `fuse_tolerance_m` radius.
3. **Refactor Orchestration**: Convert the `run_triple_lock_scan` function into a `TripleLockTask` class inheriting from the standard `PipelineTask` interface.
4. **Decouple Data Access**: Replace hardcoded `Path` objects and `glob` calls with `PipelineContext` lookups to fetch datasets from the `DataCatalog`.
5. **Standardize Output**: Modify the output logic to generate GeoJSON/Parquet for downstream pipeline stages, keeping KMZ generation as an optional `ExportTask`.
6. **Environment Setup**: Add `rasterio`, `simplekml`, and `scipy` to the pipeline `requirements.txt`.

## Risks
* **Truncated Source**: The `fuse_triple_lock` implementation is missing from the provided source and must be re-engineered.
*
