use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::api::{GridCell, IdleMode, NodeState, ScanHit};

const IDLE_INTERVAL_SECS: u64 = 300; // 5-minute scout cycle
const SCOUT_BATCH_SIZE: usize = 5;
const ALERT_CONFIDENCE_THRESHOLD: f32 = 0.65;
const HIT_MIN_CONFIDENCE: f32 = 0.20; // store hits above this for the map

// ── Great Lakes scan grid ─────────────────────────────────────────────────────
// ~40 priority cells covering known wreck hotspots and systematic coverage.
// Bands are Landsat 8/9 HLS typical surface-reflectance defaults used when
// no real satellite data is available (idle baseline scan).
const GREAT_LAKES_GRID: &[(&str, f64, f64)] = &[
    // Straits of Mackinac (high density)
    ("Mackinac Straits - East",    45.82, -84.57),
    ("Mackinac Straits - Center",  45.85, -84.72),
    ("Mackinac Straits - West",    45.78, -84.90),
    // Lake Huron - Thunder Bay (US National Marine Sanctuary)
    ("Thunder Bay - North",        45.20, -83.40),
    ("Thunder Bay - Center",       45.03, -83.25),
    ("Thunder Bay - South",        44.85, -83.10),
    // Lake Superior - Whitefish Point
    ("Whitefish Point",            46.77, -84.97),
    ("Pictured Rocks Offshore",    46.55, -86.40),
    ("Keweenaw Passage",           47.12, -88.05),
    ("Duluth Harbor Approaches",   46.78, -92.00),
    // Lake Michigan - Chicago-Milwaukee shipping lane
    ("Chicago Approaches",         42.00, -87.35),
    ("Milwaukee Offshore",         43.10, -87.80),
    ("Green Bay Entrance",         44.65, -87.45),
    ("Door Peninsula West",        45.00, -87.10),
    ("Ludington Offshore",         43.95, -86.60),
    ("Sleeping Bear Offshore",     44.85, -86.05),
    // Lake Erie - shallow, many wrecks
    ("Erie - Presque Isle",        42.15, -80.07),
    ("Erie - Cleveland Offshore",  41.65, -81.85),
    ("Erie - Toledo Approaches",   41.60, -83.20),
    ("Erie - Long Point",          42.55, -80.05),
    ("Erie - Eastern Basin",       42.70, -79.35),
    ("Niagara Mouth",              43.25, -79.05),
    // Lake Ontario
    ("Ontario - Toronto Island",   43.62, -79.38),
    ("Ontario - Kingston Basin",   44.20, -76.55),
    ("Ontario - Rochester Offshore",43.35, -77.60),
    ("Ontario - Oswego Offshore",  43.45, -76.52),
    // Lake Huron - Georgian Bay
    ("Georgian Bay North",         45.30, -80.35),
    ("Tobermory Fathom Five",      45.22, -81.65),
    ("Harbour Island",             45.78, -81.55),
    // Lake Superior - eastern basin
    ("Sault Ste. Marie Approaches",46.48, -84.35),
    ("Caribou Island",             47.38, -85.80),
    ("Michipicoten Offshore",      47.72, -85.05),
    // Lake Michigan - northern
    ("Beaver Island",              45.65, -85.52),
    ("Charlevois Offshore",        45.30, -85.27),
    ("Grand Traverse Bay",         44.95, -85.62),
    // Lake Erie - deep central trench
    ("Erie - Central Trench",      42.18, -81.35),
    ("Erie - Middle Bass Island",  41.68, -82.82),
    // Additional Huron points
    ("Alpena Offshore",            45.08, -83.48),
    ("Saginaw Bay Entrance",       43.97, -83.82),
    ("Lake Huron Mid-Lake",        44.50, -82.40),
];

// Synthetic band baseline (Landsat 8/9 HLS: Blue,Green,Red,NIR,SWIR1,SWIR2,TIR)
// Represents typical open water with minor sun glint
const BASELINE_BANDS: [f32; 7] = [0.065, 0.058, 0.034, 0.018, 0.006, 0.005, 0.115];

// ── Spawn ─────────────────────────────────────────────────────────────────────

pub fn spawn(state: Arc<NodeState>) {
    tokio::spawn(run_idle_loop(state));
}

async fn run_idle_loop(state: Arc<NodeState>) {
    info!("IdleScout: background task started (interval={}s)", IDLE_INTERVAL_SECS);
    let mut grid_idx: usize = 0;

    loop {
        let laptop = *state.laptop_mode.read().await;
        let mode = state.idle_mode.read().await.clone();

        if laptop {
            // Laptop mode — sleep longer, do nothing
            sleep(Duration::from_secs(60)).await;
            continue;
        }

        // ── Historical tile rescan (always when node has no active tasks) ─────
        if state.pipeline.active_task_count().await == 0 {
            run_historical_batch(&state).await;
        }

        // ── Great Lakes grid scan (when mode includes Scan) ───────────────────
        if matches!(mode, IdleMode::Scan | IdleMode::Both) {
            run_grid_batch(&state, &mut grid_idx).await;
        }

        // ── Autonomous Research (when mode includes Research) ─────────────────
        if matches!(mode, IdleMode::Research | IdleMode::Both) {
            if let Some(ref research) = state.research {
                if state.pipeline.active_task_count().await == 0 {
                    info!("IdleScout: starting research cycle");
                    let _ = research.run_cycle().await;
                }
            }
        }

        sleep(Duration::from_secs(IDLE_INTERVAL_SECS)).await;
    }
}

async fn run_historical_batch(_state: &Arc<NodeState>) {
    // Re-scout tiles already in the store (existing behaviour, unchanged)
    // In a real deployment these would come from state.pipeline's tile store;
    // here we just log that the pass ran.
    info!("IdleScout: historical tile rescan pass (node idle)");
}

async fn run_grid_batch(state: &Arc<NodeState>, grid_idx: &mut usize) {
    info!("IdleScout: Great Lakes grid scan — starting batch of {}", SCOUT_BATCH_SIZE);

    for _ in 0..SCOUT_BATCH_SIZE {
        if state.pipeline.active_task_count().await > 0 {
            info!("IdleScout: node became busy — pausing grid scan");
            break;
        }

        let (label, lat, lon) = GREAT_LAKES_GRID[*grid_idx % GREAT_LAKES_GRID.len()];
        *grid_idx = (*grid_idx + 1) % GREAT_LAKES_GRID.len();

        let cell = GridCell { lat, lon, label: label.to_string() };
        *state.current_cell.write().await = Some(cell.clone());
        info!("IdleScout: scanning grid cell '{}' ({:.3}, {:.3})", label, lat, lon);

        let bands = BASELINE_BANDS.to_vec();
        match state.pipeline.fire_full_pipeline(lat, lon, bands).await {
            Ok(results) => {
                for result in &results {
                    if result.anomaly_confidence >= HIT_MIN_CONFIDENCE {
                        let hit = ScanHit {
                            lat,
                            lon,
                            confidence: result.anomaly_confidence,
                            pass: result.pass.clone(),
                            cell_label: label.to_string(),
                            timestamp_ms: chrono::Utc::now().timestamp_millis(),
                        };
                        if result.anomaly_confidence >= ALERT_CONFIDENCE_THRESHOLD {
                            info!(
                                "IdleScout: HIGH-CONF hit at '{}' ({:.3},{:.3}) conf={:.2} pass={}",
                                label, lat, lon, result.anomaly_confidence, result.pass
                            );
                        }
                        state.push_hit(hit).await;
                    }
                }
            }
            Err(e) => warn!("IdleScout: pipeline error for '{}': {}", label, e),
        }

        sleep(Duration::from_secs(5)).await;
    }

    info!("IdleScout: grid batch complete (next idx={})", grid_idx);
    // Clear current cell when batch is done
    *state.current_cell.write().await = None;
}
