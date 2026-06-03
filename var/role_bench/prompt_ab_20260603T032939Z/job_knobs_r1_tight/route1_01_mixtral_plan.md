## Plan

To fix the issues with the POC blue-green clarity concept in cesarops-satellite for the clear-water Straits, the following knobs will be adjusted in `knobs.pon.rs`:

* `poc_zscore_threshold`: Cap z-scores at 10 to avoid false positives.
* `poc_max_candidates`: Increase the number of candidates to ensure that preserve wrecks are detected.
* `poc_min_separation_px`: Decrease the minimum separation between candidates to improve detection accuracy.
* `poc_downsample_max_dim`: Decrease the downsample maximum dimension to increase the resolution of the analysis.
* `min_score`: Set a minimum score for candidates to further filter out false positives.

The following functions in `poc.rs` will be modified:

* `concept_blue_green_clarity`: Implement the changes to the z-score threshold, max candidates, min separation, and downsample max dim.
* `find_peak_clusters`: Improve the algorithm to reduce edge false positives and ensure that preserve wrecks are detected.

## Acceptance

The fixed cesarops-satellite POC blue-green clarity concept will be accepted if it meets the following criteria:

* GPS: The analysis will be performed on the Cedarville location at 45.7873,-84.6708 and the Burns location at 45.87127,-84.58642, with a maximum distance of 300m.
* Metrics: The analysis will have a minimum score of 5 for candidates, and the z-scores will be capped at 10. The number of false positives will be reduced, and preserve wrecks will be detected.

## Test Commands

To build and run the fixed cesarops-satellite POC blue-green clarity concept, use the following commands:

* Build: `cargo build --release -p cesarops-satellite --features gdal`
* Run: `/data/cargo-target/release/sat-run --spec /data/missions/straits_local_run.json --root /data/cesarops/satellite_data`

Note: The `straits_local_run.json` mission file should be updated with the new knob values and the Cedarville and Burns locations.