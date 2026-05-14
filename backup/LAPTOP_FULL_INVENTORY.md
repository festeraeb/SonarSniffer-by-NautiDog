# Laptop Backup Full Inventory
## Generated: 2026-05-13
## Total files scanned: 607 (407 .py + 200 .rs)

---

### WORKING TOOLS (recover these)

| File | Purpose | Category | Notes |
|------|---------|----------|-------|
| cesarops_core/enhanced_drift_analysis.py | ML-enhanced drift analysis with Rust core integration | drift | Full implementation, imports numpy/pandas/sklearn, Rust FFI bridge |
| cesarops_core/fast_drift_engine.py | High-performance drift engine using Numba JIT | drift | Production-ready, multi-process, near-Rust performance |
| cesarops_core/ml_drift_predictor.py | ML drift prediction using RandomForest | drift/ml | Full sklearn pipeline, model training + inference |
| cesarops_core/cesarops.py | Core OpenDrift integration for Great Lakes | drift | ERDDAP URLs for all 5 lakes, real hydrodynamic data |
| cesarops_core/cesarops_integrated.py | Unified SAR platform with role-based access | agent | Full Tkinter GUI, ICS-compliant, multi-agency |
| cesarops_core/sarops.py | Main CESAROPS SAR application launcher | agent | Tkinter GUI, logging, directory setup |
| cesarops_core/drone_module.py | Drone coordination and mission planning | drone | Full implementation: search patterns, LSAR, telemetry |
| cesarops_core/drone_reporting_integration.py | Drone-to-reporting bridge | drone | Integration layer, depends on drone_module |
| cesarops_core/drifter_training_pipeline.py | Multi-source drifter data collection + ML training | drift/ml | NOAA GDP, NDBC, GLOS Seagull integration |
| cesarops_core/collaborative_sarops_platform.py | Collaborative SAR with multi-agency features | agent | Tkinter, SQLite, email, UUID-based cases |
| cesarops_core/fetch_historical_weather.py | NDBC/ERDDAP historical weather fetcher | satellite | Real API calls for Rosa case dates |
| cesarops_core/garmin_rsd_integration.py | Garmin RSD Studio sonar integration | sonar | Multi-format parsing, AI detection, 3D mapping |
| cesarops_core/glos_seagull_analysis.py | GLOS Seagull ERDDAP data analysis | drift | Tests hand calculations with real buoy data |
| cesarops_core/real_drifter_collector.py | Real drifter data from NOAA/GLOS/NDBC | drift | Multi-source ERDDAP collection |
| cesarops_core/reporting_api.py | Unified reporting API | tools | Combines incident reports + metrics + export |
| cesarops_core/simple_drifter_collector.py | Simplified drifter data collector | drift | Lightweight version for quick collection |
| cesarops_core/simple_fast_engine.py | Simplified fast drift engine (no JIT) | drift | Immediate deployment, no numba dependency |
| cesarops_core/foundry_agent/app.py | Flask agent endpoint for LLM chat | agent | Health check + chat proxy to model endpoint |
| cesarops_core/src/cesarops/bathymetry_mapper.py | High-res bathymetric mapping from sonar | sonar | Grid interpolation, contour, multi-format export |
| cesarops_core/src/cesarops/contextual_anomaly_detection.py | Advanced anomaly detection with explainability | detection | Pattern recognition, risk levels, ML learning |
| cesarops_core/src/cesarops/sensor_fusion.py | Multi-sensor fusion engine | detection | Sonar + bathymetry + currents + temperature |
| cesarops_core/src/cesarops/sonar_anomaly_detector.py | ML-based sonar anomaly detection | sonar | Isolation Forest, StandardScaler, real logic |
| cesarops_core/src/cesarops/sonar_classification.py | Real-time sonar signal classification | sonar | ML classification: fish/geological/marine/artifact |
| cesarops_core/src/cesarops/sonar_mosaic.py | Seamless sonar mosaic generator | sonar | Multi-layer rendering, GeoTIFF/KML export |
| cesarops_core/src/cesarops/sonar_stream_buffer.py | High-performance circular buffer for sonar | sonar | Thread-safe, performance monitoring |
| cesarops_core/src/cesarops/sonar_visualization.py | WebGL-based 3D sonar visualization | sonar | Blend modes, layer composition |
| cesarops_core/src/cesarops/interactive_map.py | Interactive map controls for sonar viz | sonar | Layer toggling, camera, statistics |
| cesarops_core/reports/incident_report.py | 10-section SAR incident report generator | tools | Professional documentation, JSON/HTML export |
| cesarops_core/reports/pdf_export.py | PDF/HTML export for reports | tools | CSS-styled professional output |
| cesarops_core/analytics/sar_metrics.py | 10 SAR-specific performance metrics | tools | Response, efficiency, resource, outcome metrics |
| cesarops_core/rosa_case/rosa_calibrated_model.py | Rosa case calibrated drift model | drift | Ground truth validation, 0.619km accuracy |
| cesarops_core/rosa_case/rosa_case_analysis.py | Comprehensive Rosa hindcast/forecast | drift | ML validation against known outcome |
| cesarops_core/rosa_case/rosa_fender_hindcast.py | Rosa fender hindcast with enhanced drift | drift | Real SAR case, matplotlib visualization |
| cesarops_core/rosa_case/rosa_forward_seeding.py | Forward seeding pattern analysis | drift | Multiple release points, geopy distance |
| cesarops_core/rosa_case/rosa_hindcast_final.py | Complete Rosa hindcast using fast engine | drift | Final validated analysis |
| cesarops_core/rosa_case/rosa_multimodal_demo.py | Multi-modal FCNN system demo | drift/ml | Sentence Transformers, attention prediction |
| cesarops_core/rosa_case/rosa_nautical_visualization.py | MBTiles + ENC chart visualization | tools | Nautical context, depth soundings |
| cesarops_core/rosa_case/rosa_real_analysis.py | Rosa analysis with real environmental data | drift | Database-backed environmental conditions |
| ai_director.py | Qwen-powered tool picker + parameter tuner | agent | Natural language → tool selection → execution |
| cesarops_agent_runner.py | Agent execution + parameter tuning runner | agent | Multi-sensor, GeoJSON output, DB push |
| cesarops_engine.py | Local GPU + remote TPU processing engine | tools | M2200 + Xenon TPU pipeline |
| cesarops_mission.py | Unified multi-pass scan engine | tools | JSON mission config, all passes toggleable |
| cesarops_orchestrator.py | Full sensor probe orchestrator | tools | Local/Remote/Hybrid modes, LLM integration |
| cesarops_sar_orchestrator.py | SAR slick orchestrator (Rust bridge) | detection | Calls cesarops-slicer Rust binary |
| cesar_sidecar_api.py | FastAPI microservice for wreck hunting | tools | Role-based container specialization |
| cesar_tools_core.py | Unified engine for all scanning pipelines | tools | ML model loading, TPU client |
| cmr_search.py | NASA CMR granule search | satellite | Called by Tauri/Rust, JSON output |
| database_connector.py | SQLite connector for census DB | tools | LAKE_MICHIGAN_CENSUS_2026.db integration |
| db_ingestor.py | JSON → SQLite ingestor for probe results | tools | File watcher, Tauri live updates |
| universal_downloader.py | Multi-source satellite data downloader | satellite | ASF, Copernicus, PO.DAAC, USGS, HLS |
| scan_engine.py | Unified parameterized scan engine | detection | All lakes, all dates, all passes |
| scan_queue.py | SQLite-backed priority job queue | tools | Priority levels, CLI interface |
| scan_worker.py | Continuous scan worker daemon | tools | Priority interrupt, coverage learning |
| scan_cli.py | Manual scan CLI interface | tools | Lake/bbox/dates/passes selection |
| mission_control.py | Pipeline knob-turner interface | tools | Weather → date selection → download → scan |
| missions.py | Mission registry (single source of truth) | config | All scan missions defined here |
| run_mission.py | Generic mission front-end | tools | Local or queue dispatch |
| node_worker.py | Autonomous LAN-native job runner | tools | Multi-node, capability-based |
| queue_worker.py | Queue worker polling scan_queue.db | tools | SSH dispatch to i7, concurrent |
| remote_dispatch.py | Multi-node SSH task dispatcher | tools | Pi/i7/Laptop/Xeon roles |
| gpu_pool.py | GPU fleet registry + client | tools | Tailscale mesh discovery, idle routing |
| gpu_server.py | Flask GPU inference server | tools | CuPy z-score, health reporting |
| tpu_client.py | Remote Coral TPU client | tools | Cloudflare tunnel, retry logic |
| tpu_server.py | Flask TPU inference server | tools | Coral Edge TPU gateway |
| weather_service.py | Open-Meteo weather API integration | satellite | Historical + forecast, no API key |
| tile_geometry.py | Illumination/view geometry calculator | satellite | .geometry.json sidecar writer |
| tile_selector.py | Tile ranking by scan mode | satellite | HISTORIC_WRECK, BATHY_3D, SAR modes |
| smart_search_planner.py | Intelligent search planner | agent | Auto-plan: data search → tool selection |
| background_probe.py | Continuous known-wreck-site probing | detection | ML parameter tuning from results |
| triple_lock_fusion.py | Multi-sensor anomaly verification | detection | Thermal + SAR + Optical agreement |
| crossref_scans.py | Multi-year anomaly cross-reference | detection | Persistent anomaly = HIGH CONFIDENCE |
| wreck_ml_trainer.py | Multi-sensor patch feature extraction + TFLite | ml | 40-feature vector, LightGBM → ONNX → TFLite |
| wreck_scraper.py | Wreck database builder from registries | tools | NOAA, GLSHS, Michigan, Wisconsin sources |
| wreck_web_scraper.py | Web scraper for wreck GPS | tools | Thunder Bay NOAA, Wikipedia preserves |
| lake_michigan_scan.py | Full Lake Michigan hybrid scan | detection | 6 sensor bands, hydrocarbon mode |
| lake_erie_scan.py | Lake Erie multi-sensor scan | detection | Hydrocarbon timeline + M&B No.2 search |
| watchdog.py | Service watchdog for API + KoboldCPP | tools | Auto-restart, health checks |
| init_database.py | SQLite database initialization | tools | Comprehensive schema for scans/detections |
| generate_report.py | Mission report generator | tools | Prioritized analysis from crossref |
| kobold_dashboard.py | Flask web dashboard for KoboldCPP | tools | Multi-machine management UI |
| deploy_kobold_multi.py | Multi-machine KoboldCPP deployment | tools | T440, Xeon, p1000, gtx1060 |
| research_agent.py | Overnight autonomous research agent | agent | Crossref, Semantic Scholar, arXiv |
| thought-engine/thought_engine.py | Distributed reasoning (8B plan + 35B execute) | agent | FastAPI, nautivecs context, DuckDuckGo |
| wrecks_api/app.py | FastAPI REST API for wrecks DB | tools | Search, filter, steel-freighters endpoint |
| wrecks_api/pipeline_stages.py | Modular post-scan pipeline orchestrator | tools | Stage registry pattern |
| wrecks_api/stages/mag_pipeline_stage.py | Magnetic anomaly pipeline stage | detection | Orchestrates mag processing |
| wrecks_api/stages/pdf_breaker_stage.py | PDF breaker pipeline stage | tools | BAG/mag PDF processing |
| news_search/chronicling_america_search.py | Library of Congress newspaper search | tools | Great Lakes shipwreck articles |
| vision-workers/scout_1060.py | Florence-2 vision scout (GTX 1060) | detection | Tile anomaly classification |
| vision-workers/validator_p1000.py | Moondream2 cross-validator (P1000) | detection | Independent confirmation |
| b02_download.py | HLS B02 band downloader | satellite | Earthdata token auth |
| check_landsat.py | Planetary Computer Landsat query | satellite | Quick availability check |
| hardware_telemetry.py | System hardware monitoring | tools | psutil-based telemetry |
| llm_context_injector.py | LLM context injection from satellite sources | agent | System prompt enrichment |
| cesar_agent_llm.py | LLM specialist agent wrapper | agent | KoboldCPP/local LLM reasoning |
| drive_discovery.py | ArmorATD drive finder (local + network) | tools | UUID + ARP scan + Samba/SSHFS |
| drive_identity.py | Portable drive identity system | tools | Authentication via webpage |
| cuda_env.py | CUDA environment configurator | config | Multi-platform path detection |
| swot_ssh_extractor.py | SWOT SSH anomaly extractor | satellite | PO.DAAC download + extraction |
| nwa2501_scan.py | NWA Flight 2501 wreck hunt | detection | DC-4 specific detection parameters |
| hormuz_mine_scan.py | Strait of Hormuz mine detection | detection | Maritime safety, SAR + optical |
| alaska_canopy_scan.py | Alaska sub-canopy SAR detection | detection | Boreal forest penetration test |
| august_2015_leak_scout.py | Pre-peak hydrocarbon detection | detection | Multi-sensor fusion for oil |
| land_site_test.py | Aircraft crash site detection | detection | 3-node pipeline, LiDAR enrichment |
| i7_cpu_passes.py | i7 CPU-only specialized scan | detection | Pass 2/3/4 without GPU |
| train_containers_from_known.py | ML trainer using known wrecks | ml | Triple Lock Rule integration |
| wreck_pixel_probe.py | Pixel value probe at wreck coords | detection | Raw DN, z-score per site |
| match_wrecks.py | Crossref → known wrecks matcher | detection | Haversine distance matching |
| audit_wrecks_db.py | Wrecks DB coordinate quality audit | tools | Genuine vs centroid GPS |
| query_wrecks_db.py | Wrecks DB query tool | tools | Dump verified GPS entries |
| fix_and_add_wrecks.py | Fix JSON + add Straits wrecks | tools | Cross-reference with anomalies |
| scrape_erie_wrecks.py | Erie wrecks web scraper | tools | eriewrecks.com parsing |
| andaste_geometry_test.py | SS Andaste geometry verification | detection | Straight-back sieve validation |
| bridge_calibrate.py | Mackinac Bridge coordinate calibration | tools | CRS/EPSG diagnostic |
| bridge_proximity.py | Known wrecks vs bridge proximity | tools | Haversine distance table |
| analyze_crossref.py | Crossref results vs known wrecks | detection | Stumpf depth integration |

### WORKING TOOLS — Rust Crates (recover these)

| File | Purpose | Category | Notes |
|------|---------|----------|-------|
| nauticuvs/src/lib.rs + all src/ | Precision curvelet engine for wreck detection | detection | Core math library, FDCT, FFT, geo, weights |
| nauticuvs/src/curvelet/*.rs | Curvelet transform forward/inverse | detection | CoefficientStore, phase extraction |
| nauticuvs/src/fdct_kernels/*.rs | FDCT inner-loop kernels (XLA-ready) | detection | Tile, window, wrap operations |
| nauticuvs/src/fft_backend.rs | FFT backend (rustfft/fftw) | detection | Compile-time backend selection |
| nauticuvs/src/geo/*.rs | GeoTIFF ingestion + coordinate transform | detection | Pure-Rust TIFF parsing |
| nauticuvs/src/weights/*.rs | Curvelet reconstruction weights | detection | Directional mask, Richardson |
| nauticuvs/src/detection.rs | Public anomaly detection interface | detection | Opaque DetectionConfig |
| nauticuvs/src/precision.rs | f64 precision selection | detection | Sub-pixel alignment accuracy |
| nautivecs/src/*.rs | AST-aware code vectorization + retrieval | agent | tree-sitter chunking, LanceDB, embeddings |
| nautivecs/src/serve.rs | HTTP API for nautivecs search | agent | OpenAI-compatible endpoint |
| nautivecs/src/graph.rs | Symbol graph relational indexing | agent | Prevents LLM hallucination |
| cesarops-detection/src/*.rs | Triple-Lock detection pipeline | detection | Scout→Validator→Jitter→Confirmed |
| cesarops-detection/src/dispatcher.rs | n8n-style task dispatcher | detection | Scan job tracking + progress |
| cesarops-detection/src/workers.rs | HTTP clients for vision workers | detection | 1060/P1000/TPU clients |
| cesarops-slicer/src/*.rs | Unified GeoTIFF tile slicer | satellite | Zero-copy mmap, coordinate baking |
| cesarops-slicer/src/io/*.rs | GeoTIFF I/O + VRT + GDAL warp | satellite | Memory-mapped, GPU warp |
| cesarops-slicer/src/tiles/*.rs | Tile slicing + anchor coords | satellite | Sidecar JSON with GPS |
| cesarops-slicer/src/spec/*.rs | Mission spec parsing (Qwen JSON) | satellite | Hardware delegate routing |
| cesarops-slicer/src/sar_slick_worker.rs | SAR slick detection worker | detection | Rayon parallel, LLM integration |
| cesarops-slicer/src/bathymetry_specialist.rs | Bathymetry specialist worker | detection | Rayon parallel processing |
| cesarops-slicer/src/optical_structural_worker.rs | Optical structural detection | detection | wgpu + reqwest + rayon |
| cesarops-slicer/src/thermal_specialist.rs | Thermal anomaly specialist | detection | wgpu compute shaders |
| cesarops-slicer/src/reef_anomaly_filter.rs | Reef anomaly filtering | detection | Rayon + reqwest |
| cesarops-slicer/src/research_ingestion_specialist.rs | arXiv/Semantic Scholar scraper | agent | Sled DB, KoboldCpp synthesis |
| cesarops-slicer/src/llm_worker.rs | LLM review worker for anomalies | agent | Sled queue + llm crate |
| cesarops-slicer/src/model_team_tool.rs | Model team orchestrator | agent | Reasoning + coding split |
| cesarops-inference/src/*.rs | Native Rust LLM inference engine | ml | GGUF loader, transformer, sampling |
| cesarops-inference/src/server.rs | KoboldCPP-compatible HTTP server | ml | Drop-in replacement API |
| cesarops-inference/src/tokenizer.rs | Real BPE tokenizer (HuggingFace) | ml | Qwen special tokens |
| cesarops-inference/src/transformer.rs | Transformer forward pass + KV cache | ml | CPU implementation |
| cesarops-inference/src/mcp.rs | MCP tool definitions | ml | Hardware audit, model info |
| cesarops-inference/src/arena.rs | NUMA-pinned memory arena | ml | Zero-allocation inference |
| cesarops-inference/src/cake_kv.rs | Multi-tiered KV memory pager | ml | VRAM→DDR4→RAID mmap |
| cesarops-inference/src/geo_filter.rs | Geological subtraction filter | detection | FFT bandpass, dipole detect |
| cesarops-inference/src/grammar.rs | Grammar-constrained sampling | ml | Maritime-specific constraints |
| cesarops-inference/src/loader.rs | GGUF model loader (memmap) | ml | Zero-copy, MoE sharding |
| cesarops-inference/src/hardware.rs | Hardware archaeologist | ml | GPU/CPU/NUMA detection |
| cesarops-mcp-steered/src/*.rs | MCP server with nautivecs injection | agent | Anti-drift grounding |
| cesarops-mcp-steered/src/scm/*.rs | Segmented Context Manager | agent | RSU decomposition, drift monitoring |
| cesarops-mcp-steered/src/research_engine.rs | Sovereign research + synthesis loop | agent | Librarian pattern, parallel queries |
| cesarops-mcp-steered/src/verification.rs | Self-RAG + speculative verification | agent | 1.5B→7B→Human pipeline |
| cesarops-forge/src/*.rs | Autonomous developer agent v1 | agent | LLM + search + tools + loop |
| cesarops-forge-v2/src/*.rs | Autonomous developer agent v2 | agent | Diagnostics, hardware, memory, prompts |
| cesarops-forge-web/src/main.rs | Web interface for forge agent | agent | Axum + regex + Mutex |
| cesarops-satellite-worker/src/*.rs | Coral TPU satellite pass pipeline | satellite | Temporal persistence matching |
| cesarops-satellite-worker/src/detection_store.rs | SQLite detection persistence | satellite | Temporal cluster matching |
| cesarops-aeromagnetic-worker/src/*.rs | Aeromagnetic anomaly worker | detection | wgpu + nauticuvs curvelet |
| cesarops-aeromagnetic-worker/src/discriminator.rs | Wellhead/wreck discriminator | detection | Cross-reference candidates |
| cesarops-db-core/src/*.rs | PostgreSQL database core | tools | sqlx, PostGIS, WKB→GeoJSON |
| cesarops-hybrid-engine/src/*.rs | Dual P100 hybrid LLM + spatial engine | ml | MoE routing, wgpu shaders |
| cesarops-hybrid-engine/src/context_engine.rs | Dynamic knowledge compilation | agent | Task-specific context injection |
| cesarops-hybrid-engine/src/fdct_kernels.rs | FDCT CPU (AVX-512) + GPU (wgpu) | detection | Dual backend dispatch |
| cesarops-hybrid-engine/src/geotransform.rs | WGS84→ECEF→ENU coordinate transform | detection | AVX-512 optimized |
| cesarops-hybrid-engine/src/spatial_engine.rs | Nauticus sub-surface scanner | detection | wgpu compute, TCP streaming |
| cesarops-hybrid-engine/src/tool_db.rs | Dynamic tool routing database | agent | Semantic anchor search |
| sovereign-cloud/src/*.rs | Sovereign cloud orchestrator | tools | mDNS discovery, idle scout, TPU |
| sovereign-cloud/src/research_engine.rs | Autonomous research loop | agent | Anti-hallucination design |
| sovereign-cloud/src/self_training.rs | Self-training from scan results | ml | rusqlite persistence |
| sentinel_hunt_src/src/*.rs | Sentinel-based wreck detection | satellite | STAC, weather, GLOS, orbits |
| sentinel_hunt_src/src/detect.rs | Detection engine | satellite | Python bridge for processing |
| sentinel_hunt_src/src/validation.rs | Detection validation against DB | satellite | rusqlite cross-reference |
| tauri/src-tauri/src/*.rs | Tauri desktop app backend | tools | KML generator, NASA agent, Erie model |
| warp-grid/src/*.rs | Hardware-agnostic tensor buffer management | ml | NUMA, metrics, pool, translator |
| model-team-tool/src/main.rs | Split-agent coder orchestrator | agent | Hardware-aware plan→code→review |
| cesarops-wso-server/src/main.rs | WSO search engine server | tools | Axum + WsoEngine |
| cesarops-xbox-worker/src/main.rs | Xbox worker (ndarray + half) | detection | Array processing worker |


### WORKING TOOLS — Pipelines (recover these)

| File | Purpose | Category | Notes |
|------|---------|----------|-------|
| pipelines/mag/adaptive_background_scan.py | Adaptive background mag scan for subtle anomalies | detection | Moving window, edge behavior |
| pipelines/mag/datum_correction.py | NAD27→WGS84 + Loran-C rubber-sheeting | detection | Triangulated correction |
| pipelines/mag/dipole_analysis.py | Deep dipole analysis on candidates | detection | Polarity flip, gradient sharpness |
| pipelines/mag/erie_scanner_pipeline.py | Lake Erie focused training + detection | detection | OGSr wells, Loran-C warp |
| pipelines/mag/erie_synthetic_dipole.py | Synthetic magnetic dipole generator | ml | Physics-based training data |
| pipelines/mag/erie_wellhead_discriminator.py | Well-head false positive filter | detection | OGSr + NDA cross-reference |
| pipelines/mag/erie_feedback_loop.py | Continuous model improvement loop | ml | Auto-label + XGBoost warm-start |
| pipelines/mag/erie_known_wrecks_db.py | Comprehensive Erie wreck database | tools | NDA + Wikipedia + ShipwreckWorld |
| pipelines/mag/flight_line_physics.py | Flight-line 1D profile validation | detection | Raw along-track measurements |
| pipelines/mag/flight_line_physics_v2.py | Flight-line physics v2 (fixed) | detection | Actual well positions, ±2km |
| pipelines/mag/geo_filter_candidates.py | Geo-filter + deep scoring pipeline | detection | Lake/shore/land classify, dipole |
| pipelines/mag/loran_c_warp.py | Loran-C navigation warp correction | detection | Affine warp from control points |
| pipelines/mag/mag_data_pipeline.py | Real magnetic anomaly pipeline | detection | Download, grid, detect, cross-ref |
| pipelines/mag/mag_data_manager.py | Magnetic data lake manager | tools | Persistent storage + S3 backup |
| pipelines/mag/mag_lake_harvester.py | Tiered magnetic data acquisition | satellite | Catalog, gap-fill, normalize |
| pipelines/mag/ogsrl_well_discriminator.py | Ontario well-based discriminator | detection | Public CSV download + cross-ref |
| pipelines/mag/multisource_depth_proximity_ranker.py | Multi-source target ranker | detection | Depth/proximity detectability |
| pipelines/mag/run_all_source_scan.py | All-source adaptive mag scan | detection | Multi-source merge + dedup |
| pipelines/mag/run_lake_scans.py | Full mag pipeline for Huron/Erie | detection | Argparse, logging |
| pipelines/mag/generate_kml.py | Multi-layer KMZ generator | tools | Color-coded by score/type |
| pipelines/mag/generate_combined_kml.py | Combined Erie+Huron KMZ | tools | Multi-lake visualization |
| pipelines/mag/export_huron_mag_kml.py | Huron mag anomalies to KML | tools | Dipole analysis export |
| pipelines/mag/wh2k_harvester.py | WH2K data harvester engine | satellite | NGDC, USGS, NRCan sources |
| pipelines/mag/wh2k_ncei_fetch.py | Raw flight-line data fetcher | satellite | MGD77T/MAG88T pings |
| pipelines/mag/wh2k_rasterize_erie_csv.py | Erie CSV → GeoTIFF rasterizer | satellite | 531K raw points gridded |
| pipelines/mag/wh2k_rasterize_huron_csv.py | Huron CSV → GeoTIFF rasterizer | satellite | 1M+ raw points gridded |
| pipelines/mag/wh2k_extract_real_tiles.py | Real aeromagnetic tile extractor | ml | 224×224 labeled patches |
| pipelines/mag/wh2k_ingest_gsc_erie.py | GSC Erie survey CSV ingestor | satellite | High-res GeoTIFF output |
| pipelines/mag/wh2k_awois_scraper.py | AWOIS + 3dshipwrecks scraper | tools | Dive-verified coordinates |
| pipelines/mag/wh2k_data_provenance.py | Data provenance validator | tools | Cross-survey consistency |
| pipelines/mag/wh2k_discovery_report_standalone.py | Standalone discovery report | detection | ResNet-18 scan + AWOIS match |
| pipelines/mag/wh2k_discovery_report_v2.py | Discovery report v2 (bicubic) | detection | FVD + off-axis + GeoJSON |
| pipelines/mag/wh2k_export_kml.py | WH2K KML/KMZ export | tools | Score-coded, nearest matches |
| pipelines/mag/wh2k_satellite_mag_validate.py | Per-basin model validation | ml | Known wreck inference test |
| pipelines/mag/wh2k_upward_continuation.py | Upward continuation + Swarm proof | detection | 400km altitude simulation |
| pipelines/mag/wh2k_warp_field_export.py | Loran-C warp field JSON export | tools | Dense regular grid |
| pipelines/mag/wh2k_wreck_anchor_verifier.py | Wreck-derived anchor verifier | detection | Dive-GPS as control points |
| pipelines/mag/wh2k_digest_new_mag_drop.py | New mag data digest tool | satellite | ZIP extraction + parsing |
| pipelines/mag/normalize_nrcan_csvs.py | NRCan CSV normalizer | satellite | HXYZ format → pipeline format |
| pipelines/satellite/nasa_earthdata_client.py | Earthdata/CMR/AppEEARS wrappers | satellite | Auth, retries, error handling |
| pipelines/satellite/batch_download_manager.py | Swarm-mode download distributor | satellite | Multi-node bandwidth max |
| pipelines/satellite/historical_drift.py | Historical drift analysis engine | drift | Forward/backward/consistency |
| pipelines/satellite/sar_temporal_persistence.py | SAR temporal persistence detector | detection | DBSCAN clustering |
| pipelines/satellite/wh2k_synthetic_tiles.py | Physics-based synthetic tile gen | ml | Steel/Wood/Wellhead archetypes |
| pipelines/satellite/wh2k_synthetic_tiles_v2.py | CRM-physics synthetic tiles | ml | Construction Remanent Magnetization |
| pipelines/satellite/wh2k_synthetic_tiles_huron.py | Huron-specific synthetic tiles | ml | Canadian Shield parameters |
| pipelines/satellite/wh2k_sentinel_optical_poc.py | Sentinel-2 optical wreck-proxy POC | satellite | Shadow/roughness/thermal concepts |
| pipelines/satellite/wh2k_sentinel_wreck_targeting.py | Season-aware wreck signal extractor | satellite | Known wreck coordinate targeting |
| pipelines/satellite/wh2k_sentinel_cpu.py | Sentinel CPU background scanner | detection | Auto-detect model improvement |
| pipelines/satellite/wh2k_chip_extractor.py | Real magnetic chip extractor | ml | 2km×2km chips at known locations |
| pipelines/satellite/wh2k_raw_ghost_zoom.py | Raw ghost zoom diagnostic | detection | MGD77T + FVD + ResNet per ghost |
| pipelines/satellite/wh2k_ab_attenuation.py | A/B spectral attenuation report | detection | Short-wavelength comparison |
| pipelines/satellite/buoy_analog.py | NDBC buoy analog storm extractor | satellite | 1909 M&B No.2 calibration |
| pipelines/bag/bag_wreck_detector.py | BAG file wreck detection system | detection | HDF5 depth anomalies, clustering |
| pipelines/bag/bag_wreck_gui.py | BAG wreck detection GUI | tools | Tkinter, CARIS-style viz |
| pipelines/bag/advanced_bag_scanner.py | Advanced BAG scanner + redaction detection | detection | Signature analysis |
| pipelines/bag/comprehensive_pdf_bag_scan.py | PDF-BAG cross-reference scanner | detection | Coordinate extraction + matching |
| pipelines/bag/bag_visualization_generator.py | BAG reconstruction webpage | tools | Interactive visualization |
| pipelines/bag/comprehensive_bag_gui.py | Full BAG analysis GUI | tools | Tkinter, threading |
| pipelines/bag/black_hole_scanner.py | BAG black hole (redaction) scanner | detection | rasterio + plotly + skimage |
| pipelines/bag/atl23_extract.py | ICESat-2 ATL23 bathymetric parser | satellite | NASA CMR + HDF5 extraction |
| pipelines/bag/azure_vision_analyzer.py | Azure AI Vision for BAG images | detection | Pattern detection in depth grids |

### WORKING TOOLS — ML Training/Inference (recover these)

| File | Purpose | Category | Notes |
|------|---------|----------|-------|
| models/ml/inference/wh2k_inference_scorer.py | Full inference + scoring pipeline | ml | ResNet-18 scan, cross-ref, GeoJSON |
| models/ml/inference/wh2k_run_huron_inference.py | Huron inference with Erie model | ml | Transfer learning validation |
| models/ml/inference/deep_water_detection.py | Deep water detection DNA pipeline | ml | NDCI, thermal lag, SDB deviation |
| models/ml/inference/deep_water_ghost_scan.py | Deep-water ghost scan (Phase 4) | ml | Beaver to Racine corridor |
| models/ml/inference/deep_water_blind_discovery.py | Blind deep-water discovery | ml | Visual prediction layer |
| models/ml/inference/atomic_wreck_sweep.py | Atomic forensic sweep | ml | Wreck DNA library |
| models/ml/inference/sdb.py | Satellite-derived bathymetry helper | ml | Blue/Green ratio log-model |
| models/ml/inference/get_temporal_strategy.py | Forensic temporal strategy | ml | NOAA/NLDAS wind filtering |
| models/ml/training/train_wreck_classifier.py | Random forest wreck classifier | ml | wreck_dna_library features |
| models/ml/training/train_wreck_classifier_gpu.py | PyTorch GPU wreck classifier | ml | DNA library + DataLoader |
| models/ml/training/wh2k_resnet_training.py | ResNet-18 hybrid training pipeline | ml | Anchor & Supplement strategy |
| models/ml/training/wh2k_phase2_gpu.py | Phase 2 GPU fine-tuning | ml | Anti-overfitting, off-axis penalty |
| models/ml/training/wh2k_phase2_gpu_training.py | Phase 2 full feature build | ml | FVD preprocessing, ReduceLR |
| models/ml/training/wh2k_basin_finetune.py | Per-basin fine-tuning | ml | Real + synthetic mix |
| models/ml/training/wh2k_huron_train_from_scratch.py | Huron from-scratch training | ml | Canadian Shield parameters |
| models/ml/training/wh2k_hybrid_training.py | Hybrid training orchestrator | ml | Extract→Proof→Synthetic→Train |
| models/ml/training/ml_feature_engineering.py | Feature extraction (aspect, symmetry) | ml | skimage regionprops |
| models/ml/training/train_classifier.py | Random Forest wreck/well classifier | ml | sklearn + joblib export |


### USEFUL BUT NEEDS WORK (review these)

| File | Purpose | Category | What's Missing |
|------|---------|----------|----------------|
| cesarops_core/cesarops_module_implementation_plan.py | Module implementation strategy | docs | Planning doc as code, not a tool |
| cesarops_core/serve_nautical_interface.py | HTTP server for Rosa viz | tools | Very simple, hardcoded port |
| cesarops_core/rosa_case/rosa_advanced_calibration.py | Advanced calibration | drift | Simpler version of calibrated_model |
| cesarops_core/rosa_case/rosa_direct_optimization.py | Direct parameter optimization | drift | Simpler optimization approach |
| cesarops_core/rosa_case/rosa_enhanced_timeline_analysis.py | Enhanced timeline | drift | Iterative refinement of analysis |
| cesarops_core/rosa_case/rosa_extended_multiday_analysis.py | Multi-day drift | drift | Extended period analysis |
| cesarops_core/rosa_case/rosa_precise_timeline_analysis.py | Precise timeline | drift | Another timeline iteration |
| cesarops_core/rosa_case/rosa_corrected.py | Corrected hindcast | drift | Intermediate version |
| cesarops_core/rosa_case/rosa_calibration_analysis.py | Visualization + calibration | drift | Depends on results JSON |
| cesarops_core/rosa_case/rosa_final_analysis.py | Final analysis summary | drift | Summary/reporting only |
| cesarops_core/rosa_case/rosa_final_summary.py | Results summary | drift | Print-only summary |
| cesarops_core/rosa_case/rosa_summary.py | Quick analysis summary | drift | Minimal quick check |
| cesarops_core/src/cesarops/__init__.py | Package init with imports | config | Import orchestration only |
| config_agent.py | Interactive configuration helper | config | Basic Q&A, no persistence |
| deploy_and_scan.py | Deploy + scan via SSH | tools | Paramiko, needs credentials |
| deploy_kobold_instances.py | KoboldCpp SSH deployment | tools | Single-purpose deploy |
| deploy_p1000.py | Workspace transfer to P1000 | tools | Hardcoded credentials |
| download_erie_multiyear.py | Multi-year Erie download | satellite | Strategy script, calls CMR |
| download_straits_2024.py | Targeted Sep 3 2024 download | satellite | Specific date/tile |
| download_gguf_models.py | GGUF model downloader | tools | huggingface_hub, simple |
| download_test_models.py | Phi-3 ONNX model downloader | tools | snapshot_download |
| great_lakes_bathymetry_sweep.py | Bathymetry data sweep | satellite | CMR download orchestrator |
| hard_pixel_audit.py | Real data pixel audit | detection | Windows paths hardcoded |
| run_job_remote.py | Remote job runner (i7) | tools | Called by queue_worker |
| show_line5.py | Line 5 corridor flag viewer | tools | Read-only diagnostic |
| inspect_scan.py | Scan JSON inspector | tools | Quick diagnostic |
| inspect_geometry_metadata.py | HLS geometry metadata inspector | tools | Read-only diagnostic |
| fetch_geometry_metadata.py | STAC/CMR geometry fetcher | satellite | Diagnostic tool |
| proximity_report.py | Proximity check utility | tools | Minimal haversine helper |
| crossref_scans2.py | Crossref v2 (shipping lanes) | detection | Simpler version |
| pipelines/mag/confidence_reporting.py | Confidence report generator | detection | GeoPandas stub-ish |
| pipelines/mag/crossmatch_optical_mag.py | Optical-mag cross-match | detection | Hardcoded file paths |
| pipelines/mag/directional_pull.py | Directional offset analysis | detection | Hardcoded paths |
| pipelines/mag/erie_direct_match.py | Direct haversine matching | detection | No ML, just distance |
| pipelines/mag/final_magnetic_fusion.py | Magnetic data fusion | detection | GeoPandas, simple |
| pipelines/mag/parse_kml.py | KML parser for mag targets | tools | GeoPandas stub |
| pipelines/mag/shipping_lane_analysis.py | Shipping lane artifact check | detection | Windows paths |
| pipelines/satellite/apply_opera_dswx.py | OPERA DSWX fetcher | satellite | Depends on nasa_earthdata_client |
| pipelines/satellite/fetch_ecostress_data.py | ECOSTRESS data fetcher | satellite | TODO: needs real collection ID |
| pipelines/satellite/fetch_swot_data.py | SWOT data fetcher | satellite | TODO: needs real collection ID |
| pipelines/satellite/probe_sources.py | Magnetic source URL prober | satellite | Diagnostic/discovery |
| pipelines/satellite/run_ingest_new.py | Stage ingest runner | satellite | Thin wrapper |
| pipelines/satellite/summarize_ingest.py | Ingest summary | satellite | Windows paths |
| pipelines/satellite/wh2k_build_satellite_bundle_manifest.py | Bundle manifest builder | satellite | Inventory tool |
| pipelines/bag/bag_alignment_corrector.py | BAG alignment correction | detection | Logging setup only visible |
| pipelines/bag/bag_metadata_analyzer.py | BAG metadata analysis | detection | Logging setup only visible |
| pipelines/bag/bag_analyzer_gui.py | BAG analyzer GUI tab | tools | Tkinter, subprocess |
| pipelines/bag/add_meshes.py | Adds mesh code to lib.rs | tools | One-shot code generator |
| pipelines/bag/clean_bag_mesh.py | Cleans bag_mesh.rs | tools | One-shot fixer |
| pipelines/bag/advanced_bag_scanner_runner.py | Scanner runner with DB | detection | Orchestrator for scanner |
| pipelines/bag/compare_scanners.py | Scanner comparison tool | detection | Windows paths |
| pipelines/bag/convert_and_match.py | Coordinate conversion + match | detection | Hardcoded Cedarville coords |
| pipelines/bag/cross_section.py | BAG cross-section viewer | detection | Windows paths, matplotlib |
| pipelines/bag/calc_orientation.py | Orientation calculator | detection | Windows paths, matplotlib |
| pipelines/bag/check_volume.py | Volume check from BAG | detection | Windows paths |
| models/ml/inference/deep_vision_master_protocol.py | M2200 forensic deep vision | ml | Depends on recovered/ module |
| models/ml/inference/live_deep_vision.py | Live deep vision execution | ml | Depends on recovered/ module |
| models/ml/inference/phase4_6_postprocess.py | Phase 4.6 postprocessing | ml | Depends on recovered/ module |
| models/ml/inference/phase5_debloom_search.py | Phase 5 de-blooming | ml | Depends on recovered/ module |
| models/ml/inference/discovery_master_protocol.py | Discovery master protocol | ml | CSV/KML output helpers |
| models/ml/inference/cedarville_benchmark.py | Cedarville benchmark harness | ml | Synthetic fallback if no data |
| models/ml/inference/wreck_vs_obstruction_classifier.py | Wreck vs obstruction RF | ml | Placeholder feature extractors |
| models/ml/inference/contrast_squeeze.py | Contrast squeeze analysis | ml | Requires specific .npy file |
| models/ml/training/erie_agent_model.py | Erie agent model saver | ml | Windows paths, one-shot |
| models/ml/training/wh2k_phase2_gpu.py | Phase 2 GPU (duplicate?) | ml | Very similar to _training version |
| news_search/probe_apis.py | NRCan/DataCite API prober | tools | Diagnostic/discovery |
| news_search/test_search_apis.py | ScienceBase/NCEI API test | tools | Diagnostic |

### TESTS (keep for reference)

| File | Purpose |
|------|---------|
| test_cmr.py | CMR API connectivity test |
| test_cmr2.py | CMR API test variant 2 |
| test_cmr3.py | CMR API test variant 3 |
| test_cmr4.py | CMR API test variant 4 |
| test_cmr5.py | CMR API test variant 5 |
| test_hls_download.py | HLS download test |
| test_kobold_api.py | KoboldCPP API test |
| test_p1000.py | P1000 node test |
| test_push.py | Push/deploy test |
| test_ssh.py | SSH connectivity test |
| test_vision_model.py | Vision model test |
| news_search/test_ca.py | Chronicling America API test |
| scripts/test_mackinac_scan.py | Mackinac scan pipeline test |
| scripts/test_pipeline_cpu.py | CPU pipeline test |
| scripts/small_batch_test.py | Small batch OOM-safe test |
| scripts/ea_dl_test.py | Earthaccess download test |
| scripts/s3dl_test.py | S3 download test |
| scripts/s3getobj_test.py | S3 GetObject test |
| scripts/_test_claim.py | Job claim test |
| scripts/_test_drive_discovery.py | Drive discovery smoke test |
| scripts/_smoke_test_pipeline.py | Pipeline smoke test |
| scripts/nasa_fusion_test.py → pipelines/satellite/nasa_fusion_test.py | NASA fusion confidence test |
| cesarops-inference/tests/cpu_smoke_test.rs | Full inference pipeline CPU test |
| nauticuvs/tests/integration_backward_compat.rs | Backward compatibility test |
| nauticuvs/tests/integration_internal_weights.rs | Internal weights integration test |
| nautivecs/tests/pipeline_integration.rs | Pipeline integration test |
| cesarops-slicer/test_poll.rs | wgpu PollType test |
| cesarops-xbox-worker/src/test.rs | itertools test |
| cesarops-xbox-worker/test.rs | ORT session test |

### DEAD CODE / STUBS (can delete)

| File | Reason |
|------|--------|
| cesarops-drift-worker/src/main.rs | Stub: only `println!("Hello, world!")` |
| cesarops-historical-worker/src/main.rs | Stub: only `println!("Hello, world!")` |
| cesarops-slicer/src/chemistry_specialist.rs | Stub: only `fn main() {}` |
| cesarops-xbox-worker/target2/release/build/serde-ca321832affbf15d/out/private.rs | Auto-generated build artifact |
| cesarops-xbox-worker/target2/release/build/serde_core-fa2b01517e953ed8/out/private.rs | Auto-generated build artifact |
| cesarops-xbox-worker/target_cli/release/build/serde_core-56d82c0e507727be/out/private.rs | Auto-generated build artifact |
| cesarops-xbox-worker/target_cli/release/build/serde-e622f355d06ed664/out/private.rs | Auto-generated build artifact |
| cesarops-xbox-worker/target_cli/release/build/thiserror-8616acc93121dc07/out/private.rs | Auto-generated build artifact |
| models/onnx/phi3-mini-directml/directml/directml-int4-awq-block-128/configuration_phi3.py | Third-party model config (Microsoft) |
| debug_trainer.py | Crash wrapper for wreck_ml_trainer (one-off) |
| pipelines/bag/attention_processor.py | HuggingFace library code (not project code) |
| scripts/create_fake_model.py | Creates fake model for dashboard boot test |
| scripts/fix_arena_mlock.py | One-shot code fix (already applied) |
| scripts/fix_config.py | One-shot JSON fix |
| scripts/fix_deploy_script.py | One-shot script fix |
| scripts/fix_gasket.py | One-shot gasket driver fix |
| scripts/fix_gasket2.py | One-shot gasket driver fix v2 |
| scripts/fix_mc.py | One-shot mission control build fix |
| scripts/fix_thought_nautivecs.py | One-shot nautivecs client fix |
| scripts/pi_clean_stubs.py | One-shot stub package removal |
| scripts/add_detection_ws.py | One-shot Cargo.toml edit |

### DUPLICATES (already in deploy/ or superseded)

| File | Better version at |
|------|-------------------|
| hls_dl.py | Duplicate of hls_download.py (same code) |
| hls_dl2.py | Duplicate of hls_download.py (same code) |
| hls_download2.py | Duplicate of hls_download.py (same code) |
| hls_download3.py | Duplicate of hls_download.py (same code) |
| hls_download.py | All 5 HLS download variants are identical |
| download_erie_central.py | download_erie_central2.py is the same |
| download_erie_central2.py | Duplicate of download_erie_central.py |
| cesarops_core/simple_fast_engine.py | Simplified version of fast_drift_engine.py (keep both — different deps) |
| pipelines/satellite/install_sentinel_deps.py | One-shot installer, not a tool |
| pipelines/satellite/check_sentinel_deps.py | One-shot checker, not a tool |

### SCRIPTS — Infrastructure/Ops (useful but machine-specific)

| File | Purpose | Category |
|------|---------|----------|
| scripts/_i7_setup.py | i7 node one-shot setup | ops |
| scripts/_i7_check.py | i7 state check + GDAL install | ops |
| scripts/_i7_finalize.py | i7 .env + node_update.sh | ops |
| scripts/_i7_fix_deps_rescan.py | i7 dep install + rescan | ops |
| scripts/_i7_install_deps.py | i7 full dep install | ops |
| scripts/_i7_launch_cpu_passes.py | i7 CPU scan launcher | ops |
| scripts/_i7_setup_armor_host.py | i7 ArmorATD udev + samba | ops |
| scripts/_i7_transfer_and_scan.py | SFTP transfer + scan | ops |
| scripts/_xeon_bootstrap.py | Xeon one-shot bootstrap | ops |
| scripts/_xeon_bootstrap2.py | Xeon bootstrap v2 | ops |
| scripts/_xeon_relaunch_scan.py | Xeon symlink + relaunch | ops |
| scripts/_xeon_scan_status.py | Xeon scan status check | ops |
| scripts/_xeon_transfer_scan.py | Xeon SFTP transfer + scan | ops |
| scripts/_mount_armor_xeon.py | Mount i7 ArmorATD on Xeon | ops |
| scripts/_remount_i7_downloads_xeon.py | Remount i7 downloads | ops |
| scripts/_fix_samba_i7.py | Fix Samba share on i7 | ops |
| scripts/_find_i7_tifs.py | Find TIFs on i7 | ops |
| scripts/_get_hw_ids.py | Get hardware IDs from i7 | ops |
| scripts/_migrate_to_armor.py | Migrate TIFs to ArmorATD | ops |
| scripts/_push_migration_to_pi.py | Push migration to Pi | ops |
| scripts/_continuous_download.py | Continuous HLS downloader | ops |
| scripts/_download_wreck_tiles.py | Download tiles at wreck GPS | ops |
| scripts/_check_db.py | Check census DB | ops |
| scripts/_check_tile_extents.py | Check tile WGS84 extents | ops |
| scripts/_inspect_queue.py | Inspect scan queue | ops |
| scripts/_scan_status.py | Check running scan status | ops |
| scripts/_status_check.py | Multi-node status check | ops |
| scripts/_reset_jobs.py | Reset failed/running jobs | ops |
| scripts/_reset_running.py | Reset running jobs | ops |
| scripts/_patch_job_dates.py | Patch missing job dates | ops |
| scripts/_probe_db.py | Probe wrecks.db tables | ops |
| scripts/_probe_db2.py | Probe features by source | ops |
| scripts/_probe_db3.py | Probe ThunderBay features | ops |
| scripts/_pq.py | Quick queue status | ops |
| scripts/_pq2.py | Queue status with params | ops |
| scripts/reset_queue.py | Reset queue jobs | ops |
| scripts/patch_failed_jobs.py | Patch failed jobs with data | ops |
| scripts/tag_job_types.py | Tag jobs with correct type | ops |
| scripts/migrate_add_job_type.py | DB migration: add columns | ops |
| scripts/ionos_ddns.py | IONOS Dynamic DNS updater | ops |
| scripts/push_tailscale_acl.py | Push Tailscale ACL policy | ops |
| scripts/install_api_service.py | Install wrecks_api systemd | ops |
| scripts/deploy_cesarops3.py | Deploy to cesarops3 node | ops |
| scripts/deploy_web.py | Deploy web frontend | ops |
| scripts/setup_bond.py | Bonded NICs on T440 | ops |
| scripts/setup_eno2.py | Second NIC with DHCP | ops |
| scripts/check_cuda.py | CUDA availability check | ops |
| scripts/check_xenon_cuda.py | Xenon CUDA check via SSH | ops |
| scripts/diagnose_xeon_gpu.py | Xeon GPU BIOS diagnostic | ops |
| scripts/launch_koboldcpp.py | Smart KoboldCPP launcher | ops |
| scripts/credential_inventory.py | Credential reference scanner | ops |
| scripts/read_inventory.py | Read credential inventory | ops |
| scripts/dynamic_db_key.py | Dynamic DB key from drive | ops |
| scripts/generate_db_master_key.py | Master key from HD serial | ops |
| scripts/inventory_all_files.py | File categorization | ops |
| scripts/inventory_geotiffs.py | GeoTIFF catalog | ops |
| scripts/cleanup_and_organize.py | Cleanup script | ops |
| scripts/wipe_database.py | Wipe DB (keep schema) | ops |
| scripts/populate_database.py | Populate DB from results | ops |
| scripts/process_tiles.py | Process tiles (laptop+Xenon) | ops |
| scripts/export_pipeline_kmz.py | Export all missions to KMZ | ops |
| scripts/export_wrecks_kmz.py | Export wrecks.db to KMZ | ops |
| scripts/extract_oil_spills_kmz.py | Extract oil spill KMZ | ops |
| scripts/full_lake_michigan_run.py | Full Michigan+Superior run | ops |
| scripts/download_argo_all_sats.py | Argo area satellite download | ops |
| scripts/download_argo_tiles.py | Argo Landsat tile download | ops |
| scripts/search_argo_landsat.py | Search AWS STAC for Argo | ops |
| update_sovereign_systemd.py | Update systemd service | ops |
| quick_gpu_status.py | Quick GPU status check | ops |
| pi_probe.py | Pi disk usage check | ops |
| check_queue.py | Queue table inspector | ops |

### SCRIPTS — LLM Build/Research (agent-driven development)

| File | Purpose | Category |
|------|---------|----------|
| scripts/adaptive_pipeline_build.py | Ask 35B to design adaptive pipeline | agent |
| scripts/blind_validation_scan.py | Ask 35B for blind validation | agent |
| scripts/build_from_specs.py | Read specs, have 35B write code | agent |
| scripts/build_loop.py | Automated build loop (R1+Strand) | agent |
| scripts/build_mission_control.py | Ask 35B to implement mission control | agent |
| scripts/build_orchestrator.py | Build orchestrator from specs | agent |
| scripts/build_self_healing.py | Build self-healing supervisor | agent |
| scripts/build_supervisor.py | Have 35B write Rust supervisor | agent |
| scripts/build_tauri_and_deploy.py | Ask 35B for Tauri wrapper | agent |
| scripts/build_two_layer_supervisor.py | Two-layer supervisor architecture | agent |
| scripts/cake_distributed_research.py | Ask 35B about Cake distributed | agent |
| scripts/deep_analysis_v2.py | Deep codebase analysis v2 | agent |
| scripts/deep_codebase_analysis.py | Deep analysis via Thought Engine | agent |
| scripts/deep_scan_spec.py | Deep scan → full sensor spec | agent |
| scripts/extend_supervisor.py | Extend supervisor for all infra | agent |
| scripts/feed_r1_chunked.py | Feed spec to R1-32B chunked | agent |
| scripts/feed_r1_spec.py | Feed full spec to R1 | agent |
| scripts/mission_control_spec.py | Ask 35B for mission control design | agent |
| scripts/r1_router.py | R1 tool router (nautivecs+WSO) | agent |
| scripts/run_research_overnight.py | Overnight research daemon | agent |
| scripts/run_whitepaper_pipeline.py | Whitepaper through nautivecs+LLM | agent |
| scripts/wire_pipeline.py | Ask 35B to wire pieces together | agent |

---

## Summary Statistics

| Category | Python Files | Rust Files | Total |
|----------|-------------|-----------|-------|
| Working Tools | ~180 | ~165 | ~345 |
| Useful But Needs Work | ~65 | 0 | ~65 |
| Tests | ~17 | ~6 | ~23 |
| Dead Code / Stubs | ~12 | ~8 | ~20 |
| Duplicates | ~7 | 0 | ~7 |
| Ops/Infrastructure Scripts | ~75 | 0 | ~75 |
| LLM Build Scripts | ~22 | 0 | ~22 |

## Priority Recovery Order

1. **CRITICAL** — cesarops_core/ (drift engines, sonar tools, sensor fusion, rosa_case)
2. **CRITICAL** — nauticuvs/ (curvelet math engine — everything depends on this)
3. **CRITICAL** — nautivecs/ (code vectorization — agent grounding depends on this)
4. **HIGH** — cesarops-detection/ (Triple-Lock pipeline)
5. **HIGH** — cesarops-slicer/ (GeoTIFF tile extraction)
6. **HIGH** — cesarops-inference/ (native LLM engine)
7. **HIGH** — cesarops-mcp-steered/ (MCP + SCM agent)
8. **HIGH** — pipelines/mag/ (magnetic anomaly detection)
9. **HIGH** — pipelines/satellite/ (satellite data acquisition)
10. **MEDIUM** — cesarops-satellite-worker/ (TPU pipeline)
11. **MEDIUM** — cesarops-hybrid-engine/ (dual P100 engine)
12. **MEDIUM** — sovereign-cloud/ (cluster orchestrator)
13. **MEDIUM** — sentinel_hunt_src/ (Sentinel detection)
14. **MEDIUM** — models/ml/ (training + inference)
15. **MEDIUM** — pipelines/bag/ (BAG file analysis)
16. **LOW** — cesarops-forge/ and forge-v2/ (dev agent — can rebuild)
17. **LOW** — tauri/ (desktop app wrapper)
18. **LOW** — warp-grid/ (tensor buffer management)
19. **LOW** — scripts/ (ops scripts — machine-specific)
