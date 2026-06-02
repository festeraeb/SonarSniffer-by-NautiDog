# Rust port grades (51-module queue)

**Graded:** 51 | **Avg score:** 88.0/100

| Grade | Count | Meaning |
|-------|-------|---------|
| A | {A} | Logic + tests + solid API |
| B | {B} | Good integrate-layer port |
| C | {C} | Stub/constants; needs depth |
| D | {D} | Very thin |
| F | {F} | Missing |


## By module

| ID | Grade | Score | Lines | Tests | Rust | Forge MD | Notes |
|----|-------|-------|-------|-------|------|----------|-------|
| 1 | **A** | 100 | 113 | yes | `audit_wrecks_db.rs` | — | — |
| 4 | **A** | 100 | 71 | yes | `bridge_calibrate.rs` | — | — |
| 5 | **A** | 100 | 90 | yes | `cuda_env.rs` | yes | I/O not wired (logic-only port) |
| 7 | **A** | 100 | 137 | yes | `database_connector.rs` | — | — |
| 9 | **A** | 100 | 61 | yes | `erie_multiyear_downloader.rs` | — | — |
| 10 | **A** | 100 | 90 | yes | `extract_oil_spills_kmz.rs` | — | — |
| 11 | **A** | 100 | 136 | yes | `fetcher.rs` | — | — |
| 12 | **A** | 100 | 127 | yes | `find_swot_dates.rs` | — | — |
| 15 | **A** | 100 | 77 | yes | `db_master_key.rs` | — | — |
| 16 | **A** | 100 | 141 | yes | `global_controls.rs` | — | — |
| 17 | **A** | 100 | 62 | yes | `gpu_stress_test.rs` | — | — |
| 18 | **A** | 100 | 104 | yes | `init_database.rs` | — | I/O not wired (logic-only port) |
| 19 | **A** | 100 | 67 | yes | `file_inventory.rs` | — | — |
| 20 | **A** | 100 | 81 | yes | `geotiff_inventory.rs` | — | — |
| 24 | **A** | 100 | 45 | yes | `monster_candidate.rs` | — | — |
| 31 | **A** | 100 | 151 | yes | `repeatability_check.rs` | — | — |
| 37 | **A** | 100 | 56 | yes | `xenon_sync.rs` | — | — |
| 38 | **A** | 100 | 58 | yes | `sync_xenon.rs` | — | — |
| 45 | **A** | 100 | 101 | yes | `three_tile_offset.rs` | — | — |
| 46 | **A** | 100 | 82 | yes | `tpu_client.rs` | — | — |
| 49 | **A** | 100 | 89 | yes | `cuda_verification.rs` | — | — |
| 34 | **A** | 95 | 67 | yes | `run_zero_baseline.rs` | — | — |
| 39 | **A** | 95 | 49 | yes | `gpu_health.rs` | — | — |
| 47 | **A** | 95 | 61 | yes | `tpu_server.rs` | — | — |
| 8 | **A** | 85 | 97 | no | `db_ingestor.rs` | — | I/O not wired (logic-only port) |
| 2 | **B** | 80 | 47 | no | `hls_b02_download.rs` | yes | — |
| 14 | **B** | 80 | 68 | no | `full_scan.rs` | — | — |
| 21 | **B** | 80 | 43 | no | `iowa_202_analysis.rs` | — | — |
| 22 | **B** | 80 | 42 | no | `live_feed_server.rs` | — | — |
| 23 | **B** | 80 | 41 | no | `monster_analysis.rs` | — | — |
| 25 | **B** | 80 | 78 | no | `populate_database.rs` | — | — |
| 27 | **B** | 80 | 66 | no | `process_tiles.rs` | — | — |
| 29 | **B** | 80 | 55 | no | `pull_altimetry_anonymous.rs` | — | — |
| 32 | **B** | 80 | 56 | no | `run_configured_pipeline.rs` | — | — |
| 35 | **B** | 80 | 56 | no | `small_batch_test.rs` | — | — |
| 42 | **B** | 80 | 42 | no | `gpu_test.rs` | — | — |
| 44 | **B** | 80 | 53 | no | `test_repeatability.rs` | — | — |
| 48 | **B** | 80 | 46 | no | `thermal_validation.rs` | — | — |
| 50 | **B** | 80 | 47 | no | `viirs_multi_year_scan.rs` | — | — |
| 51 | **B** | 80 | 69 | no | `zion_trench_squeeze.rs` | — | — |
| 3 | **B** | 75 | 72 | no | `batch_download_manager.rs` | — | — |
| 6 | **B** | 75 | 48 | no | `cuda_test_kmz.rs` | — | — |
| 13 | **B** | 75 | 69 | no | `full_lake_michigan_run.rs` | — | — |
| 26 | **B** | 75 | 59 | no | `prioritized_satellite_pull.rs` | — | — |
| 28 | **B** | 75 | 56 | no | `process_with_coordinates.rs` | — | — |
| 30 | **B** | 75 | 45 | no | `raw_scan_reprocess.rs` | — | — |
| 33 | **B** | 75 | 32 | no | `straits_fox_pipeline.rs` | — | — |
| 36 | **B** | 75 | 42 | no | `smart_daily_scan.rs` | — | — |
| 41 | **B** | 75 | 35 | no | `m2200_gpu_test.rs` | — | — |
| 43 | **B** | 75 | 35 | no | `pipeline_test.rs` | — | — |
| 40 | **B** | 70 | 32 | no | `gpu_detection.rs` | yes | — |
