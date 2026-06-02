# integrate/unmapped/laptopdump_wreckhunter_build/analyze_fuel_leaks.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/post_processing/fuel_leak_analyzer.py

## Steps
1.  Extract `calculate_leak_index` and `classify_pixel` logic into a new utility module in `pipelines/utils/spectral_indices.py`.
2.  Modify the `Scanner` component to extract and include B08 (NIR) and B12 (SWIR-2) reflectance values in the detection JSON output.
3.  Implement the `analyze_detection` logic within the pipeline's post-processing stage to populate `is_fuel_sheen_candidate` and `is_bubble_foam` flags.
4.  Add unit tests for `calculate_leak_index` to handle zero-division and threshold boundary conditions.
5.  Integrate SAR (Sentinel-1) correlation logic as a secondary validation step in the pipeline.

## Risks
* **Upstream Dependency**: The logic is non-functional until the scanner is updated to provide B08 and B12 reflectance data.
* **False Positives**: Bubbles/foam can mimic fuel signatures if SWIR-2 reflectance is noisy or incorrectly thresholded.
* **Data Availability**: Requires Sentinel-2 tiles to have B12 band data available for the specific target area.
