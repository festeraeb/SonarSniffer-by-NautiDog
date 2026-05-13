# Intelligent Scan Orchestrator — Design

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                    SCAN REQUEST (bbox + target info)             │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                  TARGET PROFILE CLASSIFIER                        │
│  • Steel? Sinking date? Depth? Known position?                   │
│  • Outputs: ScanProfile enum + priority sensors                  │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                  TEMPORAL STACK BUILDER                           │
│  • Queries weather service for 2yr window                        │
│  • Checks scan_history DB for previously scanned dates           │
│  • Selects 20+ NEW dates across weather categories               │
│  • Outputs: Vec<StackEntry> with date + condition + weight       │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                  ACQUISITION PLANNER                              │
│  • For each date in stack: which sensors are available?           │
│  • Queries STAC catalogs (Sentinel, Landsat, ICESat, SWOT)       │
│  • Checks aeromagnetic survey coverage                           │
│  • Outputs: Vec<AcquisitionPlan> with sensor + URL + priority    │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                  TOOL RECIPE SELECTOR                             │
│  • Queries ToolRecipe DB with target profile + sensor type       │
│  • Returns top-3 recipes with hardware guards + examples         │
│  • Compiles worker sandbox prompt                                │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                  WORKER DISPATCH                                  │
│  • Routes to correct hardware (P100 for parallel, Xeon for f64)  │
│  • Injects: recipes + weather context + scan history             │
│  • Worker runs detection pipeline in sandbox                     │
│  • Returns: detections + confidence + metadata                   │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                  RESULT AGGREGATOR + LEARNING                     │
│  • Merges detections across all dates in stack                   │
│  • Weights by weather condition (post_storm_1 = 3×)              │
│  • Compares against known wrecks (grading)                       │
│  • Updates scan_history DB                                       │
│  • Feeds back to recipe DB (what worked)                         │
└─────────────────────────────────────────────────────────────────┘
```

## Database Schema

### scan_targets (what we're looking for)

```sql
CREATE TABLE scan_targets (
    id TEXT PRIMARY KEY,
    name TEXT,
    vessel_type TEXT,          -- 'steel_freighter', 'wooden_schooner', 'barge', 'unknown'
    is_steel BOOLEAN,
    sinking_date TEXT,         -- ISO date or NULL if unknown
    sinking_era TEXT,          -- 'pre_1900', '1900_1950', '1950_2000', 'post_2000'
    last_known_lat REAL,
    last_known_lon REAL,
    search_bbox TEXT,          -- JSON [lat_min, lon_min, lat_max, lon_max]
    depth_estimate_m REAL,
    has_aeromagnetic BOOLEAN,
    flotsam_reported BOOLEAN,
    flotsam_position TEXT,     -- JSON {lat, lon, date, source}
    profile TEXT,              -- computed: 'steel_magnetic', 'shallow_optical', 'recent_change', 'deep_sar'
    created_at TEXT,
    updated_at TEXT
);
```

### scan_history (what we've already looked at)

```sql
CREATE TABLE scan_history (
    id TEXT PRIMARY KEY,
    target_id TEXT REFERENCES scan_targets(id),
    bbox TEXT,                 -- JSON
    scan_date TEXT,            -- date of the satellite tile
    weather_condition TEXT,    -- calm, post_storm_1, thermal_contrast, etc.
    sensor TEXT,               -- sentinel_1_sar, sentinel_2_optical, landsat_thermal, etc.
    source_url TEXT,           -- where the tile was downloaded from
    cloud_cover_pct REAL,
    wind_speed_mph REAL,
    water_level_departure_ft REAL,
    detection_count INTEGER,
    max_confidence REAL,
    scanned_at TEXT,           -- when we processed it
    stack_id TEXT              -- groups tiles that were processed together
);
```

### scan_stacks (temporal groupings)

```sql
CREATE TABLE scan_stacks (
    id TEXT PRIMARY KEY,
    target_id TEXT REFERENCES scan_targets(id),
    bbox TEXT,
    created_at TEXT,
    tile_count INTEGER,
    weather_distribution TEXT, -- JSON {calm: 5, post_storm_1: 3, ...}
    detection_summary TEXT,    -- JSON aggregate results
    grade REAL,                -- 0-1 score if graded against known wreck
    notes TEXT
);
```

### tool_recipes (dynamic context for workers)

```sql
CREATE TABLE tool_recipes (
    id TEXT PRIMARY KEY,
    tool_name TEXT,
    native_schema TEXT,
    semantic_anchor TEXT,
    execution_example TEXT,
    hardware_guard TEXT,
    valid_targets TEXT,        -- JSON array
    tags TEXT,                 -- JSON array
    success_count INTEGER DEFAULT 0,
    failure_count INTEGER DEFAULT 0,
    avg_latency_ms INTEGER DEFAULT 0,
    last_used TEXT,
    learned_notes TEXT         -- ML-derived improvements
);
```

### idle_grades (practice scoring)

```sql
CREATE TABLE idle_grades (
    id TEXT PRIMARY KEY,
    known_wreck_id TEXT,
    stack_id TEXT REFERENCES scan_stacks(id),
    detected BOOLEAN,
    confidence REAL,
    position_error_m REAL,    -- distance from known position
    false_positives INTEGER,
    grade REAL,               -- computed score
    graded_at TEXT,
    feedback TEXT              -- what went wrong / right
);
```

## Target Profile Decision Tree

```
INPUT: target metadata
  │
  ├─ is_steel = true?
  │   ├─ YES → profile = 'steel_magnetic'
  │   │   ├─ has_aeromagnetic = true? → PRIORITY: pull aeromagnetic first
  │   │   ├─ depth < 20m? → ADD: thermal IR (heat sink), sun glint
  │   │   └─ depth > 20m? → ADD: SAR texture, magnetic anomaly only
  │   └─ NO → profile = 'non_magnetic'
  │       ├─ depth < 10m? → PRIORITY: optical (hull shadow), ICESat-2
  │       └─ depth > 10m? → PRIORITY: SAR (surface disturbance), sonar if available
  │
  ├─ sinking_era = 'post_2000'?
  │   ├─ YES → profile += '_recent'
  │   │   ├─ PRIORITY: before/after change detection (SAR + optical)
  │   │   ├─ ADD: news/social media search for reports
  │   │   └─ ADD: AIS data for last known track
  │   └─ NO → profile += '_historical'
  │       └─ PRIORITY: temporal stacking (20+ dates), weather diversity
  │
  ├─ flotsam_reported = true?
  │   └─ YES → RUN drift analysis FIRST
  │       ├─ Narrow bbox from drift model
  │       └─ Then proceed with narrowed bbox
  │
  └─ broad_area_scan = true? (bbox > 0.5° × 0.5°)
      └─ YES → SPLIT into 0.1° tiles
          ├─ Check scan_history for each tile
          ├─ Prioritize tiles with fewest previous scans
          └─ Build DIFFERENT stack than last time (new dates)
```

## Sensor Priority by Profile

| Profile | Priority 1 | Priority 2 | Priority 3 | Priority 4 |
|---------|-----------|-----------|-----------|-----------|
| steel_magnetic_shallow | Aeromagnetic | Thermal IR (Landsat B10) | SAR texture | Optical sun glint |
| steel_magnetic_deep | Aeromagnetic | SAR texture | Sentinel-1 coherence | — |
| non_magnetic_shallow | Optical (S2) | ICESat-2 | Sun glint | SAR |
| recent_change | SAR before/after | Optical before/after | News search | AIS track |
| broad_coverage | SAR (always works) | Optical (clear days) | Thermal (contrast days) | — |

## Weather-to-Sensor Mapping

| Weather Condition | Best Sensors | Why |
|-------------------|-------------|-----|
| calm | Optical, ICESat-2, SWOT, sun glint | Clear water, no wave distortion |
| post_storm_1 | Optical (plume), SAR (texture change) | Sediment stirred up, visible plume |
| post_storm_2 | SAR (displacement), optical (dispersing plume) | Flow patterns visible |
| post_storm_3 | Thermal (mixing anomaly), optical (settling) | Temperature differential |
| thermal_contrast | Landsat B10, ECOSTRESS | Steel heat sink visible |
| low_water (seiche) | Optical, ICESat-2 | Wreck may be partially exposed |

## Implementation Language

- **Orchestrator logic**: Rust (in cesarops-hybrid-engine or new crate)
- **Database**: SQLite (same pattern as existing scan_queue.db)
- **Weather integration**: Call existing weather_service.py via HTTP or rewrite in Rust
- **STAC queries**: Use existing sentinel_hunt_src/src/stac.rs
- **Worker dispatch**: Extend existing scan_worker.py or rewrite in Rust
- **Tool recipes**: Already built in cesarops-hybrid-engine/src/tool_db.rs
