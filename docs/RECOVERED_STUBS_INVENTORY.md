# Recovered / partial stubs inventory

Files with TODO/FIXME/NotImplemented, many empty functions, or very short bodies.
Use with `PIPELINE_IMPLEMENTATION_PATH.md` before archiving.

## live_pipelines (`/codebase/projects/pipelines`)

**Flagged on canonical live tree:**

- `bag/attention_processor.py` — **stub** (many TODO/empty funcs; same file in laptop + backup)
- `satellite/nasa_fusion_test.py` — **stub**
- `bag/bag_wreck_detector.py` — **partial** (review before archive)

## repo_root_py_only (`/codebase/repos/wreckhunter2000-1/*.py`)

**Flagged:** 1

- `research_agent.py` — **partial**, 391 lines

## in_repo_backup_garbage (archive whole `backup/` — do not merge)

**Examples under `backup/` (225+ flagged `.py` — mostly `__init__.py`, vendored gstreamer, duplicates):**

- `backup/deploy/tools/pipelines/bag/attention_processor.py` — **stub**, 5677 lines, score=101
- `backup/wreckhunter2000-1/pipelines/bag/attention_processor.py` — **stub**, 5677 lines, score=101
- `backup/sonarsniffer/SonarSniffer/src-tauri/gstreamer/lib/site-packages/pygtkcompat/generictreemodel.py` — **stub**, 426 lines, score=49
- `backup/sonarsniffer/SonarSniffer/src-tauri/gstreamer/lib/gst-validate-launcher/python/launcher/baseclasses.py` — **stub**, 3063 lines, score=32
- `backup/sonarsniffer/SonarSniffer/src-tauri/gstreamer/lib/gst-validate-launcher/python/launcher/loggable.py` — **stub**, 1058 lines, score=30
- `backup/deploy/docs/CESARops_repo/create_config.py` — **stub**, 21 lines, score=23
- `backup/deploy/scripts/scripts/fix_arena_mlock.py` — **stub**, 23 lines, score=23
- `backup/deploy/tools/gl-wrecks-api/wrecks_api/__init__.py` — **stub**, 0 lines, score=23
- `backup/deploy/tools/pipelines/bag/clean_bag_mesh.py` — **stub**, 23 lines, score=23
- `backup/deploy/tools/pipelines/satellite/summarize_ingest.py` — **stub**, 12 lines, score=23
- `backup/deploy/tools/wrecks_api/__init__.py` — **stub**, 1 lines, score=23
- `backup/deploy/tools/wrecks_api/stages/__init__.py` — **stub**, 1 lines, score=23
- `backup/sonarsniffer/SonarSniffer/src-tauri/gstreamer/lib/gst-validate-launcher/python/launcher/__init__.py` — **stub**, 21 lines, score=23
- `backup/sonarsniffer/SonarSniffer/src-tauri/gstreamer/lib/gst-validate-launcher/python/launcher/apps/__init__.py` — **stub**, 0 lines, score=23
- `backup/sonarsniffer/SonarSniffer/src-tauri/gstreamer/lib/gst-validate-launcher/python/launcher/config.py` — **stub**, 24 lines, score=23
- `backup/sonarsniffer/SonarSniffer/src-tauri/gstreamer/lib/site-packages/pygtkcompat/__init__.py` — **stub**, 20 lines, score=23
- `backup/src/scripts/fix_arena_mlock.py` — **stub**, 23 lines, score=23
- `backup/src/wrecks_api/__init__.py` — **stub**, 1 lines, score=23
- `backup/src/wrecks_api/stages/__init__.py` — **stub**, 1 lines, score=23
- `backup/temp-md-scanner/main.py` — **stub**, 6 lines, score=23
- `backup/temp-md-scanner/md_scanner/__init__.py` — **stub**, 9 lines, score=23
- `backup/temp-md-scanner/md_scanner/learning/__init__.py` — **stub**, 14 lines, score=23
- `backup/wreckhunter2000-1/check_landsat.py` — **stub**, 16 lines, score=23
- `backup/wreckhunter2000-1/check_queue.py` — **stub**, 17 lines, score=23
- `backup/wreckhunter2000-1/debug_trainer.py` — **stub**, 12 lines, score=23

## laptop_programming (`/codebase/repos/laptop-code`)

**Flagged:** 30 (showing top 25)

- `pipelines/bag/attention_processor.py` — **stub**, 5677 lines, score=101
- `cesarops_core/src/sonarsniffer/sonar_parser.py` — **stub**, 175 lines, score=24
- `CESARops/create_config.py` — **stub**, 21 lines, score=23
- `cesarops/src/cesarops/scanner/__init__.py` — **stub**, 24 lines, score=23
- `cesarops/tests/__init__.py` — **stub**, 1 lines, score=23
- `pipelines/bag/clean_bag_mesh.py` — **stub**, 23 lines, score=23
- `pipelines/satellite/summarize_ingest.py` — **stub**, 12 lines, score=23
- `wrecks_api/__init__.py` — **stub**, 1 lines, score=23
- `wrecks_api/stages/__init__.py` — **stub**, 1 lines, score=23
- `CESARops/check_python_version.py` — **stub**, 15 lines, score=15
- `CESARops/check_tkinter.py` — **stub**, 16 lines, score=15
- `CESARops/create_run_script.py` — **stub**, 21 lines, score=15
- `pipelines/satellite/check_sentinel_deps.py` — **stub**, 20 lines, score=15
- `pipelines/satellite/install_sentinel_deps.py` — **stub**, 20 lines, score=15
- `ml/inference/deep_water_detection.py` — **stub**, 166 lines, score=12
- `pipelines/satellite/nasa_fusion_test.py` — **stub**, 88 lines, score=12
- `cesarops/src/cesarops/drift/sarops_core.py` — **partial**, 2638 lines, score=9
- `cesarops_core/sarops.py` — **partial**, 2638 lines, score=9
- `cesarops_core/src/sonarsniffer/ml_pipeline.py` — **partial**, 332 lines, score=9
- `cesarops_core/src/sonarsniffer/web_dashboard_generator.py` — **partial**, 1171 lines, score=9
- `ml/inference/sdb.py` — **partial**, 124 lines, score=9
- `ml/inference/wreck_vs_obstruction_classifier.py` — **partial**, 76 lines, score=9
- `cesarops/src/cesarops/__init__.py` — **partial**, 40 lines, score=8
- `ml/training/erie_agent_model.py` — **partial**, 42 lines, score=8
- `pipelines/bag/check_volume.py` — **partial**, 43 lines, score=8

## backup_pipelines (`/codebase/repos/wreckhunter2000-1/backup/deploy/tools/pipelines`)

**Flagged:** 8 (showing top 8)

- `bag/attention_processor.py` — **stub**, 5677 lines, score=101
- `bag/clean_bag_mesh.py` — **stub**, 23 lines, score=23
- `satellite/summarize_ingest.py` — **stub**, 12 lines, score=23
- `satellite/check_sentinel_deps.py` — **stub**, 20 lines, score=15
- `satellite/nasa_fusion_test.py` — **stub**, 88 lines, score=12
- `bag/check_volume.py` — **partial**, 43 lines, score=8
- `bag/cross_section.py` — **partial**, 44 lines, score=8
- `bag/bag_wreck_detector.py` — **partial**, 1535 lines, score=5

## Pipelines canonical rule

- **Keep:** `/codebase/projects/pipelines/`
- **Archive after backup:** `backup/deploy/tools/pipelines/`, `laptop-code/pipelines/`
- **Fix or archive on live:** `bag/attention_processor.py`, `satellite/nasa_fusion_test.py`
