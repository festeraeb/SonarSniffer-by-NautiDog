# Mission Routing: Natural Language → Autonomous Search Pipeline

## Overview

A user describes a search mission in plain English. The system:
1. Extracts parameters (location, date, target type, constraints)
2. Asks clarifying questions if needed
3. Selects detection tools based on target characteristics
4. Inventories available imagery and hardware
5. Schedules downloads (the bottleneck)
6. Assigns GPU workers to process as data arrives
7. Reports findings with confidence scores and coordinates

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    USER INPUT (plain English)                     │
│  "An airplane reported overdue, last known north of Anchorage    │
│   Alaska on May 10th 2026"                                       │
└──────────────────────────────┬──────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                   MISSION PLANNER (LLM)                           │
│  - Extract: target=aircraft, location=N of Anchorage, date=5/10  │
│  - Ask: terrain? (mountain/water/forest), color? size?           │
│  - Decide: which sensors/tools apply                             │
└──────────────────────────────┬──────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                   TOOL SELECTOR                                   │
│  Based on target + terrain + conditions:                         │
│  ┌─────────────┐ ┌──────────────┐ ┌────────────────┐           │
│  │ Glint (NIR) │ │ Spectral     │ │ Change Detect  │           │
│  │ windshield  │ │ paint color  │ │ before/after   │           │
│  └─────────────┘ └──────────────┘ └────────────────┘           │
│  ┌─────────────┐ ┌──────────────┐ ┌────────────────┐           │
│  │ LiDAR       │ │ SAR (cloud   │ │ Thermal (SWIR) │           │
│  │ if avail    │ │ penetrating) │ │ engine heat    │           │
│  └─────────────┘ └──────────────┘ └────────────────┘           │
└──────────────────────────────┬──────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                   DATA INVENTORY                                  │
│  1. Check local DB: any tiles covering bbox already cached?      │
│  2. Query sources:                                               │
│     - NASA HLS (Sentinel-2/Landsat) via CMR API                  │
│     - ESA Copernicus (Sentinel-1 SAR)                            │
│     - USGS EarthExplorer                                         │
│     - OpenTopography (LiDAR)                                     │
│     - Planet Labs (if API key available)                          │
│  3. Filter by: date range, cloud cover, bbox overlap             │
│  4. Rank by: temporal proximity to event, resolution, bands      │
└──────────────────────────────┬──────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                   HARDWARE INVENTORY                              │
│  Poll cluster: what's available right now?                        │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐          │
│  │ P100 #0  │ │ P100 #1  │ │ 1070     │ │ 1060     │          │
│  │ 16GB     │ │ 16GB     │ │ 8GB      │ │ 6GB      │          │
│  │ IDLE     │ │ LLM      │ │ IDLE     │ │ IDLE     │          │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘          │
│  Available VRAM: 30GB (P100#0 + 1070 + 1060)                    │
│  Tiles that fit without slicing: ~13 at 2.3GB each              │
└──────────────────────────────┬──────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                   DOWNLOAD SCHEDULER                              │
│  The bottleneck. Manages concurrent downloads.                   │
│                                                                   │
│  Strategy:                                                        │
│  - Start downloading highest-priority tiles first                │
│  - As each tile completes → immediately dispatch to GPU          │
│  - Don't wait for all downloads to finish                        │
│  - Pipeline: download[n+1] while processing[n]                   │
│                                                                   │
│  Queue:                                                           │
│  [tile_1: downloading 45%] [tile_2: queued] [tile_3: queued]    │
│  [tile_4: DONE → dispatched to P100#0]                           │
│  [tile_5: DONE → dispatched to 1070]                             │
└──────────────────────────────┬──────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                   WORKER SWARM                                    │
│  Each GPU runs its assigned analysis pass:                        │
│                                                                   │
│  P100 #0: temporal_stack(tiles[0..10]) → anomaly_map_A           │
│  1070:    glint_detection(tile_latest) → glint_hits              │
│  1060:    spectral_filter(tile_latest, bands=[NIR,SWIR,RED])     │
│                                                                   │
│  Results flow back to orchestrator as they complete.              │
│  Cross-verification between independent analyses.                │
└──────────────────────────────┬──────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                   RESULTS FUSION & REPORTING                      │
│  - Merge hits from all analysis passes                           │
│  - Cross-verify: if glint + spectral + change all agree → HIGH  │
│  - Geocode: pixel coords → lat/lon via GeoTransform              │
│  - Generate report with confidence scores                        │
│  - Plot on map (leaflet/mapbox in web UI)                        │
│  - Store in scan database for future reference                   │
└─────────────────────────────────────────────────────────────────┘
```

## Mission Planner LLM

The planner is a specialized prompt that runs on the main LLM (Qwen on P100s or the bootstrap brain). It extracts structured mission parameters from natural language.

### Input Schema (what the LLM produces)

```json
{
  "mission_type": "aircraft_search",
  "target": {
    "type": "fixed_wing_aircraft",
    "size_estimate_m": 15,
    "color": "white_with_red_stripe",
    "material": "aluminum"
  },
  "location": {
    "description": "north of Anchorage Alaska",
    "bbox": [-150.5, 61.2, -149.0, 62.5],
    "terrain": "mountainous_forest",
    "elevation_range_m": [0, 3000]
  },
  "temporal": {
    "event_date": "2026-05-10",
    "search_window_days_before": 3,
    "search_window_days_after": 5
  },
  "conditions": {
    "weather_at_event": "overcast",
    "season": "spring",
    "snow_cover_likely": true
  },
  "tools_selected": [
    {"tool": "glint_detection", "reason": "windshield/metal specular reflection in NIR"},
    {"tool": "spectral_paint", "reason": "white+red paint signature in visible bands"},
    {"tool": "change_detection", "reason": "compare pre/post event for new anomalies"},
    {"tool": "sar_coherence", "reason": "cloud-penetrating, works in overcast"},
    {"tool": "thermal_swir", "reason": "engine/fuel residual heat if recent"}
  ],
  "data_sources": [
    {"source": "sentinel2_hls", "priority": 1, "reason": "10m optical, good for paint/glint"},
    {"source": "sentinel1_sar", "priority": 2, "reason": "cloud penetrating, terrain"},
    {"source": "landsat9", "priority": 3, "reason": "thermal band for heat signature"}
  ]
}
```

### Clarifying Questions

If the LLM can't determine key parameters, it asks:

- "What color is the aircraft? This helps with spectral filtering."
- "Was it over water or land? This changes which detection tools apply."
- "Do you have a more specific last-known position or heading?"
- "What time of day was last contact? This affects thermal detection viability."
- "Any ELT (emergency locator transmitter) signal received? This narrows the bbox."

## Tool Registry

Each detection tool has a profile:

```rust
pub struct DetectionTool {
    pub name: &'static str,
    pub description: &'static str,
    pub applicable_targets: Vec<TargetType>,
    pub applicable_terrain: Vec<TerrainType>,
    pub required_bands: Vec<Band>,
    pub min_resolution_m: f32,
    pub works_in_cloud: bool,
    pub works_at_night: bool,
    pub gpu_vram_mb: u32,        // VRAM needed per tile
    pub processing_time_s: f32,  // estimated per tile
}

// Registry
const TOOLS: &[DetectionTool] = &[
    DetectionTool {
        name: "glint_detection",
        description: "Specular reflection from glass/metal surfaces",
        applicable_targets: vec![Aircraft, Vehicle, Vessel],
        applicable_terrain: vec![Water, Forest, Mountain, Desert],
        required_bands: vec![NIR, SWIR1],
        min_resolution_m: 10.0,
        works_in_cloud: false,
        works_at_night: false,
        gpu_vram_mb: 500,
        processing_time_s: 2.0,
    },
    DetectionTool {
        name: "spectral_paint",
        description: "Color signature matching in visible/NIR bands",
        applicable_targets: vec![Aircraft, Vehicle, Vessel, Structure],
        applicable_terrain: vec![Water, Forest, Mountain, Desert, Urban],
        required_bands: vec![Red, Green, Blue, NIR],
        min_resolution_m: 10.0,
        works_in_cloud: false,
        works_at_night: false,
        gpu_vram_mb: 800,
        processing_time_s: 3.0,
    },
    DetectionTool {
        name: "change_detection",
        description: "Before/after comparison for new anomalies",
        applicable_targets: vec![Aircraft, Vehicle, Vessel, Debris],
        applicable_terrain: vec![Any],
        required_bands: vec![Any],
        min_resolution_m: 30.0,
        works_in_cloud: false,
        works_at_night: false,
        gpu_vram_mb: 2000, // needs 2 tiles in memory
        processing_time_s: 5.0,
    },
    DetectionTool {
        name: "sar_coherence",
        description: "SAR interferometric coherence loss detection",
        applicable_targets: vec![Aircraft, Vessel, Structure, Debris],
        applicable_terrain: vec![Any],
        required_bands: vec![SAR_VV, SAR_VH],
        min_resolution_m: 20.0,
        works_in_cloud: true,
        works_at_night: true,
        gpu_vram_mb: 1500,
        processing_time_s: 8.0,
    },
    DetectionTool {
        name: "thermal_swir",
        description: "Thermal anomaly detection in SWIR bands",
        applicable_targets: vec![Aircraft, Vehicle, Fire],
        applicable_terrain: vec![Forest, Mountain, Desert],
        required_bands: vec![SWIR1, SWIR2, TIR],
        min_resolution_m: 30.0,
        works_in_cloud: false,
        works_at_night: true,
        gpu_vram_mb: 400,
        processing_time_s: 1.5,
    },
    DetectionTool {
        name: "lidar_canopy_penetration",
        description: "LiDAR point cloud analysis for objects under tree canopy",
        applicable_targets: vec![Aircraft, Vehicle],
        applicable_terrain: vec![Forest],
        required_bands: vec![LiDAR],
        min_resolution_m: 1.0,
        works_in_cloud: false,
        works_at_night: false,
        gpu_vram_mb: 3000,
        processing_time_s: 15.0,
    },
];
```

## Download Scheduler

The download scheduler is the critical path. It manages:

```rust
pub struct DownloadScheduler {
    /// Active downloads (max concurrent based on bandwidth)
    active: Vec<DownloadTask>,
    /// Queued downloads sorted by priority
    queue: BinaryHeap<DownloadTask>,
    /// Completed downloads ready for processing
    ready: Vec<CompletedTile>,
    /// Max concurrent downloads (auto-tuned based on bandwidth)
    max_concurrent: usize,
    /// Bandwidth estimate (bytes/sec, rolling average)
    bandwidth_estimate: f64,
}

pub struct DownloadTask {
    pub source: DataSource,
    pub tile_id: String,
    pub url: String,
    pub bbox: BBox,
    pub priority: u32,       // lower = higher priority
    pub size_estimate_mb: f32,
    pub bands: Vec<Band>,
    pub status: DownloadStatus,
}

impl DownloadScheduler {
    /// Called every tick. Starts new downloads, checks completions.
    pub async fn tick(&mut self) -> Vec<CompletedTile> {
        // 1. Check for completed downloads → move to ready queue
        // 2. If active < max_concurrent, pop from queue and start
        // 3. Update bandwidth estimate from recent completions
        // 4. Return newly completed tiles for immediate processing
    }

    /// Priority scoring: temporal proximity × resolution × band match
    fn score_priority(task: &DownloadTask, mission: &MissionParams) -> u32 {
        let temporal_score = days_from_event(task, mission); // closer = higher
        let resolution_score = 100 - (task.resolution_m as u32).min(100);
        let band_match = count_matching_bands(task, mission);
        temporal_score * 10 + resolution_score + band_match * 20
    }
}
```

## Worker Assignment

As tiles complete downloading, they're assigned to available GPUs:

```rust
pub struct WorkerPool {
    pub workers: Vec<GpuWorker>,
}

pub struct GpuWorker {
    pub gpu_id: usize,
    pub name: String,
    pub vram_total_mb: u32,
    pub vram_used_mb: u32,
    pub current_task: Option<AnalysisTask>,
    pub endpoint: String,  // for remote nodes
}

impl WorkerPool {
    /// Assign a completed tile to the best available worker
    pub fn assign(&mut self, tile: CompletedTile, tool: &DetectionTool) -> Option<usize> {
        // Find a worker with enough free VRAM
        let needed = tool.gpu_vram_mb;
        self.workers.iter_mut()
            .enumerate()
            .filter(|(_, w)| w.current_task.is_none())
            .filter(|(_, w)| (w.vram_total_mb - w.vram_used_mb) >= needed)
            .min_by_key(|(_, w)| w.vram_used_mb) // prefer emptiest GPU
            .map(|(idx, worker)| {
                worker.current_task = Some(AnalysisTask {
                    tile: tile.clone(),
                    tool: tool.name.to_string(),
                    started: Instant::now(),
                });
                worker.vram_used_mb += needed;
                idx
            })
    }
}
```

## Pipeline Flow (streaming)

The key insight: don't wait for all downloads. Process as data arrives.

```
Time →
Download:  [tile1████████] [tile2████████] [tile3████████] [tile4████]
GPU 0:          [analyze tile1] [analyze tile3]
GPU 1:               [analyze tile2]     [analyze tile4]
Results:              [hit!]                    [hit!]
                         ↓                        ↓
                    [cross-verify: both hit same region → HIGH CONFIDENCE]
```

## Integration Points

### With Forge v2 (Web UI)
- Mission input via chat interface
- Real-time progress display (downloads, processing, hits)
- Map visualization of results
- STOP/STEER buttons for operator control

### With Agent Dispatch
- Mission planner runs as an agent task
- Each GPU worker can be in agent mode (safe mode for untrusted models)
- Results feed back through the agent loop

### With Cluster Panel
- Hardware inventory reads from `/cluster/discover`
- Worker assignment uses the same GPU status from `/monitor`
- CESAROPS preset configures the default tool allocation

### With n8n
- Mission can be triggered via n8n webhook
- Download scheduling can be a visual workflow
- Results can trigger notifications/alerts

## API Endpoints (to add to forge-v2)

```
POST /mission/plan     — Submit natural language mission, get structured plan
POST /mission/launch   — Execute a planned mission
GET  /mission/status   — Current mission progress (downloads, processing, hits)
POST /mission/stop     — Abort current mission
GET  /mission/results  — Get all hits from current/past missions
```

## Example Flow

```
User: "An airplane reported overdue, last known north of Anchorage 
       Alaska on May 10th. White Cessna 172 with red stripe."

Planner: {extracts params, selects tools}
  → target: Cessna 172, white+red, aluminum, ~8m wingspan
  → bbox: [-150.5, 61.2, -149.0, 62.5]
  → tools: glint (windshield), spectral_paint (white+red), 
            change_detection, sar_coherence (overcast likely)
  → sources: Sentinel-2 (priority 1), Sentinel-1 SAR (priority 2)

Inventory: 
  → Local DB: 0 tiles in bbox (never scanned Alaska before)
  → CMR API: 4 Sentinel-2 tiles available (May 8, 9, 11, 12)
  → ASF DAAC: 2 Sentinel-1 passes (May 9, 11)
  → Hardware: P100#0 free, 1070 free, 1060 free

Download Schedule:
  → [Priority 1] S2 May 11 (day after event) — downloading
  → [Priority 2] S2 May 8 (baseline before event) — queued
  → [Priority 3] S1 May 11 (SAR, cloud-penetrating) — queued
  → [Priority 4] S2 May 12 (2 days after) — queued

Processing (as downloads complete):
  → S2 May 11 arrives → P100#0: glint + spectral_paint
  → S2 May 8 arrives → 1070: hold for change_detection pair
  → Both ready → P100#0: change_detection(May8 vs May11)
  → S1 May 11 arrives → 1060: sar_coherence

Results:
  → Glint hit at (61.847, -149.723) — confidence 0.72
  → Spectral match at (61.845, -149.725) — confidence 0.68
  → Change detection anomaly at (61.846, -149.724) — confidence 0.81
  → SAR coherence loss at (61.847, -149.723) — confidence 0.55

Cross-verification:
  → 4/4 tools agree within 200m radius → CONFIDENCE: VERY HIGH
  → Coordinates: 61.846°N, 149.724°W
  → Terrain: forested slope, 1200m elevation
  → Recommendation: Deploy ground team to coordinates
```
