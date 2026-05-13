# Scan Pipeline Orchestration — Requirements

## Goal

Run a blind validation scan of the Straits of Mackinac and all of Lake Erie using the full CESAROPS sensor stack, with autonomous drift correction, model loading/unloading orchestration, and result comparison against known wreck databases.

## Context

The codebase is significantly larger than what has been examined so far. Known components include but are not limited to:
- Universal satellite downloader (HLS, Sentinel-1/2, SWOT, ICESat-2, Landsat)
- Weather service (NOAA buoy integration, storm/calm classification)
- Mission control (web UI with CODE/SCAN/RESEARCH modes)
- nauticuvs (curvelet transforms for edge detection in low-res imagery)
- nautivecs (codebase search + context injection)
- cesarops-hybrid-engine (GPU cluster coordination, WGSL shaders)
- cesarops-slicer (specialized compute passes)
- PDF redaction breaker
- .bag file masking restorer
- ML detection pipeline (proven accurate on P1000 before AI layer)
- Sub-pixel slice-stitch-replace drift correction algorithm
- GitHub repos under festeraeb account (need inventory)

## Requirements

### R1: Full Codebase Inventory
- Deep scan ALL code on T440 (/home/cesarops/wreckhunter2000-1/ + shared drive)
- Deep scan local Windows machine codebase
- Inventory GitHub repos under festeraeb
- Map which tools exist where and what's missing from T440
- Identify the nauticuvs curvelet code and its integration points
- Find the sub-pixel drift correction / slice-stitch code

### R2: Pipeline Timing & Hardware Assignment
- **Dual Xeons (94GB RAM, AVX-512)**: Drift correction (sub-pixel alignment, phase correlation, FFTs)
- **Dual P100s (32GB HBM2)**: Tile batch processing (anomaly detection, curvelet filtering) — LLM UNLOADED during scan
- **1070 (8GB)**: Thought engine orchestration (Qwen3-8B reasoning, scheduling decisions)
- **1060 (6GB)**: Lightweight preprocessing, frontend serving
- **P1000 (4GB)**: Backup/auditor role
- Calculate: how many non-sliced temporal stacks fit in P100 VRAM at once?
- Calculate: processing time per tile batch on P100s vs Xeons

### R3: Model Loading/Unloading Orchestration
- Before scan: stop KoboldCPP (frees 20GB VRAM)
- During scan: P100s run detection shaders + nauticuvs curvelets
- After scan: restart KoboldCPP with Qwen3.6-35B for result analysis
- The 1070 + P1000 + 1060 (nautivecs + thought engine) orchestrate the pipeline while P100s are in scan mode
- swap_model.sh already handles the systemd stop/start — extend it for scan mode

### R4: Drift Correction Pipeline (Xeon-based)
- Sub-pixel slice-stitch-replace algorithm runs on Xeons (not GPUs)
- Phase correlation or feature matching between temporal stack frames
- 94GB RAM holds dozens of tiles simultaneously for alignment
- Output: drift-corrected temporal stack ready for GPU anomaly detection
- Need to find existing slicing code and evaluate if it needs improvement
- Have the thinking engine (8B) + 35B analyze the algorithm and suggest improvements

### R5: Sensor Stack Integration
- Optical (HLS/Sentinel-2): PARTIAL — glint detection works
- SAR (Sentinel-1): MISSING — needs implementation
- Thermal (Landsat Band 10): STUB — needs real band ratio math
- ICESat-2: MISSING — needs photon counting implementation
- SWOT: MISSING — needs surface height extraction
- Aeromagnetic/Dipole: MISSING — needs WGSL shaders
- Weather: PARTIAL — planning works, analysis integration missing
- nauticuvs curvelets: EXISTS — needs integration into pipeline

### R6: Blind Validation Protocol
- Download 20+ days of tiles for Mackinac Straits + Lake Erie
- Run drift correction on Xeons
- Run detection on P100s (batch 1-2 tiles at a time)
- Note positions of all anomalies with confidence scores
- Web crawl known wreck databases (NOAA AWOIS, Michigan SHPO, Ohio DNR)
- Compare detections against known coordinates (500m match threshold)
- Score: precision, recall, F1
- Classify unmatched detections for ground truthing

### R7: Autonomous Operation
- Once pipeline is validated, it should run without human intervention
- Weather monitoring triggers scan acquisition
- Drift correction runs automatically on new tiles
- Detection runs in batches when P100s are available
- Results reported via Mission Control UI
- Thought engine on 1070 makes scheduling decisions

## Next Steps (For Next Session)

1. Have the 35B on T440 do a FULL codebase scan (all .rs, .py, .wgsl files)
2. Check GitHub repos under festeraeb for additional tools
3. Find the nauticuvs curvelet code and the drift correction algorithm
4. Calculate P100 VRAM budget for tile batching
5. Have the thinking engine analyze the slicing code and propose improvements
6. Spec the Xeon-based drift correction as a Rust crate using ndarray + SIMD
7. Write the orchestration logic for model swap during scan mode

## Constraints

- No Docker (native Rust/Python + systemd)
- All data on /mnt/data-external (916GB SSD, 4TB RAID arriving)
- NVIDIA 580 driver (Pascal legacy, pinned)
- Minimize Kiro token usage — use local LLM for deep dives, validate with Kiro
- Operator is dyslexic — results must be visual/simple, not text walls
