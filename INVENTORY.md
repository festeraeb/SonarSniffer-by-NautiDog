# Scan/Detection Pipeline — File Inventory & Status

## Updated: May 30, 2026
## Purpose: Full inventory including pipeline tools, frontend, SonarSniffer, server, and backup/laptop dumps

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
| `cesarops-drift-worker/src/main.rs` | Drift correction worker | Minimal skeleton |
| `cesarops-historical-worker/src/main.rs` | Historical data processing | Minimal skeleton |

---

## COMPLETED IMPLEMENTATIONS (verified 2025)

These files were previously listed as stubs but are **fully implemented**:

| File | What it does | Lines | Tests |
|------|-------------|-------|-------|
| `cesarops-inference/src/satellite_stitch.rs` | Sub-pixel drift correction via 2D FFT phase correlation, parabolic sub-pixel refinement | 501 | 5 |
| `cesarops-inference/src/optical_mass.rs` | Thermocline jitter via 32x32 box-mean + Beer-Lambert attenuation, tonnage estimation | 251 | 7 |
| `cesarops-inference/src/magnetic_eraser.rs` | 2D moving-median baseline erasure (two-pass row+col), anomaly center extraction | 342 | ✅ |
| `cesarops-aeromagnetic-worker/src/dipole_analysis.rs` | Magnetic dipole classification: lobe separation, polarity flip distance, gradient sharpness, PCA aspect ratio, man-made scoring | 534 | 3 |
| `cesarops-detection/src/bag_scanner.rs` | BAG (HDF5) file reader, background estimation, anomaly detection, BFS clustering, UTM→WGS84 | 627 | 3 |

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

---
---

# PART 2 — FULL CODEBASE AUDIT (May 30, 2026)
# Covers: server, frontend, SonarSniffer, loose pipeline tools, laptop dumps

---

## THE MAIN SERVER — `wrecks_api/app.py` (4060 lines)

The live FastAPI server that ties everything together. Runs at `127.0.0.1:8099`.
This is NOT just a database reader — it is the full pipeline orchestrator.

### What it currently has (working endpoints):
| Tag | Endpoints | Status |
|-----|-----------|--------|
| wrecks | GET /wrecks, /wrecks/{id}, /wrecks/search, /wrecks/bbox, /wrecks/magnetic, /wrecks/steel-freighters | ✅ |
| map | GET /wrecks/map/geojson | ✅ |
| scan | POST /scan/start, GET /scan/status/{id}, /scan/results/{id}, /scan/audit/yesterday | ✅ |
| scan | POST /scan/results/{id}/restore | ✅ |
| tools | POST /tools/swarm-mag/fetch, GET /tools/swarm-mag/status/{id} | ✅ |
| tools | POST /tools/mag-pipeline/run, GET /tools/mag-pipeline/status/{id} | ✅ |
| forge | /health, /monitor, /validate/*, /cluster/* (proxy to Forge) | ✅ |
| workers | GET /jobs, POST /jobs, /jobs/claim, /jobs/{id}/finish | ✅ |
| workers | POST /workers/heartbeat, GET /workers/online | ✅ |
| telemetry | POST/GET /telemetry | ✅ |
| agent | POST /tools/agent/request, /tools/agent/probe, /tools/mission/run, /tools/run-task | ✅ |
| agent | GET /tools/nodes/check, /tools/agent/provider-status, /tools/work-dir | ✅ |
| search | POST /search, GET /search/{id}, /search/{id}/report, /search/idle/status | ✅ |
| hardware | GET /hardware | ✅ |
| kobold | GET/POST /kobold/status, /kobold/launch, /kobold/download-model, /kobold/upload-model | ✅ |
| watchdog | GET /watchdog/status, POST /watchdog/start-api, /watchdog/restart | ✅ |

### What the server is MISSING (endpoints that panels expect but don't exist):
| Missing Endpoint | Needed By | Priority |
|-----------------|-----------|----------|
| POST /sonar/parse | SonarPanel (not built yet) | HIGH |
| GET /sonar/jobs/{id} | SonarPanel | HIGH |
| POST /bag/scan | AutoBagPrompt panel | MEDIUM |
| GET /bag/scan/{id} | AutoBagPrompt panel | MEDIUM |
| GET /satellite/temporal-stack | SatellitePanel (not built yet) | MEDIUM |
| POST /loran/correct | LoranPanel in Tauri frontend | LOW |

### Pipeline stage registry (wrecks_api/pipeline_stages.py):
| Stage | Status |
|-------|--------|
| `mag_pipeline` | ✅ registered and wired |
| `pdf_breaker` | ✅ `wrecks_api/stages/pdf_breaker_stage.py` EXISTS but NOT in STAGE_REGISTRY |
| `bag_scanner` | ❌ Not registered — Rust `bag_scanner.rs` exists but no Python stage wrapper |
| `satellite` | ❌ Not registered — `pipelines/satellite/temporal_stack_engine.py` exists |
| `sonar` | ❌ Not registered — SonarSniffer parsers exist but aren't exposed |

**Fix needed**: Add to `STAGE_REGISTRY` in `wrecks_api/pipeline_stages.py`:
```python
from wrecks_api.stages.pdf_breaker_stage import run_pdf_breaker_stage
from wrecks_api.stages.bag_scanner_stage import run_bag_scanner_stage     # needs to be created
from wrecks_api.stages.satellite_stage import run_satellite_stage          # needs to be created

STAGE_REGISTRY = {
    "mag_pipeline": run_mag_pipeline_stage,
    "pdf_breaker": run_pdf_breaker_stage,    # already written, just wire it
    "bag_scanner": run_bag_scanner_stage,
    "satellite": run_satellite_stage,
}
```

---

## THE DESKTOP FRONTEND — Tauri React App (`backup/deploy/tools/cesarops-core-github/tauri/`)

Full Tauri v2 desktop app: `com.cesarops.wreckhunter`. React 18 + TypeScript.
Talks to `wrecks_api` via `src/services/api.ts` using `ensure_backend` Tauri command.

**STATUS**: Lives in `backup/`. NOT in the live workspace. Needs to be moved to `cesarops-tauri/`.

### Panels already built (src/components/):
| Component | What it does | Backend endpoint |
|-----------|-------------|-----------------|
| `WreckList.tsx` | Paginated wreck table with filters | GET /wrecks |
| `WreckDetail.tsx` | Single wreck detail view | GET /wrecks/{id} |
| `MapPanel.tsx` | GeoJSON map overlay | GET /wrecks/map/geojson |
| `StatsPanel.tsx` | Database stats dashboard | GET /stats |
| `ScanPanel.tsx` | Trigger scan jobs, poll status | POST /scan/start, GET /scan/status/{id} |
| `MagPipelinePanel.tsx` | Mag pipeline trigger + monitor | POST /tools/mag-pipeline/run |
| `AgentPanel.tsx` | LLM agent interface | POST /tools/agent/request |
| `HarvesterPanel.tsx` | Satellite data harvester | POST /tools/swarm-mag/fetch |
| `EriePanel.tsx` | Erie-specific operations | Various |
| `ExportPanel.tsx` | KML/GeoJSON export | Scan results |
| `LoranPanel.tsx` | LORAN-C coordinate correction | POST /loran/correct (MISSING) |
| `PDFBreakerPanel.tsx` | PDF redaction breaker | POST /scan/start with pdf config |
| `RestorationPanel.tsx` | Restoration candidate workflow | POST /scan/results/{id}/restore |
| `AutoBagPrompt.tsx` | BAG file drop + auto scan | POST /bag/scan (MISSING) |
| `ExtendedSensorsPanel.tsx` | Sensor config view | GET /hardware |

### What frontend panels are MISSING:
- `SonarPanel.tsx` — drag-drop sonar files, parse, visualize waterfall, flag targets
- `SatellitePanel.tsx` — temporal stack viewer, SWOT/ICESat-2 overlays
- `BathymetryPanel.tsx` — BAG file viewer with wreck overlay on bathymetry grid

### Tauri Rust backend (`src-tauri/src/`):
| File | Purpose | Status |
|------|---------|--------|
| `main.rs` → `lib.rs` | Bootstrap, invoke `ensure_backend` | ✅ |
| `lib.rs` | `ensure_backend` command — starts uvicorn, returns URL | ✅ (in backup) |
| `erie_model.rs` | Erie-specific ML model inference | ✅ (in backup) |
| `kml_generator.rs` | KML export from wreck results | ✅ (in backup) |

---

## SONAR SNIFFER — `backup/sonarsniffer/SonarSniffer/`

Full Tauri desktop sonar processing app. Different product from CESAROPS but uses same curvelet lib.
The sonar parsers and target detection Rust code should be integrated into `cesarops-detection`.

### Rust backend (`src-tauri/src/`):
| File | Purpose | Status |
|------|---------|--------|
| `humminbird_parser.rs` | Humminbird DAT+SON (B000-B004 channels, GPS decoding) | ✅ WORKING |
| `lowrance_parser.rs` | Lowrance SL2/SL3 format | ✅ WORKING |
| `garmin_rsd_parser.rs` | Garmin sonar data format | ✅ WORKING |
| `cerulean_parser.rs` | Cerulean multibeam sonar | ✅ WORKING |
| `jsf_parser.rs` | EdgeTech JSF sidescan sonar | ✅ WORKING |
| `xtf_parser.rs` | XTF (eXtended Triton Format) — marine survey | ✅ WORKING |
| `format_detector.rs` | Auto-detects sonar file format from magic bytes | ✅ WORKING |
| `target_detection.rs` | Blob detection on sonar pings — size/depth classification | ✅ WORKING |
| `channel_alignment.rs` | Align port/starboard sidescan channels | ✅ WORKING |
| `channel_discovery.rs` | Discover available channels from multi-file session | ✅ WORKING |
| `curvelet_diag.rs` | Curvelet-based sonar anomaly diagnostics | ✅ WORKING |
| `healing_api.rs` | Gap-fill sonar data dropouts | ✅ WORKING |
| `mosaic/` | Geo-mosaic sonar tiles | ✅ WORKING |
| `outputs.rs` | KML, GeoJSON, MBTiles, CSV export | ✅ WORKING |
| `video.rs` / `video_enhanced.rs` | Waterfall GIF/MP4 generation | ✅ WORKING |
| `corpus_scan.rs` | Batch scan directory of sonar files | ✅ WORKING |
| `probing.rs` | Probe sonar files before full parse | ✅ WORKING |
| `static_server.rs` | Embedded static file server (web viewer) | ✅ WORKING |

### Python side (`backup/github_repos/SonarSniffer/src/sonarsniffer/`):
| File | Purpose | Status |
|------|---------|--------|
| `sonar_parser.py` | Python sonar parser (SON/SL2/SL3) | ✅ WORKING |
| `pipeline.py` | Full Python pipeline: parse → detect → export | ✅ WORKING |
| `advanced_target_detection.py` | Advanced ML target classification | ✅ WORKING |
| `geospatial_export.py` | GeoTIFF + KML export from sonar data | ✅ WORKING |
| `gstreamer_bridge.py` | GStreamer video waterfall export | ✅ WORKING |
| `web_dashboard_generator.py` | Static HTML viewer generation | ✅ WORKING |
| `mbtiles_kml_system.py` | MBTiles + KML combined export | ✅ WORKING |
| `ml_pipeline.py` | ML target scoring | ✅ WORKING |
| `telemetry.py` | GPS telemetry processing | ✅ WORKING |
| `incremental_loading.py` | Stream-load large sonar files | ✅ WORKING |
| `engine_working.py` | Main engine (production) | ✅ WORKING |
| `engine_nextgen_syncfirst.py` | Nextgen engine (sync-first architecture) | ✅ WORKING |

### SonarSniffer curvelet crate (`backup/github_repos/SonarSniffer-by-NautiDog/curvelet-crate/`):
Duplicate of `nauticuvs`. Same API: `curvelet_forward`, `curvelet_inverse`, `CurveletCoeffs`.
**Action**: keep `nauticuvs` as canonical, delete this duplicate.

### Integration gap:
SonarSniffer's parsers are a **completely isolated app**. They need to be wired into CESAROPS by:
1. Adding a `cesarops-sonar` crate (or moving `backup/sonarsniffer/SonarSniffer/src-tauri/src/` to a new crate) 
2. Exposing a `/sonar/parse` endpoint in `wrecks_api`
3. Building `SonarPanel.tsx` in the Tauri frontend

---

## LOOSE PIPELINE TOOLS (not wired to anything)

These files exist and work in isolation but aren't called by any pipeline orchestrator.

### pipelines/mag/ loose tools (selection of unwired ones):
| File | Purpose | Should feed |
|------|---------|------------|
| `mag_gpu_dipole.py` | GPU-accelerated dipole scanning (CUDA) | `aeromagnetic-worker` |
| `adaptive_background_scan.py` | Adaptive background subtraction | `magnetic_eraser.rs` |
| `nauticuvs_mag_curvelet.py` | Python curvelet on mag grids | `aeromagnetic-worker` curvelet.rs |
| `erie_synthetic_dipole.py` | Synthetic dipole injection for testing | test harness |
| `multisource_depth_proximity_ranker.py` | Rank candidates by depth proximity | post-detection ranking |
| `shipping_lane_analysis.py` | Filter candidates inside shipping lanes | pre-filter |
| `wh2k_upward_continuation.py` | Upward continuation transform | preprocessing |
| `wh2k_wreck_anchor_verifier.py` | Verify anchor sites in AWOIS | validation |
| `mag_data_manager.py` | Manage mag data sources | `mag_data_pipeline.py` |
| `wh2k_ncei_fetch.py` | Fetch NCEI magnetic surveys | data ingestion |
| `huron_mag_water_scan.py` | Lake Huron scan | scan orchestrator |
| `run_all_source_scan.py` | Run scan across all data sources | top-level trigger |
| `confidence_reporting.py` | Confidence score reporting | post-detection |

### pipelines/satellite/ loose tools:
| File | Purpose | Should feed |
|------|---------|------------|
| `temporal_stack_engine.py` | 20-day tile stacking (no wiring) | `satellite_stitch.rs` |
| `sat_mission_orchestrator.py` | Mission-driven satellite tasking | `cesarops-satellite-worker` |
| `wh2k_sentinel_wreck_targeting.py` | Sentinel-2 wreck targeting | main scan pipeline |
| `wh2k_ab_attenuation.py` | Attenuation correction | `optical_mass.rs` |
| `sar_temporal_persistence.py` | SAR target persistence across dates | main detection |
| `historical_drift.py` | Historical drift model | `satellite_stitch.rs` |
| `apply_opera_dswx.py` | Apply DSWx water mask | preprocessing |
| `nasa_fusion_test.py` | Fusion of NASA data sources | test/validation |

### pipelines/bag/ loose tools (most not wired to Rust):
| File | Purpose | Should feed |
|------|---------|------------|
| `bag_alignment_corrector.py` | Correct BAG coordinate offset | `bag_scanner.rs` |
| `bag_visualization_generator.py` | Generate HTML visualization | frontend BAG viewer |
| `comprehensive_bag_gui.py` | Tkinter GUI for BAG scanning | deprecated, use Tauri |
| `comprehensive_pdf_bag_scan.py` | PDF + BAG combined scan | pipeline stage |
| `black_hole_scanner.py` | Detect black holes (sensor dropout craters) | `bag_scanner.rs` |
| `cross_section.py` | Cross-section profile of detected wrecks | post-detection |
| `convert_and_match.py` | Convert BAG to GeoTIFF + match wrecks DB | wiring to DB |
| `add_meshes.py` | Add 3D mesh from BAG to wreck record | 3D viz |
| `bag_metadata_analyzer.py` | Extract and validate BAG metadata | preprocessing |
| `azure_vision_analyzer.py` | Azure Vision AI on BAG imagery | scout layer |
| `attention_processor.py` | Attention mechanism on sonar tiles | ML pipeline |

### pipelines/wreckhunter/ loose tools:
| File | Purpose | Should feed |
|------|---------|------------|
| `db_ingestor.py` | Ingest scan results into wrecks.db | `wrecks_api` |
| `populate_database.py` | Populate DB from NOAA/AWOIS sources | `wrecks_api` |
| `smart_daily_scan.py` | Weather-aware daily scan scheduler | `weather_service.py` |
| `sync_xenon_db.py` | Sync database to Xenon node | fleet sync |
| `full_lake_michigan_run.py` | Full Lake Michigan pipeline run | top-level trigger |
| `prioritized_satellite_pull.py` | Priority-order satellite data pull | `universal_downloader.py` |
| `straits_fox_runner.py` | Straits of Mackinac specific scan | scan orchestrator |
| `iowa_202_analysis.py` | Iowa 202 wreck site analysis | site-specific |
| `small_batch_anomaly_test.py` | Small batch anomaly validation | testing |

### pipelines/analysis/ loose tools:
| File | Purpose |
|------|---------|
| `monster_candidate.py` | Analysis of largest anomaly candidates |
| `monster_site_analysis.py` | Deep analysis of top-ranked sites |

---

## LAPTOP DUMPS — `backup/deploy/`

These are the most important source of missing logic.

### `backup/deploy/tools/cesarops_core/` — SAROPS drift engine (not integrated):
| File | Purpose | Integration gap |
|------|---------|----------------|
| `sarops.py` | Full SAROPS (Search and Rescue Optimal Planning System) drift model | Not wired to `weather_service.py` or scan scheduler |
| `fast_drift_engine.py` | Fast probabilistic drift calculation | Not called by anything in live codebase |
| `ml_drift_predictor.py` | ML drift prediction (trained on GLOS buoys) | Not connected |
| `drifter_training_pipeline.py` | Training pipeline for drift ML model | Not run post-training |
| `enhanced_drift_analysis.py` | Enhanced uncertainty-aware drift | Not wired |
| `garmin_rsd_integration.py` | Garmin GPS integration for drift tracking | Not in main pipeline |
| `drone_module.py` | UAV/drone SAR coordination | Not wired |
| `drone_reporting_integration.py` | Drone mission reporting | Not wired |
| `reporting_api.py` | SAR operation reporting API | Duplicate of wrecks_api |
| `collaborative_sarops_platform.py` | Multi-agency coordination | Standalone |
| `cesarops_integrated.py` | Full integrated CESAROPS system | Standalone (most complete version?) |
| `garmin_rsd_integration.py` | Garmin GPS device integration | Not wired to SonarSniffer |

### `backup/deploy/tools/cesarops-core-github/` — GitHub repo snapshot:
| File | Purpose | Gap |
|------|---------|-----|
| `cesarops_engine.py` | Core search engine | May be more current than live |
| `ai_director.py` | AI-driven search director | Not in live codebase root |
| `database_connector.py` | DB connection pooling | Not used by wrecks_api |
| `db_ingestor.py` | DB ingestor | Duplicate — which version is current? |
| `remote_dispatch.py` | Remote job dispatch | Not wired to fleet |
| `init_database.py` | Database initialization | Run once? Or needs re-run? |
| `config_agent.py` | Config management agent | Not in live codebase |
| `tauri/` | **The full Tauri desktop frontend** | ← NEEDS TO MOVE TO LIVE WORKSPACE |

### `backup/deploy/agent/` — Agent code:
| File | Purpose | Gap |
|------|---------|-----|
| `ai_director.py` | AI search director | Duplicate — reconcile with live |
| `cesarops_orchestrator.py` | Mission orchestrator | Lives in backup only |
| `cesarops_sar_orchestrator.py` | SAR-specific orchestrator | Lives in backup only |
| `smart_search_planner.py` | Smart search planner | Duplicate |
| `thought-engine/thought_engine.py` | LLM thought chain engine | Not wired to `wrecks_api` agent endpoints |
| `llm_context_injector.py` | Injects geospatial context into LLM prompts | Not wired |
| `research_agent_laptop.py` | Research agent | Lives in backup only |

### `backup/deploy/detection/` — Detection code:
| File | Purpose | Gap |
|------|---------|-----|
| `scan_engine.py` | Version of scan engine | Compare with live — may have newer code |
| `scan_worker.py` | Scan worker | Compare with live |
| `background_probe.py` | Background probing | Compare with live |
| `crossref_scans.py` | Cross-reference scans | Compare with live |
| `triple_lock_fusion.py` | Triple-lock fusion | Compare with live |
| `lake_michigan_scan.py` | Michigan scan | Compare with live |
| `lake_erie_scan.py` | Erie scan | Compare with live |

---

## SUMMARY: WHAT TO DO

### Tier 1 — Wire in (code exists, just not connected):
1. **Add `pdf_breaker` to `STAGE_REGISTRY`** in `wrecks_api/pipeline_stages.py` (5 min)
2. **Create `wrecks_api/stages/bag_scanner_stage.py`** — calls the Rust `cesarops-detection bag_scanner` or Python `bag_wreck_detector.py` 
3. **Move Tauri frontend** from `backup/deploy/tools/cesarops-core-github/tauri/` → `cesarops-tauri/`
4. **Add `satellite_stage.py`** — wraps `temporal_stack_engine.py`

### Tier 2 — Port SonarSniffer into main pipeline:
5. Create `cesarops-sonar` crate in workspace — copy the 6 parser files + `target_detection.rs` from backup
6. Add `/sonar/parse` endpoint to `wrecks_api`
7. Build `SonarPanel.tsx` in the Tauri frontend

### Tier 3 — Integrate SAROPS drift engine:
8. Wire `sarops.py` + `fast_drift_engine.py` into `weather_service.py`
9. Connect drift prediction to scan scheduling

### Tier 4 — Reconcile duplicates in backup vs live:
10. Compare `backup/deploy/detection/scan_engine.py` vs live `scan_engine.py` — merge newer logic
11. Determine which `ai_director.py` / `cesarops_orchestrator.py` is canonical
12. Verify `init_database.py` has been run against `db/wrecks.db`
