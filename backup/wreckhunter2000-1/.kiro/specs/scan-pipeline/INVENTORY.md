# Scan/Detection Pipeline — File Inventory & Status

## Generated: May 13, 2026
## Purpose: Organize scattered scan/detection code for true implementation spec

---

## WORKING IMPLEMENTATIONS (proven, use as-is or refactor into final pipeline)

### Python — Core Detection
| File | What it does | Status |
|------|-------------|--------|
| `scan_engine.py` | Unified 7-pass detection engine (anomaly, hydrocarbon, Stumpf bathymetry, NauticUVs LoG, SWIR silt erasure, mussel clearspot, triple-lock fusion). Full KMZ output. | ✅ WORKING |
| `lake_michigan_scan.py` | Core processing functions (imported by scan_engine) | ✅ WORKING |
| `lake_erie_scan.py` | Erie-specific (SWIR silt erasure, mussel clearspot) | ✅ WORKING |
| `triple_lock_fusion.py` | Multi-sensor fusion (thermal+SAR+optical) | ✅ WORKING |
| `weather_service.py` | Open-Meteo integration, storm/calm/post-storm classification | ✅ WORKING |
| `universal_downloader.py` | 5-source satellite downloader (ASF, Copernicus, PO.DAAC, USGS, HLS) | ✅ WORKING |
| `wreck_ml_trainer.py` | ML model training for wreck detection | ✅ WORKING |

### Python — Magnetics (pipelines/mag/)
| File | What it does | Status |
|------|-------------|--------|
| `erie_scanner_pipeline.py` | Erie magnetic scanning | ✅ WORKING |
| `dipole_analysis.py` | Dipole detection and analysis | ✅ WORKING |
| `flight_line_physics_v2.py` | Flight line correction (v2 is latest) | ✅ WORKING |
| `datum_correction.py` | Datum correction for mag data | ✅ WORKING |
| `erie_wellhead_discriminator.py` | Wellhead vs wreck discrimination | ✅ WORKING |
| `final_magnetic_fusion.py` | Final fusion of magnetic sources | ✅ WORKING |
| `loran_c_warp.py` | LORAN-C coordinate warping | ✅ WORKING |
| `crossmatch_optical_mag.py` | Cross-match optical with magnetics | ✅ WORKING |

### Python — Satellite (pipelines/satellite/)
| File | What it does | Status |
|------|-------------|--------|
| `batch_download_manager.py` | Batch download management | ✅ WORKING |
| `nasa_earthdata_client.py` | NASA Earthdata API client | ✅ WORKING |
| `fetch_swot_data.py` | SWOT data fetching | ✅ WORKING |
| `fetch_ecostress_data.py` | ECOSTRESS thermal data | ✅ WORKING |
| `sar_temporal_persistence.py` | SAR temporal persistence analysis | ✅ WORKING |

### Python — Bathymetry (pipelines/bag/)
| File | What it does | Status |
|------|-------------|--------|
| `bag_wreck_detector.py` | BAG file wreck detection | ✅ WORKING |
| `advanced_bag_scanner.py` | Advanced BAG scanning | ✅ WORKING |
| `atl23_extract.py` | ICESat-2 ATL23 extraction | ✅ WORKING |

### Python — ML (ml/)
| File | What it does | Status |
|------|-------------|--------|
| `ml/inference/deep_water_detection.py` | Deep water wreck detection | ✅ WORKING |
| `ml/inference/wreck_vs_obstruction_classifier.py` | Classification | ✅ WORKING |
| `ml/training/train_wreck_classifier_gpu.py` | GPU training | ✅ WORKING |

### Rust — Detection
| Crate/File | What it does | Status |
|-----------|-------------|--------|
| `sentinel_hunt_src/src/detect.rs` | Full STAC→weather→fetch→detect pipeline | ✅ WORKING |
| `sentinel_hunt_src/src/weather.rs` | Weather client for scan windows | ✅ WORKING |
| `sentinel_hunt_src/src/stac.rs` | STAC API client | ✅ WORKING |
| `cesarops-detection/src/pipeline.rs` | Triple-Lock pipeline (Scout→Validator→Jitter) | ✅ WORKING |
| `sovereign-cloud/src/pipeline.rs` | 4-pass pipeline with TPU offload | ✅ WORKING |
| `sovereign-cloud/src/tile_store.rs` | Sled-backed tile store | ✅ WORKING |
| `cesarops-inference/src/geo_filter.rs` | FFT bandpass + dipole detection | ✅ WORKING |

### Vision Workers
| File | What it does | Status |
|------|-------------|--------|
| `vision-workers/scout_1060.py` | GTX 1060 Florence-2 scout | ✅ WORKING |
| `vision-workers/validator_p1000.py` | P1000 Moondream2 validator | ✅ WORKING |

---

## STUBS/SKELETONS (need real implementation)

| File | What it claims to do | Why it's a stub |
|------|---------------------|-----------------|
| `cesarops-inference/src/satellite_stitch.rs` | Grid alignment via curvelets | Returns placeholder offsets |
| `cesarops-inference/src/optical_mass.rs` | Thermocline jitter + tonnage | Returns hardcoded 0.008432, 12450.0 |
| `cesarops-inference/src/magnetic_eraser.rs` | Sub-nT anomaly extraction | Returns hardcoded 0.000321 |
| `cesarops-drift-worker/src/main.rs` | Drift correction worker | Minimal skeleton |
| `cesarops-historical-worker/src/main.rs` | Historical data processing | Minimal skeleton |

---

## DUPLICATES (pick one, delete the rest)

### Files duplicated between `programming/` root and `wreckhunter2000-1/`:
- universal_downloader.py, weather_service.py, tile_geometry.py, tile_selector.py
- cmr_search.py, swot_ssh_extractor.py, crossref_scans.py
- hls_dl.py, hls_dl2.py, hls_download.py, hls_download2.py, hls_download3.py
- download_erie_multiyear.py, download_straits_2024.py
- hard_pixel_audit.py, wreck_pixel_probe.py, wreck_ml_trainer.py
- scan_wreck_db.py, smart_search_planner.py, background_probe.py
- bridge_calibrate.py, bridge_proximity.py, triple_lock_fusion.py
- lake_michigan_scan.py, match_wrecks.py, inspect_scan.py

### Directories fully duplicated:
- `pipelines/` (both locations)
- `cesarops_core/` (both locations)
- `sensors/` (both locations)
- `sentinel_hunt_src/` (both locations)

### cesarops-slicer exists in THREE places:
1. `wreckhunter2000-1/cesarops-slicer/` ← MOST COMPLETE (has specialists)
2. `programming/cesarops-slicer/` (minimal)
3. `programming/cesarops/slicer/` (minimal)

---

## NOT IN WORKSPACE (need to be added to Cargo.toml workspace or kept separate)

These Rust crates exist but aren't in the workspace `members` list:
- `cesarops-detection` — Triple-Lock pipeline
- `cesarops-satellite-worker` — Satellite tile processing
- `cesarops-drift-worker` — Drift correction (stub)
- `cesarops-aeromagnetic-worker` — Mag processing + GPU shader
- `cesarops-slicer` — Tile slicing + specialists
- `sovereign-cloud` — Pipeline orchestration + tile store
- `sentinel_hunt_src` — Full detection pipeline
- `warp-grid` — GPU compute pool
- `cesarops-hybrid-engine` — Spatial engine + curvelet shaders
- `nauticuvs` — Curvelet transform library
- `cesarops-db-core` — Database layer

---

## GPU SHADERS (for P100 detection pipeline)

| File | Purpose |
|------|---------|
| `cesarops-inference/shaders/geo_filter.wgsl` | Geological bandpass filter |
| `cesarops-aeromagnetic-worker/src/dipole_shader.wgsl` | Dipole detection |
| `cesarops-hybrid-engine/src/shaders/curvelet_f64.wgsl` | Curvelet transform (f64) |
| `cesarops-hybrid-engine/src/shaders/nauticus_scanner.wgsl` | Scanner |
| `warp-grid/shaders/pascal/flash_attention.wgsl` | Flash attention (P100 f16) |
| `warp-grid/shaders/pascal/matmul_half2.wgsl` | f16 matmul (P100) |
| `warp-grid/shaders/generic/matmul_f32.wgsl` | Generic f32 matmul |

---

## CONFIGURATION FILES

| File | Purpose |
|------|---------|
| `sensor_config.json` | Sensor definitions |
| `satellite_data_sources.json` | Data source URLs/auth |
| `known_wrecks.json` | Known wreck database (validation) |
| `known_wrecks_erie.json` | Erie-specific known wrecks |
| `sensors/sensor_master_list.json` | Master sensor list |

---

## RECOMMENDED ORGANIZATION

### Keep in wreckhunter2000-1/ (canonical location):
```
wreckhunter2000-1/
├── scan_engine.py              # Python unified scanner
├── universal_downloader.py     # Satellite data acquisition
├── weather_service.py          # Weather-driven scheduling
├── pipelines/
│   ├── mag/                    # Magnetic analysis (~45 scripts)
│   ├── satellite/              # Satellite processing (~25 scripts)
│   └── bag/                    # Bathymetry (~21 scripts)
├── ml/
│   ├── inference/              # Detection models
│   └── training/               # Model training
├── vision-workers/             # GPU vision workers
├── sentinel_hunt_src/          # Rust detection pipeline
├── cesarops-detection/         # Rust triple-lock
├── cesarops-slicer/            # Rust tile slicer + specialists
├── cesarops-aeromagnetic-worker/  # Rust mag + GPU shader
├── sovereign-cloud/            # Rust orchestration + tile store
├── warp-grid/                  # Rust GPU compute pool
├── nauticuvs/                  # Rust curvelet library
└── cesarops-hybrid-engine/     # Rust spatial + curvelet shaders
```

### Delete from programming/ root (duplicates):
Everything in `programming/` that also exists in `wreckhunter2000-1/` — the wreckhunter2000-1 copy is canonical.

### Add to Cargo workspace:
At minimum, add these to `Cargo.toml` workspace members for the scan pipeline:
- `sentinel_hunt_src`
- `cesarops-detection`
- `sovereign-cloud`
- `warp-grid`

---

## NEXT: What needs to be spec'd for true implementation

1. **End-to-end pipeline wiring** — connect weather_service → universal_downloader → scan_engine → detection → reporting
2. **Drift correction** — the stub in satellite_stitch.rs needs real curvelet-based sub-pixel alignment
3. **GPU acceleration** — wire the WGSL shaders (geo_filter, dipole, curvelet) into the Rust pipeline via warp-grid
4. **Autonomous scheduling** — AI decides when to scan based on weather windows (the steering doc defines the strategy, code needs to implement it)
5. **Temporal stacking** — 20+ day tile stacking with drift-corrected alignment
6. **Validation** — blind scan against known wreck locations to measure accuracy
