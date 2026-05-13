# Intelligent Scan Orchestrator — Requirements

## Overview

Replace the current "1 bbox, 1 pass" scan queue with a decision-driven orchestrator
that behaves like an expert researcher. Instead of hard-coded pipelines, worker nodes
receive context-enriched instructions from a Tool Recipe Database that tells them
WHY they're scanning, WHAT to look for, and HOW to do it based on the target profile.

## R1: Target Profile Classification

The orchestrator MUST classify each scan target before choosing a strategy:

- **Is the vessel steel?** → prioritize magnetic/thermal signatures
- **When did it sink?** → determines which historical data sources exist
- **Is aeromagnetic survey data available for the area?** → if yes, pull and scan it first
- **Is it a recent sinking?** → before/after satellite comparison, news/social media search
- **Is it a broad area scan?** → systematic coverage with stack diversity
- **Is there flotsam/debris reported?** → drift analysis to narrow bbox

## R2: Weather-Driven Temporal Stacking (20+ days)

Every scan job MUST build a temporal stack of 20+ tiles:

- Query `get_scan_windows()` for the target bbox over 2+ years
- Select tiles across ALL weather categories (calm, post-storm, thermal contrast)
- Weight: post_storm_1 = 3×, calm = 1×, transitional = 0.5×
- Never duplicate the same weather category stack from a previous scan of the same area
- Track which dates have been scanned per bbox in the database

## R3: Multi-Source Satellite Acquisition

For each tile in the stack, attempt downloads from multiple sources in priority order:

1. **Sentinel-1 SAR** (always available, penetrates clouds)
2. **Sentinel-2 optical** (cloud-filtered, < 30% cover)
3. **Landsat 8/9** (thermal Band 10 for heat/cold sink)
4. **ICESat-2** (surface height anomalies over shallow wrecks)
5. **SWOT** (wide-swath altimetry for surface disturbance)
6. **ECOSTRESS** (thermal IR for steel heat sink detection)
7. **Aeromagnetic surveys** (NOAA/USGS historical, if available for area)

Priority depends on target profile:
- Steel vessel → thermal + magnetic first
- Recent sinking → SAR before/after + optical change detection
- Shallow water → ICESat-2 + optical sun glint

## R4: Tool Recipe Database (Dynamic Context Injection)

Instead of hard-coding scan logic, store "recipes" in a database:

- Each recipe = what to do + why + hardware constraints + success examples
- Worker nodes receive ONLY the relevant recipes for their current task
- Recipes are updated by ML training results (what worked, what didn't)
- Recipes include weather-condition-specific instructions

## R5: Scan History & Stack Diversity

The orchestrator MUST track scan history per bbox:

- Which dates have been scanned before
- Which weather conditions were represented
- What the detection results were
- On re-scan: build a DIFFERENT stack (new dates, different conditions)
- Goal: every re-scan adds new information, never repeats the same view

## R6: Idle Scan as Practice/Grading

Idle scans (background coverage) serve as training data:

- Run the full pipeline on known-wreck locations
- Compare detections against ground truth
- Grade the worker's performance (precision, recall, false positive rate)
- Feed grades back into recipe refinement
- Workers that score poorly get additional context in their recipes

## R7: Historical Research Integration

For targets with limited satellite history:

- Search newspaper archives, social media, maritime records
- Look for flotsam reports, last-known positions, distress calls
- If flotsam position is known → run drift analysis (wind + current) to estimate sinking location
- Narrow the bbox based on drift model output
- This becomes the "directed scan" input

## R8: Drift Analysis for Flotsam-Based Targeting

When debris/flotsam is reported at a known position:

- Fetch wind data (Open-Meteo) and current data (NOAA GLERL) for the time window
- Run reverse drift model: where was the object N hours/days ago?
- Generate probability heatmap of likely sinking locations
- Convert heatmap peaks to directed scan bboxes

## R9: Worker Sandbox Context

Each worker node operates in a sandbox with:

- Its hardware capabilities (from DeviceHardwareSignature)
- The top-N relevant tool recipes for its current task
- A reminder of WHY it's an expert (domain-specific system prompt)
- ML-learned rules from previous scan results
- Weather condition of the current tile being processed

## R10: Continuous Learning Loop

The system improves over time:

- Every detection is logged with: weather condition, sensor, confidence, bbox
- Known wrecks are used as positive training examples
- False positives are logged and fed back to tighten recipes
- The orchestrator learns which weather+sensor combinations work best per depth/substrate
- Band weights are adjusted based on accumulated detection statistics
