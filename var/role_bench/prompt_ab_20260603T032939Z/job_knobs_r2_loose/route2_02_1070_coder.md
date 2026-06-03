## Spec ##

**Objective:** Stabilize the `blue_green_clarity` index pipeline for clear-water Straits optical tiles. Address three critical POC failures: uncapped z-scores, edge-induced false positives, and missed preserve wreck detections. Calibrate against known geographic anchors (Cedarville, Burns).

**Key Fixes Implemented:**
1. **Uncapped Z-Scores:** Explicit `np.clip(z_scores, -10, 10)` applied post-normalization. Prevents outlier amplification in clear-water scenes where background variance is low.
2. **Edge False Positives:** Configurable margin mask (`edge_margin_pixels`) applied to z-score maps before anomaly thresholding. Eliminates border artifacts from convolution