# Pipeline implementation path (laptop vs live)

**Generated:** 2026-05-26 02:27 UTC

## Paths

| Role | Path |
|------|------|
| **Canonical (live)** | `/codebase/projects/pipelines` |
| Repo symlink | `/codebase/repos/wreckhunter2000-1/pipelines` → live |
| Laptop dump | `/codebase/repos/laptop-code/pipelines` |
| In-repo backup (archive target) | `/codebase/repos/wreckhunter2000-1/backup/deploy/tools/pipelines` |

## Executive summary

| Metric | Count |
|---|---|
| Archive | 3 |
| Partial | 3 |
| Stub | 2 |
| Working | 103 |

### By presence
| Presence | Count |
|---|---|
| backup_only | 3 |
| diverged | 4 |
| identical | 87 |
| live_only | 17 |

### Recommended actions
| Action | Count |
|---|---|
| KEEP | 99 |
| KEEP_LIVE_DIFF | 4 |
| REVIEW | 3 |
| DELETE_BACKUP_COPY | 3 |
| FIX_OR_ARCHIVE | 2 |

## Unification plan

### Phase 1 — Backup (do first)

```bash
TS=$(date -u +%Y%m%d)
sudo mkdir -p /data/backups
tar -czf /data/backups/pipelines-live-$TS.tar.gz -C /codebase/projects pipelines
tar -czf /data/backups/laptop-pipelines-$TS.tar.gz -C /codebase/repos/laptop-code pipelines
tar -czf /data/backups/wreckhunter2000-1-$TS.tar.gz \
  -C /codebase/repos wreckhunter2000-1 \
  --exclude=wreckhunter2000-1/target \
  --exclude=wreckhunter2000-1/.cargo-docker \
  --exclude=wreckhunter2000-1/backup
```

### Phase 2 — Canonical tree

- Keep **`/codebase/projects/pipelines/`** as the only writable pipeline tree.
- Forge, wrecks_api, and orchestrator already use `repo/pipelines` symlink.

### Phase 3 — Port laptop-only (real code)

Copy only files marked **PORT_TO_LIVE** below; run `python3 -m py_compile` after.

### Phase 4 — Archive duplicates

- Move `backup/deploy/tools/pipelines/` → `/data/backups/archive-in-repo-pipelines-$TS/`
- After porting, move `laptop-code/pipelines/` → `/data/backups/laptop-pipelines-$TS/` (keep read-only)
- Do **not** delete `/data/laptopdump` until backups verified.

### Phase 5 — Stub cleanup

- Files marked **FIX_OR_ARCHIVE** or **Stub**: either implement, wire to Forge, or move to `archive/stubs/`.

## Per-file matrix

| Path | Status | Presence | Live lines | Stub | Action | L | V | B |
|------|--------|----------|----------:|-----:|--------|---|---|---|
| **MAG** | | | | | | | | |
| `mag/adaptive_background_scan.py` | Working | diverged | 280 | 0 | KEEP_LIVE_DIFF | ✓ | ✓ | ✓ |
| `mag/confidence_reporting.py` | Working | identical | 30 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/crossmatch_optical_mag.py` | Working | identical | 75 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/datum_correction.py` | Working | identical | 643 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/dipole_analysis.py` | Working | identical | 430 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/directional_pull.py` | Working | identical | 292 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/erie_central_aeromag_orchestrator.py` | Working | live_only | 743 | 0 | KEEP | · | ✓ | · |
| `mag/erie_direct_match.py` | Working | identical | 368 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/erie_feedback_loop.py` | Working | identical | 316 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/erie_known_wrecks_db.py` | Working | identical | 454 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/erie_mag_validation.py` | Working | live_only | 211 | 0 | KEEP | · | ✓ | · |
| `mag/erie_scanner_pipeline.py` | Working | identical | 489 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/erie_synthetic_dipole.py` | Working | identical | 495 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/erie_wellhead_discriminator.py` | Working | diverged | 480 | 0 | KEEP_LIVE_DIFF | ✓ | ✓ | ✓ |
| `mag/export_huron_mag_kml.py` | Working | identical | 226 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/final_magnetic_fusion.py` | Working | identical | 32 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/flight_line_physics.py` | Working | identical | 456 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/flight_line_physics_v2.py` | Working | identical | 469 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/forge_cli.py` | Working | live_only | 153 | 0 | KEEP | · | ✓ | · |
| `mag/generate_combined_kml.py` | Working | identical | 275 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/generate_kml.py` | Working | identical | 288 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/geo_filter_candidates.py` | Working | identical | 553 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/huron_mag_water_scan.py` | Working | identical | 158 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/ingest_new_mag.py` | Working | identical | 51 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/loran_c_warp.py` | Working | identical | 686 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/mag_data_manager.py` | Working | identical | 224 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/mag_data_pipeline.py` | Working | diverged | 1412 | 0 | KEEP_LIVE_DIFF | ✓ | ✓ | ✓ |
| `mag/mag_dipole_compare.py` | Working | live_only | 141 | 0 | KEEP | · | ✓ | · |
| `mag/mag_erie_fetch.py` | Working | live_only | 305 | 0 | KEEP | · | ✓ | · |
| `mag/mag_gpu_dipole.py` | Working | live_only | 196 | 0 | KEEP | · | ✓ | · |
| `mag/mag_lake_harvester.py` | Working | identical | 2814 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/mag_preprocess.py` | Working | live_only | 28 | 0 | KEEP | · | ✓ | · |
| `mag/mag_runtime.py` | Working | live_only | 269 | 0 | KEEP | · | ✓ | · |
| `mag/mag_rust_detect.py` | Working | live_only | 58 | 0 | KEEP | · | ✓ | · |
| `mag/multisource_depth_proximity_ranker.py` | Working | identical | 246 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/nauticuvs_mag_curvelet.py` | Working | live_only | 94 | 0 | KEEP | · | ✓ | · |
| `mag/normalize_nrcan_csvs.py` | Working | identical | 99 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/ogsrl_well_discriminator.py` | Archive | backup_only | - | 0 | DELETE_BACKUP_COPY | · | · | ✓ |
| `mag/parse_kml.py` | Working | identical | 42 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/run_all_source_scan.py` | Working | identical | 271 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/run_lake_scans.py` | Working | identical | 198 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/shipping_lane_analysis.py` | Working | identical | 295 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/test_mag_pipeline.py` | Working | live_only | 84 | 0 | KEEP | · | ✓ | · |
| `mag/wh2k_awois_scraper.py` | Working | identical | 777 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_data_provenance.py` | Working | identical | 426 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_digest_new_mag_drop.py` | Working | identical | 164 | 3 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_discovery_report_standalone.py` | Working | identical | 306 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_discovery_report_v2.py` | Working | identical | 594 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_export_kml.py` | Working | identical | 475 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_extract_real_tiles.py` | Working | identical | 397 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_harvester.py` | Working | identical | 733 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_ingest_gsc_erie.py` | Working | identical | 230 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_ncei_fetch.py` | Partial | identical | 721 | 9 | REVIEW | ✓ | ✓ | ✓ |
| `mag/wh2k_rasterize_erie_csv.py` | Working | identical | 311 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_rasterize_huron_csv.py` | Working | identical | 270 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_satellite_mag_validate.py` | Working | identical | 490 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_upward_continuation.py` | Working | identical | 348 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_warp_field_export.py` | Working | identical | 241 | 0 | KEEP | ✓ | ✓ | ✓ |
| `mag/wh2k_wreck_anchor_verifier.py` | Working | identical | 720 | 0 | KEEP | ✓ | ✓ | ✓ |
| **SATELLITE** | | | | | | | | |
| `satellite/__init__.py` | Archive | backup_only | - | 0 | DELETE_BACKUP_COPY | · | · | ✓ |
| `satellite/apply_opera_dswx.py` | Working | identical | 49 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/batch_download_manager.py` | Archive | backup_only | - | 0 | DELETE_BACKUP_COPY | · | · | ✓ |
| `satellite/buoy_analog.py` | Working | identical | 548 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/check_sentinel_deps.py` | Working | identical | 20 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/fetch_ecostress_data.py` | Working | identical | 47 | 3 | KEEP | ✓ | ✓ | ✓ |
| `satellite/fetch_swot_data.py` | Working | identical | 47 | 3 | KEEP | ✓ | ✓ | ✓ |
| `satellite/forge_cli.py` | Working | live_only | 91 | 0 | KEEP | · | ✓ | · |
| `satellite/historical_drift.py` | Working | identical | 619 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/install_sentinel_deps.py` | Working | identical | 20 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/nasa_earthdata_client.py` | Working | identical | 235 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/nasa_fusion_test.py` | Stub | identical | 88 | 12 | FIX_OR_ARCHIVE | ✓ | ✓ | ✓ |
| `satellite/probe_sources.py` | Working | identical | 68 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/run_ingest_new.py` | Working | identical | 34 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/sar_temporal_persistence.py` | Working | identical | 47 | 3 | KEEP | ✓ | ✓ | ✓ |
| `satellite/sat_mission_orchestrator.py` | Working | live_only | 508 | 0 | KEEP | · | ✓ | · |
| `satellite/summarize_ingest.py` | Working | identical | 12 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/temporal_stack_engine.py` | Working | live_only | 143 | 0 | KEEP | · | ✓ | · |
| `satellite/tile_image_fetch.py` | Partial | live_only | 50 | 9 | REVIEW | · | ✓ | · |
| `satellite/wh2k_ab_attenuation.py` | Working | identical | 250 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/wh2k_build_satellite_bundle_manifest.py` | Working | identical | 142 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/wh2k_chip_extractor.py` | Working | identical | 480 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/wh2k_raw_ghost_zoom.py` | Working | identical | 459 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/wh2k_sentinel_cpu.py` | Working | identical | 379 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/wh2k_sentinel_optical_poc.py` | Working | identical | 949 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/wh2k_sentinel_wreck_targeting.py` | Working | identical | 854 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/wh2k_synthetic_tiles.py` | Working | identical | 639 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/wh2k_synthetic_tiles_huron.py` | Working | identical | 190 | 0 | KEEP | ✓ | ✓ | ✓ |
| `satellite/wh2k_synthetic_tiles_v2.py` | Working | identical | 962 | 0 | KEEP | ✓ | ✓ | ✓ |
| **BAG** | | | | | | | | |
| `bag/add_meshes.py` | Working | identical | 57 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/advanced_bag_scanner.py` | Working | identical | 1524 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/advanced_bag_scanner_runner.py` | Working | diverged | 336 | 0 | KEEP_LIVE_DIFF | ✓ | ✓ | ✓ |
| `bag/atl23_extract.py` | Working | identical | 384 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/attention_processor.py` | Stub | identical | 5677 | 101 | FIX_OR_ARCHIVE | ✓ | ✓ | ✓ |
| `bag/azure_vision_analyzer.py` | Working | identical | 264 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/bag_alignment_corrector.py` | Working | identical | 469 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/bag_analyzer_gui.py` | Working | identical | 59 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/bag_metadata_analyzer.py` | Working | identical | 372 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/bag_visualization_generator.py` | Working | identical | 626 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/bag_wreck_detector.py` | Partial | identical | 1535 | 5 | REVIEW | ✓ | ✓ | ✓ |
| `bag/bag_wreck_gui.py` | Working | identical | 625 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/black_hole_scanner.py` | Working | identical | 316 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/calc_orientation.py` | Working | identical | 79 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/check_volume.py` | Working | identical | 43 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/clean_bag_mesh.py` | Working | identical | 23 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/compare_scanners.py` | Working | identical | 74 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/comprehensive_bag_gui.py` | Working | identical | 718 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/comprehensive_pdf_bag_scan.py` | Working | identical | 324 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/convert_and_match.py` | Working | identical | 53 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/cross_section.py` | Working | identical | 44 | 0 | KEEP | ✓ | ✓ | ✓ |
| `bag/download_bag_files.py` | Working | live_only | 161 | 0 | KEEP | · | ✓ | · |
| `bag/forge_cli.py` | Working | live_only | 128 | 0 | KEEP | · | ✓ | · |

## Priority: PORT_TO_LIVE

- None.

## Priority: DIFF_MANUAL (live is newer — keep live unless review says otherwise)

These four files differ between laptop and live; **live copies are newer** (May 23 vs May 10) with extra physics notes and Erie ground-truth fields:

- `mag/adaptive_background_scan.py` — live adds vertical-derivative option + dipole downstream notes
- `mag/erie_wellhead_discriminator.py` — live adds `CONFIRMED_FIELD_SITES` GPS validation
- `mag/mag_data_pipeline.py` — live is superset (orchestrator/NFS paths); review diff
- `bag/advanced_bag_scanner_runner.py` — live may have Forge wiring changes; review diff

```bash
diff -u /codebase/repos/laptop-code/pipelines/FILE /codebase/projects/pipelines/FILE
```

## Priority: FIX_OR_ARCHIVE / Stub on live

- `bag/attention_processor.py` — Stub (empty_funcs=4,stub_markers=27,script_no_funcs)
- `satellite/nasa_fusion_test.py` — Stub (stub_markers=4)

## Live-only modules (already canonical)

- `bag/download_bag_files.py` (161 lines)
- `bag/forge_cli.py` (128 lines)
- `mag/erie_central_aeromag_orchestrator.py` (743 lines)
- `mag/erie_mag_validation.py` (211 lines)
- `mag/forge_cli.py` (153 lines)
- `mag/mag_dipole_compare.py` (141 lines)
- `mag/mag_erie_fetch.py` (305 lines)
- `mag/mag_gpu_dipole.py` (196 lines)
- `mag/mag_preprocess.py` (28 lines)
- `mag/mag_runtime.py` (269 lines)
- `mag/mag_rust_detect.py` (58 lines)
- `mag/nauticuvs_mag_curvelet.py` (94 lines)
- `mag/test_mag_pipeline.py` (84 lines)
- `satellite/forge_cli.py` (91 lines)
- `satellite/sat_mission_orchestrator.py` (508 lines)
- `satellite/temporal_stack_engine.py` (143 lines)
