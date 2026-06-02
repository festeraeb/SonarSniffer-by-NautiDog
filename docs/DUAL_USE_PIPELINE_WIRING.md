# Dual-Use Pipeline Wiring Blueprint

Goal: one routing fabric for water and land Search and Rescue, with shipwreck and SonarSniffer lanes sharing the same orchestration model.

## 1. Core orchestration pattern

Use a single mission envelope and fan out into sensor/tool lanes:

1. Mission intake and normalization
2. Weather intelligence and condition windows
3. Data acquisition and tile/chip slicing
4. Parallel analysis lanes (satellite, magnetic, drift, bag, PDF, sonar)
5. Candidate fusion and confidence scoring
6. Mission admin UI and operator triage
7. Export mission package and field-ready map layers

Recommended orchestrator anchors already in repo:
- mission and weather: mission_control.py + weather_service.py
- satellite orchestration: pipelines/satellite/sat_mission_orchestrator.py
- magnetic orchestration: pipelines/mag/erie_central_aeromag_orchestrator.py and wrecks_api/stages/mag_pipeline_stage.py
- PDF breaker stage: wrecks_api/stages/pdf_breaker_stage.py
- fleet routing and health: scripts/fleet-route-health.sh, scripts/forge-routing-switch.sh, scripts/fleet-job-runner.sh

## 2. Mission contract (single schema for all lanes)

Mission envelope fields:
- mission_id
- domain: water or land
- objective: wreck_hunt, vessel_sar, aircraft_sar, vehicle_sar, person_search
- bbox and optional AOI polygons
- date_window and priority weather windows
- tools_enabled by lane
- output targets: db, map package, task queue, operator dashboard

Domain profile decides thresholds and models, not architecture.

## 3. Weather intelligence per tool lane

Attach weather gating at lane level so each tool receives only useful windows.

- Optical satellite lane
  - Favor calm and low cloud windows
  - Use post-storm days for plume and disturbance signals
- SAR satellite lane
  - Cloud/night tolerant
  - Use storm and immediate post-storm windows
- Thermal lane
  - Favor night windows and low humidity
- Drift lane
  - Requires wind/current history and forecast vectors
  - Always consumes weather stream and water-state feeds
- Magnetic lane
  - Mostly weather-agnostic for anomaly signal, but use weather for mission risk and sortie planning
- BAG and sonar lane
  - Use wave height/current to prioritize survey timing and confidence adjustment
- PDF breaker lane
  - No weather dependence, but mission metadata affects extraction targets

Weather source already present: weather_service.py

## 4. Satellite stack wiring: download, slice, analyze, stitch, compare

Canonical flow:
1. Download scene inventory and selected granules
2. Slice scenes into mission chips (with overlap)
3. Analyze each chip independently per tool concept
4. Compare chips across temporal windows
5. Stitch only where needed for operator context, not for heavy compute
6. Reproject detections to original scene coordinates
7. Emit candidate list with source-chip provenance

Implementation anchors:
- scripts/production_satellite_downloader.py
- scripts/run_great_lakes_satellite.sh
- scripts/resume_satellite_pipeline.sh
- pipelines/satellite/sat_mission_orchestrator.py
- cesarops-inference/src/satellite_stitch.rs

Rule: treat slicing as the primary compute unit; stitching is a visualization and QA unit.

## 5. Magnetic stack wiring (same routing pattern)

Canonical flow mirrors satellite:
1. Ingest source grids and normalize
2. Slice to analysis windows
3. Run anomaly and dipole passes
4. Compare with known clutter signatures
5. Stitch context surfaces for human review
6. Emit ranked candidates + confidence + supporting artifacts

Implementation anchors:
- wrecks_api/stages/mag_pipeline_stage.py
- pipelines/mag/mag_data_pipeline.py
- pipelines/mag/final_magnetic_fusion.py
- cesarops-inference/src/magnetic_eraser.rs

## 6. BAG masked-artifact recovery wiring

Canonical flow:
1. Ingest BAG and metadata
2. Recover masked/occluded regions
3. Generate cleaned surfaces and anomaly tiles
4. Run detector and confidence pass
5. Output candidate geometries and review package

Implementation anchors:
- pipelines/bag/advanced_bag_scanner.py
- pipelines/bag/advanced_bag_scanner_runner.py
- pipelines/bag/bag_wreck_detector.py

## 7. PDF redaction-breaker wiring

Canonical flow:
1. Ingest mission PDFs and report corpus
2. Run redaction-breaker extraction
3. Normalize findings into mission entities (coords, names, vessels, incidents)
4. Route entities into candidate fusion graph

Implementation anchor:
- wrecks_api/stages/pdf_breaker_stage.py

## 8. Drift analysis wiring

Canonical flow:
1. Build environmental timeline (wind, water level, current)
2. Generate forward and backward drift envelopes
3. Intersect with sensor detections from satellite/mag/BAG/sonar
4. Raise candidates where drift-consistent evidence overlaps

Existing data hints:
- outputs/historic_news/drift_candidates.json
- scripts/reestimate_from_historic_news.py

## 9. Candidate fusion and ranking

All lanes emit a common candidate object:
- candidate_id
- geometry (point/polygon)
- source_lane and source_artifact
- confidence_raw
- confidence_calibrated
- environmental_context
- mission_relevance
- explainability notes

Fusion policy:
- High confidence: multi-lane agreement with temporal consistency
- Medium confidence: single strong lane + drift or metadata support
- Low confidence: weak signal or single-lane outlier

## 10. Frontend admin tools for SAR operations

Admin panel should expose:
- mission queue and lane health
- live worker routing and endpoint health
- candidate map with evidence layers
- operator actions: accept, reject, request rescan, request tighter AOI
- incident package export for responders

Routing/ops anchors:
- scripts/fleet-route-health.sh
- scripts/forge-health-probe.sh
- scripts/forge-health-recover.sh
- docs/FLEET_WIRING_PLAYBOOK.md

## 11. Water-first, land-second adaptation model

Keep same lanes and contracts. Only swap profile packs:

Water profile examples:
- objectives: wreck_hunt, vessel_sar
- stronger plume, bathymetry, wave/current weighting
- sonar and BAG lanes prioritized

Land profile examples:
- objectives: aircraft_sar, vehicle_sar, person_search
- stronger canopy, thermal, terrain and road-access weighting
- magnetic lane for metal wreck signatures, optical/SAR for clearing anomalies

This means no rewrite: same orchestrator, same fusion API, different mission profile presets.

## 12. Wiring sequence you can execute now

1. Bring up fleet endpoints and routing
2. Activate n8n and fleet health workflows
3. Run mission intake through mission_control.py
4. Dispatch satellite and magnetic orchestrators from one mission envelope
5. Run BAG and PDF stages into same candidate store
6. Execute fusion ranking and publish admin dashboard view
7. Export SAR action package

Use docs/FLEET_WIRING_PLAYBOOK.md for infra bring-up, then this document for pipeline logic wiring.
