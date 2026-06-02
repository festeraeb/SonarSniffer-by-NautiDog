# Satellite Pipeline — Merged Execution Plan

## 1. Build & Wire (GDAL + Rayon)
The immediate priority is transitioning from the cloud-based STAC model to a local, offline-first execution model using `rayon` for multi-core CPU parallelism.

### Core Implementation Tasks
*   **`chip.rs` (GDAL Fix):** 
    *   Update `bbox_to_pixel_window` to use `gdal::Dataset::geo_transform` for precise WGS84 $\to$ UTM $\to$ Pixel conversion.
    *   Refactor `decode_local_band` to match GDAL 0.17 signatures: `dataset.read_as::<f32>(window_origin, window_size, out_size, resample)`.
    *   Apply DN $\to$ Reflectance scaling (factor $\approx$ 0.0001 for Sentinel-2).
*   **`poc.rs` (Local Logic):** 
    *   Implement `run_poc_aoi_local` using `glob` to find `*.blue.tif` and `*.green.tif` in the `scene_dir`.
    *   **Parallelism:** Use `rayon` to map over scene paths, decoding B02 (blue) and B03 (green) in parallel.
    *   **Concept Adaptation:** Implement `blue_green_clarity` (B02/B03) to replace the standard B02/B04 (Red) version, as red/NIR do not penetrate to the target depth.
*   **`mission.rs` (Branch Wiring):** 
    *   Wire the `stage_poc_aoi` function to trigger `run_poc_aoi_local` when `knobs.use_local_scenes` is true.

## 2. Science & Calibration
Detection is based on **water-column disturbance (plume)**, not bottom reflectance. The signal is the clarity/texture of the column above the wreck.

### Concept Weighting Matrix
| Concept | Target: **Cedarville** (Steel) | Target: **Burns** (Wood) | Band Priority |
| :--- | :--- | :--- | :--- |
| **Thermal/Cold-Sink** | **HIGH** (Strong signal) | **LOW** (Weak signal) | N/A (Thermal) |
| **Zebra-Clarity** | Medium | **HIGH** (Physical obstruction) | B02 + B03 |
| **Glint/Roughness** | Medium | **HIGH** (Current modulation) | B02 + B03 |

### Physics Constraints
*   **Depth:** Target is ~34m. This is beyond the SDB bottom-detection limit (~20-30m). Do not expect hull reflectance; look for the plume.
*   **Glint:** Straits currents over a 32ft hull modulate surface roughness. This is **signal**, not noise.

## 3. Run Order & Commands
**Strict adherence to the calibration order is mandatory.**

1.  **Phase 1: Calibration (The "Must-Pass" Test)**
    *   **Target:** `Cedarville` (45.7873°N, -84.6708°W).
    *   **Goal:** If the pipeline does not flag Cedarville, the detector is broken. **Stop and fix.**
2.  **Phase 2: Control Validation**
    *   **Targets:** `Eber Ward`, `William Young`, `M. Stalker`, `Elva` (from `known_wrecks_straits.json`).
    *   **Goal:** Ensure consistency across known shallow wrecks.
3.  **Phase 3: The Mission (Burns Search)**
    *   **Target:** `Robert Burns` (45.87127°N, -84.58642°W).
    *   **Goal:** Identify a candidate within **300m** of the BAG coordinate.

## 4. Acceptance & Burns Check
Success is defined by **Cross-Sensor Confirmation**:
*   **BAG Signal:** Physical anomaly (32ft relief) on the floor.
*   **S2 Signal:** Optical plume/disturbance in the water column.
*   **Acceptance Criteria:** If the S2 pipeline produces a candidate within 300m of the BAG point, the wreck is confirmed. The 300m buffer accounts for 10m resolution coarseness and plume advection.

## 5. Optional GPU (P100)
**Decision Logic:** Only engage P100s if CPU execution fails throughput requirements.

*   **GO (Switch to P100):** If the 32-core Xeon cluster cannot process the 17+ Sentinel-2 scenes within < 2 hours, or if "glint/roughness" requires heavy FFT/convolution.
*   **NO-GO (Stay on CPU):** If the bottleneck is I/O (RAID0 read speed) or GDAL re-projection. GPU acceleration will not fix I/O-bound processes.

## 6. Risks & Do-Not-Trust-Until
**Do NOT trust a Burns hit if:**
1.  **The pipeline failed Cedarville:** The detector is uncalibrated.
2.  **The signal is purely thermal:** For a wood wreck (Burns), a thermal-only hit is likely a false positive.
3.  **The signal lacks temporal stability:** A true wreck plume should show consistent advection patterns across the multi-year stack.
4.  **The hit is a cloud/shadow artifact:** Ensure the `scl` (Scene Classification Layer) mask is applied to exclude clouds.