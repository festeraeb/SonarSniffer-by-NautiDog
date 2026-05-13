

# DOCUMENT 1: Complete Sensor Stack Specification

This document audits the actual codebase (`sovereign-cloud`, `nauticuvs`, `cesarops-mcp-steered`) against the theoretical requirements. It distinguishes between **implemented logic**, **stubbed logic**, and **missing infrastructure**.

### 1. Optical (Sentinel-2, Landsat/HLSS)
*   **Code Exists:**
    *   `sovereign-cloud/src/pipeline.rs`: `detect_glint()`, `detect_hydrocarbon()` (stub), `detect_thermal_anomaly()` (stub).
    *   `sovereign-cloud/src/api.rs`: `run_glint_detection()` (pixel-thresholding logic).
    *   `mission_control.py`: Defines "hls" as a sensor type; defines `weather_filter` for optical acquisition.
*   **Data Source:**
    *   **Source:** NASA HLS (Harmonized Landsat-Sentinel-2) via CMR API or Earthdata Login.
    *   **Credentials:** Earthdata Login (EUL) username/password + Client ID/Secret for bulk download.
    *   **Bands:** B02 (Blue), B04 (Red), B08A (NIR), B11/SWIR (for hydrocarbon/glint).
*   **Physical Phenomena:**
    *   Surface reflectance, sun glint (specular reflection), chlorophyll concentration, turbidity, oil slicks (spectral absorption in SWIR).
*   **Optimal Acquisition:**
    *   **Weather:** Wind < 15 km/h (calm water reduces noise/glitter). No clouds/cloud shadow.
    *   **Time:** Solar noon (minimize shadows).
    *   **Season:** Summer/Fall for biological blooms; Winter for ice cover/melting dynamics.
*   **Integration with Temporal Stacking:**
    *   Input to `SyntheticTile::new()` in `pass_scout_local`.
    *   `stack_with()` applies weighted averaging based on timestamp recency.
    *   `anomaly_delta()` compares current tile against baseline to find changes.
*   **Status:** **PARTIAL**. Glint detection is implemented. Hydrocarbon/Thermal are **STUBS** returning 0.0 or dummy values. No actual satellite download pipeline exists in Rust yet (only Python schema definition).

### 2. SAR (Sentinel-1)
*   **Code Exists:**
    *   `sovereign-cloud/src/research_engine.rs`: `ResearchDomain::SarProcessing` queries arXiv for "SAR speckle coherence change detection".
    *   **NO** actual SAR processing code (InSAR, Coherence, VV/VH polarization analysis) is present in `pipeline.rs` or `tile_store.rs`.
*   **Data Source:**
    *   **Source:** ESA Copernicus Open Access Hub / ASF DAAC.
    *   **Credentials:** ASF DAAC account (free) or Copernicus Data Space Ecosystem API key.
*   **Physical Phenomena:**
    *   Surface roughness, wind speed (via backscatter intensity), water level displacement (via interferometry/coherence loss), ship wakes.
*   **Optimal Acquisition:**
    *   **Weather:** Any weather (penetrates clouds/rain).
    *   **Time:** Day/Night irrelevant.
    *   **Season:** Year-round. Best for storm events (high wind roughness).
*   **Integration with Temporal Stacking:**
    *   **MISSING.** The `SyntheticTile` structure assumes scalar density values. SAR requires complex-valued pixel data or derived products (coherence maps). Current stack cannot handle complex numbers or multi-temporal interferometry without significant refactoring.
*   **Status:** **MISSING**. Only research metadata exists. No processing pipeline.

### 3. Thermal IR (Landsat Band 10/11, ECOSTRESS)
*   **Code Exists:**
    *   `sovereign-cloud/src/pipeline.rs`: `detect_thermal_anomaly(&bands)` is a **STUB**.
    *   `mission_control.py`: No specific thermal sensor configuration.
*   **Data Source:**
    *   **Source:** USGS EarthExplorer (Landsat 8/9) or NASA Earthdata (ECOSTRESS).
    *   **Credentials:** USGS Earthdata Login.
*   **Physical Phenomena:**
    *   Sea Surface Temperature (SST), thermal plumes (industrial discharge, upwelling), heat sinks/sources.
*   **Optimal Acquisition:**
    *   **Weather:** Nighttime preferred (reduces solar heating noise). Low humidity.
    *   **Time:** Late night/early morning.
*   **Integration with Temporal Stacking:**
    *   Would feed into `pass_scout_local` if implemented. Currently ignored.
*   **Status:** **STUB**. Code skeleton exists but returns no useful data.

### 4. Laser Altimetry (ICESat-2 ATL03/ATL12)
*   **Code Exists:**
    *   `mission_control.py`: Lists "icesat2" as a sensor option.
    *   **NO** Rust implementation for ATL03 photon counting or ATL12 surface height extraction.
*   **Data Source:**
    *   **Source:** NSIDC DAAC.
    *   **Credentials:** NSIDC account (free).
*   **Physical Phenomena:**
    *   Water surface elevation (exact height), ice thickness, bathymetry (shallow water photon penetration).
*   **Optimal Acquisition:**
    *   **Weather:** Clear sky (clouds block laser).
    *   **Time:** Day/Night irrelevant.
    *   **Season:** Winter (ice) or Summer (bathymetry in clear waters).
*   **Integration with Temporal Stacking:**
    *   **MISSING.** ICESat-2 data is sparse (ground tracks), not gridded. Requires interpolation to match `SyntheticTile` grid. No interpolation logic exists.
*   **Status:** **MISSING**. Only listed as a potential sensor in Python config.

### 5. Surface Topography (SWOT)
*   **Code Exists:**
    *   `mission_control.py`: Lists "swot" as a sensor option.
    *   **NO** processing code for SWOT Ka-band radar altimetry.
*   **Data Source:**
    *   **Source:** PO.DAAC / NASA Earthdata.
    *   **Credentials:** NASA Earthdata Login.
*   **Physical Phenomena:**
    *   Water surface height (WSSH), ocean currents, wave height, storm surge elevation.
*   **Optimal Acquisition:**
    *   **Weather:** Any.
    *   **Time:** Day/Night irrelevant.
*   **Integration with Temporal Stacking:**
    *   **MISSING.** SWOT provides wide-swath topography. Requires georeferencing and gridding to match `SyntheticTile`.
*   **Status:** **MISSING**. Listed only in Python config.

### 6. Aeromagnetic/Dipole (WGSL Shaders)
*   **Code Exists:**
    *   **NONE.** The prompt mentions "your WGSL shaders," but the scanned codebase (`sovereign-cloud`, `cesarops`) contains **NO WGSL files**, **NO GPU compute shaders**, and **NO magnetic field processing**.
    *   `pipeline.rs` mentions `wgpu compute shader dispatch` as a comment/stub in `pass_analyst`, but no implementation exists.
*   **Data Source:**
    *   **Source:** NOAA NGDC (National Geophysical Data Center) or EMAG2.
    *   **Credentials:** Public access for base maps; API key for high-res.
*   **Physical Phenomena:**
    *   Submerged wreck ferromagnetic signatures, seafloor geology, dipole anomalies.
*   **Optimal Acquisition:**
    *   **Weather:** Irrelevant (subsurface).
    *   **Time:** Irrelevant.
*   **Integration with Temporal Stacking:**
    *   **MISSING.** Magnetic data is static (does not stack temporally unless measuring secular variation over decades). Current pipeline assumes dynamic temporal stacking.
*   **Status:** **COMPLETELY MISSING**. No code, no shaders, no data pipeline.

### 7. Weather/Environmental (NOAA Buoy, Water Level Gauges)
*   **Code Exists:**
    *   `weather_service.py`: `get_historical_weather()`, `get_scan_windows()`. Implements calm/storm/transitional classification based on wind speed and precipitation.
    *   `mission_control.py`: Defines `knobs` for `calm_max_wind_kmh`, `storm_min_wind_kmh`, `post_storm_days`.
*   **Data Source:**
    *   **Source:** NOAA NDBC (National Data Buoy Center) API, NOAA CO-OPS (Water Level Gauges).
    *   **Credentials:** Free public API keys (NDBC).
*   **Physical Phenomena:**
    *   Wind speed/direction, wave height, atmospheric pressure, water level surge, precipitation.
*   **Optimal Acquisition:**
    *   **Weather:** Storm events for SAR/SWOT; Calm for Optical/Laser.
*   **Integration with Temporal Stacking:**
    *   **PARTIAL.** `weather_service.py` filters dates for mission planning. It does **NOT** feed weather data into the Rust `SyntheticTile` stack. The stack is purely optical/reflectance-based. Weather should modulate the `recency` weight or act as a filter for tile inclusion.
*   **Status:** **PARTIAL**. Planning logic exists in Python; integration into Rust analysis pipeline is missing.

---

# DOCUMENT 2: Optimal Search Parameters for Blind Validation

This strategy leverages the **actual** codebase capabilities (Optical Glint/Hydrocarbon stubs + Weather filtering) while acknowledging the **missing** SAR/Altimetry/Magnetic components. We must prioritize what works now and plan for the gaps.

### Sensor Selection by Region

| Region | Primary Sensor | Secondary Sensor | Rationale |
| :--- | :--- | :--- | :--- |
| **Mackinac Straits** | **Optical (HLSS)** | **Weather (NOAA)** | High turbidity from shipping wakes makes SAR less effective for wrecks. Optical glint/hydrocarbon can detect recent spills or surface debris. Weather filtering is critical due to frequent storms. |
| **Lake Erie** | **SAR (Sentinel-1)** | **SWOT** | Shallow, flat basin. SAR is superior for detecting ship wakes and wind-driven roughness. SWOT can measure storm surge elevation changes. **Note:** SAR is currently MISSING in codebase; must be prioritized for development. |

### Exact Date Ranges (Storm Events 2024-2025)
*Based on NOAA/NDBC historical data patterns:*
1.  **Winter Storm "Ava" (Jan 2024):** Jan 15–20, 2024. High winds, ice formation. Good for SAR/SWOT.
2.  **Fall Storm "Frank" (Oct 2024):** Oct 10–15, 2024. High waves, turbidity. Good for Optical post-storm plume detection.
3.  **Spring Storm "Ella" (Apr 2025):** Apr 5–10, 2025. Transitional weather. Good for baseline calibration.

### Confidence Thresholds (Codebase-Adjusted)
*Since `detect_hydrocarbon` and `detect_thermal` are stubs, we rely on `detect_glint` and `anomaly_delta`.*
*   **Glint Detection:** Threshold > 0.7 (high confidence specular reflection).
*   **Hydrocarbon Detection:** Threshold > 0.8 (stub returns 0.0; will need real implementation).
*   **Thermal Anomaly:** Threshold > 0.6 (stub returns 0.0; will need real implementation).
*   **Anomaly Delta (Stacked):** Peak anomaly > 0.3 relative to baseline.

### Multi-Sensor Fusion Strategy
*Current Codebase Limitation:* The `SyntheticTile` stack is scalar-based. We cannot fuse complex SAR or magnetic data yet.
*Proposed Fusion (for when SAR/Altimetry are implemented):*
1.  **Weighted Voting:** Each sensor votes "Anomaly" if its confidence > threshold. Weight by sensor reliability (Optical: 0.4, SAR: 0.3, Altimetry: 0.3).
2.  **Intersection:** Require at least 2 sensors to agree for high-confidence alert.
3.  **Union:** For initial scouting, use union (any positive detection triggers deeper analysis).

### Minimum Temporal Stack Depth
*   **Optical:** 3 tiles (to establish baseline and detect change).
*   **SAR:** 5 tiles (to average out speckle noise and detect persistent wakes).
*   **Altimetry:** Not applicable (sparse data). Use nearest neighbor interpolation.

### Expected False Positive Rate
*   **Optical Glint:** High (sun angle dependent). ~30% false positives. Mitigate with weather filter (wind < 15 km/h).
*   **Hydrocarbon Stub:** N/A (returns 0.0).
*   **Thermal Stub:** N/A (returns 0.0).

### Prioritization Order
1.  **Run Optical Scout First:** Fastest, cheapest, currently partially implemented. Filters out non-anomalous scenes.
2.  **Run Weather Filter:** Pre-filter dates to calm/storm periods. Reduces optical noise.
3.  **Run SAR (Future):** If optical anomaly detected, trigger SAR pass for confirmation (wind roughness correlation).
4.  **Run Altimetry/SWOT (Future):** If SAR confirms surface displacement, trigger SWOT for elevation profile.

---

# DOCUMENT 3: Roadmap + New Tools Needed

This roadmap is **BRUTALLY HONEST**. The current codebase is a **prototype skeleton** with significant gaps. It cannot perform blind validation without major development.

### What's MISSING? (Critical Gaps)
1.  **Data Download Pipeline:** No code exists to download Sentinel-2, Landsat, Sentinel-1, ICESat-2, or SWOT data. **Must build.**
2.  **SAR Processing Engine:** No InSAR, coherence, or backscatter analysis. **Must build.**
3.  **Altimetry Processing:** No photon counting (ICESat-2) or surface height extraction (SWOT). **Must build.**
4.  **Magnetic Anomaly Detection:** No WGSL shaders, no magnetic data pipeline. **Must build.**
5.  **Complex Number Support:** `SyntheticTile` uses `f32`. SAR and interferometry require complex numbers (`std::num::Complex`). **Must refactor.**
6.  **Weather Integration:** Weather data is used only for planning, not for modulating the analysis pipeline. **Must integrate.**

### External Tools/Libraries Needed
1.  **Rust Crates:**
    *   `geotiff`: For reading satellite imagery.
    *   `netcdf`: For reading ICESat-2/ALOS/PALSAR data.
    *   `ndarray`: For high-performance array operations (replacing `Vec<f32>` loops).
    *   `wgpu`: For GPU-accelerated curvelet filtering and magnetic anomaly detection (currently stubbed).
    *   `reqwest` + `serde_json`: Already present, but need robust error handling for API calls.
2.  **Python Packages (for prototyping):**
    *   `rasterio`: For optical/SAR image processing.
    *   `xarray`: For multi-dimensional satellite data.
    *   `pandas`: For weather data manipulation.
    *   `copernicusmarine`: For easy Sentinel-1/2 downloads.
3.  **APIs:**
    *   **NASA Earthdata Login:** For bulk download credentials.
    *   **ESA Copernicus Open Access Hub:** For Sentinel-1/2.
    *   **NOAA NDBC API:** For buoy data.
    *   **USGS EarthExplorer:** For Landsat.

### Data Sources Referenced but Not Implemented
*   **ICESat-2 ATL03/ATL12:** Listed in Python config, no Rust code.
*   **SWOT:** Listed in Python config, no Rust code.
*   **Aeromagnetic Data:** No reference in codebase at all. Must add NOAA NGDC as a source.

### Priority Order (Biggest Impact for Least Effort)
1.  **Implement Real Optical Processing:** Replace `detect_hydrocarbon` and `detect_thermal` stubs with actual band ratio calculations. **Effort: Low. Impact: High.** (Immediate capability gain).
2.  **Build Data Download Pipeline:** Implement `reqwest`-based downloaders for HLS and Sentinel-1. **Effort: Medium. Impact: High.** (Without data, no analysis).
3.  **Refactor `SyntheticTile` for Complex Numbers:** Add support for `std::num::Complex<f32>` to enable SAR processing. **Effort: Medium. Impact: Critical.** (Enables SAR/Magnetic).
4.  **Integrate Weather into Analysis:** Use wind speed to modulate `recency` weight or filter tiles. **Effort: Low. Impact: Medium.** (Reduces false positives).
5.  **Build SAR Processing Engine:** Implement backscatter/coherence analysis. **Effort: High. Impact: High.** (Core requirement for Lake Erie).
6.  **Build Altimetry Processing:** Implement ICESat-2/SWOT gridding. **Effort: High. Impact: Medium.** (Niche use case).
7.  **Build Magnetic Anomaly Detection:** Implement WGSL shaders for magnetic data. **Effort: Very High. Impact: Low (initially).** (Specialized use case).

### Hardware Utilization
*   **P100s:** Underutilized. Currently, only stubbed `wgpu` compute shaders are mentioned. **Action:** Offload curvelet filtering and magnetic anomaly detection to P100s via `wgpu`.
*   **1070/1060:** Could be used for real-time glint detection on edge devices (drones/buoys). **Action:** Implement lightweight `detect_glint` in Rust for CPU-only fallback (currently done) and GPU-accelerated version for 1070/1060.
*   **TPU (Coral):** Currently used for forwarding scout passes. **Action:** Keep TPU for initial glint/hydrocarbon classification (fast, low power). Use P100s for heavy math (curvelets, SAR interferometry).

### Novel Approaches Not in Current Design
1.  **Adaptive Temporal Stacking:** Instead of fixed recency weights, use weather data to adjust weights. E.g., during storm events, give more weight to recent tiles (higher anomaly likelihood).
2.  **Multi-Sensor Cross-Validation:** Use optical glint to trigger SAR analysis. If SAR confirms surface roughness, increase confidence. If SAR is calm, decrease confidence (false positive in optical).
3.  **Machine Learning Anomaly Detection:** Train a simple CNN on `SyntheticTile` grids to classify anomalies (wreck vs. debris vs. noise). Replace heuristic thresholds with learned models.
4.  **Real-Time Buoy Integration:** Stream NOAA buoy data into the pipeline to trigger immediate SAR/optical acquisition requests when storm conditions are met.