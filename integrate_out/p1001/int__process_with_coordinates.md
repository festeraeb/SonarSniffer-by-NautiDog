# integrate/unmapped/laptopdump_wreckhunter_build/process_with_coordinates.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/processing/anomaly_extractor.py

## Steps
1.  **Refactor Parsing Logic**: Extract the regex-based stdout parsing into a dedicated `GPUOutputParser` class within the pipeline.
2.  **Replace Hardcoded Georeferencing**: Remove the hardcoded UTM/WGS84 math. Integrate `rasterio` to extract the actual affine transform and CRS from the source TIFF to ensure accurate coordinate conversion.
3.  **Standardize Binary Execution**: Replace the hardcoded `target/release/cesarops-gpu.exe` path with a pipeline-managed binary path via environment variables or configuration.
4.  **Integrate into Workflow**: Add the parser as a post-processing step in the standard `T440_Inference_Pipeline`.
5.  **Implement Unit Tests**: Create tests using mock stdout to verify regex accuracy for dimension and Z-score extraction.

## Risks
* **Coordinate Inaccuracy**: The current script uses "rough" math; if ported without replacing it with `rasterio`-based georeferencing, all output coordinates will be wrong.
* **Binary Dependency**: The pipeline must ensure the `cesarops-gpu` binary is compiled and available in the execution environment.
* **Regex Fragility**: The parser relies on specific string patterns in the Rust binary's stdout; any change to the Rust `println!` statements will break the pipeline.
