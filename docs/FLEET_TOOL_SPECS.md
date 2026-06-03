# Fleet Tool Specs — Remaining Satellite Pipeline Detectors

Each section is a self-contained task for one fleet shard. All operate on
**local data already on disk** (no network). Build with:
```
cargo build --release -p cesarops-satellite --features gdal
```

Target: **45.87127°N, -84.58642°W** (Burns, wood, 34m/113ft, 32ft relief).
Calibration: **Cedarville** (45.7873°N, -84.6708°W, steel, 32m/105ft, 588ft).

---

## TOOL 1: SAR Backscatter Anomaly Detector

### Data on disk
`data/straits_sar/sar/rtc_S1A_IW_SLC__1SDV_20240811T232538_...tif` (455MB, Sentinel-1
RTC GeoTIFF, terrain-corrected backscatter, Aug 11 2024).

### What SAR sees on wrecks
- A wreck on the bottom creates a **permanent scatterer** — radar bounces off
  the hull structure differently from the surrounding flat sediment.
- In RTC backscatter: wreck = bright spot (high backscatter) against dark
  (smooth) bottom. Like a rock in a flat field.
- SAR penetrates water in C-band (5.4GHz) to ~1-2 wavelengths (~10cm) so it
  does NOT image the wreck at 34m depth directly. BUT:
- **Surface roughness modulation**: the wreck-driven current perturbs the surface,
  changing wave patterns. SAR is exquisitely sensitive to surface roughness
  (Bragg scattering). A slick/calm patch over a cold upwelling = dark spot.
  A rough patch over current turbulence = bright spot.
- **Multi-temporal SAR**: compare ascending vs descending orbits. A persistent
  brightness/darkness anomaly at the same location across orbits = real target.

### File: `cesarops-satellite/src/sar.rs`

Already has: `SarPoint`, `group_by_orbit`, `run_dbscan`, `calculate_persistence`.
These operate on POINT DETECTIONS already extracted. What's MISSING: the
**extraction step** that reads the RTC GeoTIFF and produces SarPoints.

### Add: `extract_sar_anomalies_local`

```rust
pub fn extract_sar_anomalies_local(
    sar_tif_path: &Path,
    bbox: &BBox,
    threshold_sigma: f64,  // default 3.0
) -> Vec<SarPoint>
```

Logic:
1. Open via GDAL, window to bbox (same `bbox_to_pixel_window` from chip.rs).
2. Read backscatter band as f32.
3. Compute local mean + std in sliding windows (~50px).
4. Threshold: pixels > mean + threshold_sigma*std = bright anomaly.
   Also: pixels < mean - threshold_sigma*std = dark anomaly (calm patch).
5. Cluster adjacent bright/dark pixels (connected components or simple NMS).
6. Emit centroid of each cluster as `SarPoint { lat, lon, orbit: "descending" }`.
   (Orbit direction from filename: `_IW_SLC__1SDV_` = descending typically; 
   or parse from the granule metadata timestamp vs ground track.)

### Wire into mission.rs
Add a SAR stage (or fold into poc_aoi) that calls `extract_sar_anomalies_local`
when `use_local_scenes && sar_dir_exists`, feeds results to `run_dbscan` →
`calculate_persistence`, emits `SarCluster`s into the fusion.

### Acceptance
- Any SarCluster within 500m of Cedarville (large metal target = strong scatterer)
- Check Burns location (may be weaker on wood, but surface roughness should show)

---

## TOOL 2: Landsat Thermal (B10) Cold-Sink Detector

### Data on disk
6 Landsat bundles in `data/straits_multisensor/usgs/LC0{8,9}_L2SP_*.tar.gz`.
These are Level-2 Surface Product bundles containing:
- `*_SR_B*.TIF` — surface reflectance bands
- `*_ST_B10.TIF` — **surface temperature** (Band 10, thermal infrared, 100m res)
- `*_QA_PIXEL.TIF` — quality mask

### What thermal sees on wrecks (especially steel)
A deep steel wreck (like Cedarville at 32m) acts as a COLD SINK — it chills the
water column above it. The thermal disturbance rises to the surface, creating a
persistent cold spot ~0.1-0.5°C below surrounding water temperature. At 100m
Landsat resolution that's detectable as a multi-pixel cold anomaly.

**HEAT SINK (shallow wrecks in the photic zone)**: A wreck above ~20m (within
sunlight penetration) HEATS during the day — the dark hull absorbs solar energy
and warms the water column above it. This shows as a WARM spot in daytime
thermal passes. At night it cools faster than surrounding sediment → COLD spot.
So the thermal detector needs BOTH polarities:
- **Cold anomaly** (z < -2): deep wreck (>20m) OR nighttime shallow wreck
- **Hot anomaly** (z > +2): daytime shallow wreck in photic zone

The SIGN of the anomaly + acquisition time + depth context discriminates:
- Deep wreck (below photic) = ALWAYS cold (day or night)
- Shallow wreck (in photic) = HOT during day, COLD at night
- Both are valid detections — flag the polarity in the candidate output.

**Wood wrecks (Burns at 34m)**: weaker thermal signature, but a large intact hull (32ft
relief) may still produce measurable cooling via current disruption (forced
upwelling of deep cold water). Worth running — "some wood wrecks produce a
signature" per user.

### Add: `concept_thermal_anomaly` in a new file or in `poc.rs`

```rust
pub fn concept_thermal_anomaly(
    thermal_band: &Array2<f32>,  // ST_B10, already in Kelvin/10
    bbox: &BBox,
    scene_date: &str,
    cfg: &PocConfig,
) -> Vec<OpticalCandidate>
```

Logic:
1. Apply Landsat thermal scale: `ST_B10 * 0.00341802 + 149.0` → Kelvin.
   Convert to °C: `K - 273.15`.
2. Compute local mean water temp in ~500m windows (at 100m/px = 5px windows).
3. Z-score each pixel against local mean.
4. Threshold BOTH directions:
   - z < -2.0 = cold anomaly (deep wreck, or nighttime shallow)
   - z > +2.0 = hot anomaly (daytime shallow wreck)
5. NMS / peak extraction → candidates with `concept = "thermal_cold_sink"` or
   `concept = "thermal_heat_sink"` depending on polarity.
6. Tag each candidate with its polarity sign for the fusion stage.

### Prerequisite: Untar Landsat bundles

Before running, extract the `.tar.gz` bundles:
```bash
cd data/straits_multisensor/usgs
for f in *.tar.gz; do mkdir -p "${f%.tar.gz}" && tar xzf "$f" -C "${f%.tar.gz}"; done
```
Then find `*_ST_B10.TIF` in each extracted dir.

### Wire: `load_landsat_thermal_local`

```rust
pub fn load_landsat_thermal_local(
    landsat_dir: &Path,  // parent of extracted bundle dirs
    bbox: &BBox,
    target_px: usize,
) -> Vec<(Array2<f32>, String)>  // (thermal_band, date)
```
- Glob `landsat_dir/LC*/*_ST_B10.TIF`
- For each, decode via GDAL (same decode_local_band pattern)
- Extract date from dirname (LC09_L2SP_022028_**20240701**_...)

### Multi-date thermal persistence
Same principle as optical temporal stack: a real cold sink is PERSISTENT across
all 6 Landsat passes. Count dates where pixel is cold-anomalous. Persistence >
0.5 = candidate.

### Acceptance
- Cedarville (steel, strong cold sink) MUST show a persistent cold anomaly.
- Burns (wood) — check, may show weaker signal.

---

## TOOL 3: SWOT Water Surface Height Anomaly

### Data on disk
120 SWOT granules: `data/straits_multisensor/podaac/swot/SWOT_L2_LR_SSH_Expert_*.nc`
(NetCDF, ~35MB each, covering 2023-2024 historically).

### What SWOT measures
SWOT measures **sea/lake surface height** at ~2km resolution via radar altimetry.
A wreck-driven upwelling/cold-sink creates a LOCAL SURFACE HEIGHT DEPRESSION
(cold water is denser → surface drops ~mm-scale). Not directly imageable at one
pass, but PERSISTENT across passes = detectable in a stack.

### Add: `load_swot_ssh_local`

```rust
pub fn load_swot_ssh_local(
    swot_dir: &Path,
    bbox: &BBox,
) -> Vec<(f64, f64, f64, String)>  // (lat, lon, ssh_meters, date)
```

Logic:
1. For each `.nc` file, open with netcdf-rs (add dep) or shell out to
   `gdalinfo` / `gdal_translate` (GDAL reads NetCDF).
   SWOT L2 SSH variables: `latitude`, `longitude`, `ssha` (sea surface height anomaly).
2. Filter points inside bbox.
3. Collect (lat, lon, ssha, date) tuples.

### Temporal analysis
- Group by approximate location (grid to 2km cells matching SWOT resolution).
- Per cell: compute mean/std of ssha across all 120 passes.
- Cells with PERSISTENTLY NEGATIVE ssha (depression) = cold-sink-driven surface drop.
- These are VERY low resolution (2km) so they won't pinpoint a wreck — they
  CORROBORATE a candidate found by other sensors at higher resolution.

### Wire into fusion
`nasa_fusion.rs::fetch_swot_score` is currently a stub returning 0.5.
Replace with: if a persistent SWOT SSH depression exists within 2km of the
cluster centroid → score 0.8-0.9; otherwise keep 0.5.

### Acceptance
- Not a standalone detector (too coarse). Provides a +0.3 fusion bonus when
  co-located with optical or SAR hits.

---

## TOOL 4: ICESat-2 ATL13 Water Surface/Depth Profile

### Data on disk
120 ATL13 granules: `data/straits_multisensor/podaac/icesat2/ATL13_*.h5.h5`
(HDF5, inland water product, 2018-2024).

### What ICESat-2 ATL13 measures
Photon-counting lidar measuring water surface height along ground tracks at
~100m along-track × ~11m footprint. ATL13 specifically reports:
- Water surface height (reference water body height)
- Significant wave height
- Subsurface signal returns (in very clear water, photons penetrate and return
  from the bottom — this IS bathymetric lidar in clear conditions)

### What it can do for wreck detection
- **Surface depression** (same as SWOT but HIGHER resolution along-track): a
  wreck cold-sink depression in ATL13 surface height that repeats across
  multiple passes/years.
- **Subsurface returns**: in Straits clear-water conditions (Secchi 8-12m),
  ATL13 photons may penetrate and return from objects at 20-30m. The Burns at
  34m is borderline but the Cedarville at 32m might show subsurface scatter.

### Add: `load_icesat2_local`

```rust
pub fn load_icesat2_local(
    atl_dir: &Path,
    bbox: &BBox,
) -> Vec<(f64, f64, f64, f64, String)>  // (lat, lon, surface_ht, subsurface_signal, date)
```

Logic:
1. Open each `.h5` file via HDF5 reader (use `hdf5-rs` crate or shell to
   `h5dump` / GDAL). ATL13 structure:
   - `/gt1l/segment_lat`, `/gt1l/segment_lon`, `/gt1l/ht_water_surf`
   - (6 ground tracks: gt1l, gt1r, gt2l, gt2r, gt3l, gt3r)
2. Filter points inside bbox.
3. Extract surface height + any subsurface photon count if available.

### Temporal stacking
Same persistence approach: grid to ~200m cells, stack surface heights across
120 passes over 6 years, find cells with persistent depression.

### Wire into fusion
Like SWOT: provides a corroborating score in `nasa_fusion.rs`. Persistent
ICESat-2 surface depression within 500m of a candidate → fusion bonus.

---

## TOOL 5: Blue-Green Glint / Current-Roughness Concept

### Data
Same Sentinel-2 blue/green tiles already loaded by the POC stage.

### Physics
Straits currents flowing over a 32ft-relief hull create surface roughness
modulation visible as glint-pattern changes in blue-green reflectance. This is
DISTINCT from the clarity concept (which measures column turbidity). Glint
measures the SURFACE texture itself.

### Add: `concept_glint_roughness` in `poc.rs`

```rust
pub fn concept_glint_roughness(
    b02: &Array2<f32>,  // blue
    b03: &Array2<f32>,  // green
    bbox: &BBox,
    scene_date: &str,
    cfg: &PocConfig,
) -> Vec<OpticalCandidate>
```

Logic:
1. Compute per-pixel blue/green intensity (average of B02+B03 = raw brightness).
2. Compute LOCAL VARIANCE in a 5×5 window = surface roughness proxy.
   High variance = rough (waves/glint modulation). Low variance = smooth (slick).
3. Z-score the variance map: both POSITIVE z (rough anomaly = turbulence over
   wreck) and NEGATIVE z (smooth anomaly = slick/upwelling-calmed surface).
4. The PATTERN matters: a wreck creates a rough-then-smooth dipole aligned with
   current (rough upstream face, smooth wake). Look for high LOCAL GRADIENT of
   the variance map (Sobel on the variance).
5. Threshold on Sobel(variance) = glint transition boundary = wreck-driven
   current disruption.
6. Peak extraction → candidates with `concept = "glint_roughness"`.

### Multi-date
Run across all 9+ scenes. A wreck-driven glint pattern is PERSISTENT (though
it shifts with current direction). Persistence scoring same as other tools.

### Acceptance
- Should fire on ANY large obstruction in current (Cedarville definitely).
- Wood wrecks (Burns): this is the PRIMARY concept — physical obstruction, not
  thermal. This is where Burns should light up.

---

## INTEGRATION: Wire all tools into the fusion

### File: `cesarops-satellite/src/fusion.rs`

Currently `SignalBundle` has: `concept_scores`, `temporal_z`, `drift_proximity`.

Add:
```rust
pub sar_score: Option<f64>,       // from SAR backscatter
pub thermal_score: Option<f64>,   // from Landsat cold-sink
pub swot_score: Option<f64>,      // from SWOT SSH depression
pub icesat2_score: Option<f64>,   // from ICESat-2 surface/subsurface
```

Update `composite_score()`:
```rust
let sar_contrib = self.sar_score.unwrap_or(0.0) * W_SAR;
let thermal_contrib = self.thermal_score.unwrap_or(0.0) * W_THERMAL;
// Add to existing sum
```

Weight constants (tunable via Knobs):
```rust
const W_SAR: f64 = 15.0;      // strong for any material
const W_THERMAL: f64 = 10.0;  // strong for steel, weak for wood
const W_SWOT: f64 = 5.0;      // corroboration only (coarse)
const W_ICESAT2: f64 = 5.0;   // corroboration only
```

### Mission stage ordering
```
download → target_known → poc_aoi (optical clarity + glint) → 
sar_local → thermal_local → temporal_stack → 
fusion (combine all) → validate_gt → report
```

---

## Build / run
```bash
# Build
cargo build --release -p cesarops-satellite --features gdal

# Untar Landsat first
cd data/straits_multisensor/usgs
for f in *.tar.gz; do d="${f%.tar.gz}"; mkdir -p "$d" && tar xzf "$f" -C "$d"; done

# Full multi-sensor run
/data/cargo-target/release/sat-run \
  --spec data/missions/straits_local_run.json \
  --root /data/cesarops/satellite_data
```

## Dependencies to add to Cargo.toml (if needed)
- `netcdf` or use GDAL's NetCDF driver (already available via gdal feature)
- `hdf5` for ICESat-2 (or use GDAL's HDF5 driver — try `Dataset::open` on .h5)
- `rayon` already present

## DO NOT
- Don't hit network (all data is local)
- Don't use red/NIR for deep-target column detection (only for surface glint)
- Don't weight thermal equally for wood vs steel targets
- Don't expect single-pixel precision from SWOT/ICESat-2 (they CORROBORATE)
