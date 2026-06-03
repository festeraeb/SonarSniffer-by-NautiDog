## Spec ##
**Objective:** Stabilize the `blue_green_clarity` index pipeline for clear-water Straits optical tiles. Address three critical POC failures: uncapped z-scores, edge-induced false positives, and missed preserve wreck detections. Calibrate against known geographic anchors (Cedarville, Burns).

**Key Fixes Implemented:**
1. **Uncapped Z-Scores:** Explicit `np.clip(z_scores, -10, 10)` applied post-normalization. Prevents outlier amplification in clear-water scenes where background variance is low.
2. **Edge False Positives:** Configurable margin mask (`edge_margin_pixels`) applied to z-score maps before anomaly thresholding. Eliminates border artifacts from convolution/windowing or tile stitching.
3. **Preserve Wreck Hits:** Spatial validation layer using geodesic distance checks against known wreck coordinates. Detections within 50m tolerance are flagged as confirmed hits.
4. **Calibration Anchors:** Validates pipeline output against Cedarville (45.7873, -84.6708) and Burns (45.87127, -84.58642). Requires ≥1 detection within 300m of each point to pass calibration gate.

**Assumptions & Dependencies:**
- Tiles are GeoTIFFs with consistent band ordering: `[1:Blue, 2:Green, 3:Red, ...]`
- CRS is WGS84 (EPSG:4326) or transformable via `rasterio`. Projected tiles require `rasterio.warp.reproject`.
- Dependencies: `rasterio>=1.3`, `numpy>=1.24`, `scipy>=1.10`, `geopy>=2.3`, `json

<!-- reasoning -->
Here's a thinking process:

1.  **Analyze User Input:**
   - **Role/Context:** "Qwen lead (P100). Spec for 1070 Qwen2.5-Coder." This suggests I should act as a lead developer/architect, focusing on high-quality, production-ready code following specifications.
   - **Task:** "Fix cesarops-satellite POC blue_green_clarity for clear-water Straits: uncapped z-scores (peaks >10), edge false positives, no hits on preserve wrecks. Calibrate Cedarville 45.7873,-84.6708; check Burns 45.87127,-84.58642 within 300m. Data: NFS straits_optical_* local tiles; mission data/missions/straits_local_run.json."
   - **Key Issues to Fix:**
     - Uncapped z-scores (peaks > 10)
     - Edge false positives
     - No hits on preserve wrecks
   - **Calibration/Validation Points:**
     - Cedarville: 45.7873, -84.6708
     - Burns: 45.87127, -84.58642 (check within 300m)
   - **Data Sources:**
     - NFS `straits_optical_*` local tiles
     - Mission config: `data/missions/straits_local_run.json`
   - **Domain:** Satellite imagery analysis, specifically "blue_green_clarity" (likely water clarity/turbidity index), "cesarops-satellite POC" (likely a project/codebase name), "clear-water Straits" (Geographical context: likely Lake Huron's Straits of Mackinac or similar, given coordinates ~45.8N, 84.6W).

2.  **Identify Core Technical Requirements:**
   - Need to process optical satellite tiles (likely GeoTIFFs or similar).
   - Compute a "blue_green_clarity" index (likely based on blue/green band ratios or differences).
   - Apply z-score normalization but cap it to prevent extreme values (>10).
   - Handle edge effects (false positives at tile boundaries).
   - Ensure detection of specific features (preserve wrecks).
   - Validate against known coordinates (Cedarville, Burns).
   - Read mission config from JSON.
   - Output should be a robust, well-structured Python script/module following best practices.

3.  **Determine Missing Information & Assumptions:**
   - I don't have the actual codebase. I need to create a complete, self-contained solution that addresses the spec.
   - I'll assume standard geospatial processing libraries: `rasterio`, `numpy`, `geopandas`, `scipy`, `json`.
   - "blue_green_clarity" likely means a water clarity index derived from blue and green bands. I'll define a reasonable formula (e.g., normalized difference or ratio) and explain it.
   - Z-score capping: `np.clip(z_scores, -10, 10)` or similar.
   - Edge false positives: Typically caused by border artifacts in convolution/window operations. I'll add a margin mask or use `scipy.ndimage` with proper padding, or explicitly mask tile edges.
   - Preserve wrecks: Likely specific coordinates or polygons. I'll add a validation step to check if detected anomalies match known wreck locations within a tolerance.
   - Mission JSON: I'll define a schema and show how to load it.
   - Coordinates: Cedarville (45.7873, -84.6708), Burns (45.87127, -84.58642). I'll use `haversine` or `geopy` for distance checks.

4.  **Design the Solution Architecture:**
   - **Config Loader:** Read `straits_local_run.json`
   - **Tile Processor:** Load optical tiles, compute blue-green clarity index
   - **Z-Score Calculator:** Compute per-tile or global z-scores, apply cap
   - **Edge Masking:** Remove false positives from tile boundaries
   - **Wreck Validator:** Check detected anomalies against preserve wreck coordinates
   - **Calibration/Validation:** Verify against Cedarville and Burns coordinates within 300m
   - **Output:** Save results, log metrics, handle errors gracefully

   I'll structure it as a class-based pipeline for modularity and testability.

5.  **Draft the Code (Mental Refinement):**
   ```python
   import json
   import numpy as np
   import rasterio
   from rasterio.windows import from_bounds
   from scipy import stats
   from geopy.distance import geodesic
   import logging
   from pathlib import Path
   from typing import Dict, List, Tuple, Optional
   import warnings

   logging.basicConfig(level=logging.INFO)
   logger = logging.getLogger(__name__)

   class StraitsClarityProcessor:
       def __init__(self, mission_config_path: str):
           self.config = self._load_config(mission_config_path)
           self.tile_dir = Path(self.config.get("tile_dir", "data/straits_optical_tiles"))
           self.z_score_cap = self.config.get("z_score_cap", 10.0)
           self.edge_margin = self.config.get("edge_margin_pixels", 10)
           self.wreck_coords = self.config.get("preserve_wrecks", [])
           self.calibration_points = {
               "Cedarville": (45.7873, -84.6708),
               "Burns": (45.87127, -84.58642)
           }
           self.results = []

       def _load_config(self, path: str) -> dict:
           with open(path, 'r') as f:
               return json.load(f)

       def _compute_clarity_index(self, tile_data: np.ndarray) -> np.ndarray:
           # Assuming tile_data shape: (bands, height, width)
           # Blue band index 2, Green band index 1 (common in Landsat/Sentinel)
           # Adjust based on actual band order in config
           blue = tile_data[2]
           green = tile_data[1]
           # Avoid division by zero
           mask = (green > 0) & (blue > 0)
           clarity = np.zeros_like(blue, dtype=np.float32)
           clarity[mask] = (green[mask] - blue[mask]) / (green[mask] + blue[mask])
           return clarity

       def _apply_zscore_cap(self, clarity_map: np.ndarray) -> np.ndarray:
           mean = np.nanmean(clarity_map)
           std = np.nanstd(clarity_map)
           if std == 0:
               return np.zeros_like(clarity_map)
           z_scores = (clarity_map - mean) / std
           return np.clip(z_scores, -self.z_score_cap, self.z_score_cap)

       def _mask_edges(self, z_scores: np.ndarray) -> np.ndarray:
           h, w = z_scores.shape
           margin = self.edge_margin
           mask = np.ones_like(z_scores, dtype=bool)
           mask[:margin, :] = False
           mask[-margin:, :] = False
           mask[:, :margin] = False
           mask[:, -margin:] = False
           z_scores[~mask] = np.nan
           return z_scores

       def _validate_wrecks(self, detections: List[Tuple[float, float]]) -> Dict[str, bool]:
           hits = {}
           for wreck in self.wreck_coords:
               wreck_lat, wreck_lon = wreck
               found = False
               for det_lat, det_lon in detections:
                   dist = geodesic((wreck_lat, wreck_lon), (det_lat, det_lon)).meters
                   if dist < 50: # 50m tolerance for detection
                       found = True
                       break
               hits[f"wreck_{wreck_lat}_{wreck_lon}"] = found
           return hits

       def _check_calibration(self, detections: List[Tuple[float, float]]) -> Dict[str, float]:
           results = {}
           for name, (lat, lon) in self.calibration_points.items():
               min_dist = float('inf')
               for det_lat, det_lon in detections:
                   dist = geodesic((lat, lon), (det_lat, det_lon)).meters
                   min_dist = min(min_dist, dist)
               results[name] = min_dist
           return results

       def process_tile(self, tile_path: str) -> Dict:
           with rasterio.open(tile_path) as src:
               clarity_map = self._compute_clarity_index(src.read())
               z_scores = self._apply_zscore_cap(clarity_map)
               z_scores = self._mask_edges(z_scores)

           # Find anomalies (e.g., z > 2)
           anomalies = np.argwhere(z_scores > 2)
           detections = []
           for r, c in anomalies:
               # Convert pixel to lat/lon
               x, y = src.xy(r, c)
               detections.append((y, x)) # rasterio xy returns (lon, lat) -> (x, y) -> (lon, lat)
               # Actually rasterio.xy(row, col) returns (x, y) which is (lon, lat) for CRS=EPSG:4326
               # But tiles are usually projected. I'll assume src.crs is known or use transform.
               # For simplicity, I'll use src.xy(r, c) and assume it returns (lon, lat) or handle projection.
               # Let's be precise: src.xy(r, c) returns (x, y) in CRS units.
               # I'll add a note about CRS handling.
           # ...
   ```

   Wait, I need to be more careful with coordinate conversion. Satellite tiles are usually in a projected CRS (e.g., UTM). I'll add a proper geotransform handling or assume the mission config specifies the CRS. I'll make it robust by using `rasterio.transform.rowcol` or `src.xy`.

   Also, the spec mentions "blue_green_clarity" specifically. I'll define it clearly and allow configuration.
   The "preserve wrecks" likely refers to specific known shipwreck locations in the Straits of Mackinac. I'll add a placeholder list that can be populated from config.
   The "uncapped z-scores (peaks >10)" fix is straightforward: `np.clip`.
   "Edge false positives" fix: margin masking.
   "Calibrate Cedarville... check Burns within 300m": I'll add a validation function that checks if detected anomalies fall within 300m of these points.

   Let's structure the code properly with type hints, logging, error handling, and clear separation of concerns. I'll also include a test/validation section as requested by the prompt format ("Spec for 1070 Qwen2.5-Coder. ## Spec ## Files ## Tests.").

6.  **Refine & Structure Output:**
   I'll follow the requested format:
   - `## Spec ##` (clarify assumptions & approach)
   - `## Files ##` (provide the complete code)
   - `## Tests ##` (provide validation/test code)
   - Ensure it addresses all constraints: uncapped z-scores, edge false positives, preserve wrecks, calibration points, data paths.

   Key improvements during drafting:
   - Use `rasterio` properly for geospatial conversion.
   - Handle CRS transformation explicitly.
   - Make z-score capping configurable but default to 10.
   - Edge masking with configurable margin.
   - Wreck detection with distance threshold.
   - Calibration check with 300m tolerance.
   - Add comprehensive docstrings and logging.
   - Ensure it's production-ready (P100 standard).

   I'll write the code carefully, checking for common pitfalls:
   - `src.xy()` returns (x, y) in CRS units. If CRS is WGS84, it's (lon, lat). If projected, need to transform. I'll add a `crs_transform` helper or assume WGS84 for simplicity but note it.
   - Z-score calculation: should be per-tile or global? Per-tile is safer for local tiles. I'll do per-tile but allow global mode.
   - Edge masking: `np.pad` or direct slicing. Direct slicing is fine.
   - Wreck validation: use `geopy.distance.geodesic` or `haversine`. I'll use `geopy` but note it requires installation, or use `numpy` haversine for zero-dependency beyond core. I'll stick to `geopy` for accuracy but provide a fallback or note it. Actually, `scipy.spatial.distance` or `numpy` is fine. I'll use `geopy` for clarity.

   Let's draft the complete response. I'll structure it exactly as requested.
   I'll make sure the code is self-contained, handles the JSON config, processes tiles, applies fixes, and validates against the specified coordinates.
   I'll add comments explaining the fixes for each issue.
   I'll ensure type hints and PEP8 compliance.
   I'll include a test suite that mocks the data and validates the logic.

   Structure:
   ```
   ## Spec ##
   - Approach summary
   - Key fixes addressed
   - Assumptions & dependencies

   ## Files ##
   - `straits_clarity_processor.py` (main module)
   - `straits_local_run.json` (config schema/example)

   ## Tests ##
   - `test_straits_clarity.py` (unit/integration tests)
   ```

   I'll write it out carefully now. 
   Self-Correction/Verification during thought:
   - "uncapped z-scores (peaks >10)" -> The fix is explicitly capping at ±10. I'll implement `np.clip(z_scores, -10, 10)`.
   - "edge false positives" -> I'll implement a configurable edge margin mask.
   - "no hits on preserve wrecks" -> I'll implement a spatial join/distance check against known wreck coordinates.
   - "Calibrate Cedarville 45.7873,-84.6708; check Burns 45.87127,-84.58642 within 300m" -> I'll add a validation step that checks if detected anomalies fall within 300m of these points.
   - Data paths: `straits_optical_*` local tiles; `data/missions/straits_local_run.json`.
   - I'll make the code robust to missing geospatial libraries by using standard ones (`rasterio`, `numpy`, `scipy`, `json`, `pathlib`, `logging`).
   - I'll add a `run_pipeline()` function that ties it all together.
   - I'll ensure the JSON config matches the expected structure.
   - I'll write tests that mock rasterio and verify the logic.

   All set. Generating response. 
   Output matches the refined structure.✅
   Code will be production-grade, well-documented, and directly address every constraint.📝
   I'll make sure to explicitly call out how each issue is fixed in the code/comments.
   I'll use `geopy.distance.geodesic` for accurate distance calculations, but note it in requirements. Alternatively, I'll implement a simple haversine to avoid external deps beyond core stack. I'll stick to `geopy` for accuracy but provide a fallback note. Actually, `scipy.spatial.distance` or `numpy` is fine. I'll use `geopy` for clarity.
   I'll structure the JSON config to include tile_dir, z_score_cap, edge_margin, preserve_wrecks, calibration_points.
   Ready.✅
   Output generation. 
   [Proceeds]
