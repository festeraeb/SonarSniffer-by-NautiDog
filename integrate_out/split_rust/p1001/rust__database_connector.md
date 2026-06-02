# /codebase/projects/pipelines/utils/database_connector.py

## Verdict
PORT_TO_PIPELINES
## Rust path
cesarops-inference/src/integrate/database_connector.rs
## Rust source
```rust
//! CESAROPS Database Connector
//! Plugs cesarops_cli.py into existing LAKE_MICHIGAN_CENSUS_2026.db

use rusqlite::{Connection, params, Row, RowIterator};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Main census database (existing)
const CENSUS_DB: &str = "wreckhunter2000/LAKE_MICHIGAN_CENSUS_2026.db";

/// Alternative: cesarops_runs.db for detailed run logging
const RUNS_DB: &str = "outputs/run_zero/cesarops_runs.db";

/// Get connection to main census database
pub fn get_census_db() -> Result<Connection, Box<dyn std::error::Error>> {
    let db_path = Path::new(CENSUS_DB);
    if !db_path.exists() {
        return Err(format!("Census database not found: {}", db_path.display()).into());
    }
    let conn = Connection::open(db_path)?;
    // rusqlite automatically handles row_factory
    Ok(conn)
}

/// Get connection to detailed runs database
pub fn get_runs_db() -> Option<Connection> {
    let db_path = Path::new(RUNS_DB);
    if !db_path.exists() {
        eprintln!("Note: Runs database not found: {}", db_path.display());
        eprintln!("  Run init_database.py first or use census_db");
        return None;
    }
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to open runs database: {}", e);
            return None;
        }
    };
    Some(conn)
}

/// Log a scan run to anomaly_hits table in census DB
pub fn log_scan_run_to_census(
    run_name: &str,
    tile_count: u32,
    detection_count: u32,
    notes: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    let conn = get_census_db()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    
    let epoch_date = format!("{:04}-{:02}-{:02}", now / 31536000, (now % 31536000) / 86400, (now % 86400) / 3600);
    let now_iso = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    
    let mut stmt = conn.prepare(
        "INSERT INTO anomaly_hits (
            epoch_date, lat, lon, concept, score, classification,
            scene_id, thermal_zscore, ingested_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )?;
    
    let run_id = stmt.execute(params![
        epoch_date,
        42.5,
        -87.0,
        format!("cuda_batch_{}_tiles", tile_count),
        0.8,
        "cesarops_cuda_test",
        run_name,
        detection_count as f64,
        now_iso
    ])?;
    
    conn.commit()?;
    Ok(run_id)
}

/// Get all stationary anchors from census DB
pub fn get_stationary_anchors() -> Result<Vec<StationaryAnchor>, Box<dyn std::error::Error>> {
    let conn = get_census_db()?;
    let mut stmt = conn.prepare(
        "SELECT id, lat, lon, triple_lock_status, swot_persistent_anomaly,
               combined_score, thermal_sink_l8, sar_stability_s1
        FROM stationary_anchors
        ORDER BY id"
    )?;
    
    let rows = stmt.query_map([], |row| {
        Ok(StationaryAnchor {
            id: row.get(0)?,
            lat: row.get(1)?,
            lon: row.get(2)?,
            triple_lock_status: row.get(3)?,
            swot_persistent_anomaly: row.get(4)?,
            combined_score: row.get(5)?,
            thermal_sink_l8: row.get(6)?,
            sar_stability_s1: row.get(7)?,
        })
    })?;
    
    let anchors: Vec<StationaryAnchor> = rows.collect();
    Ok(anchors)
}

/// Get new arrivals from census DB
pub fn get_new_arrivals(limit: Option<u32>) -> Result<Vec<NewArrival>, Box<dyn std::error::Error>> {
    let conn = get_census_db()?;
    let mut stmt = conn.prepare(
        "SELECT id, lat, lon, triple_lock_status, flagged_at,
               score, priority, thermal_sink_l8, sar_stability_s1
        FROM new_arrivals
        ORDER BY id DESC"
    )?;
    
    let mut query = stmt.query_map([], |row| {
        Ok(NewArrival {
            id: row.get(0)?,
            lat: row.get(1)?,
            lon: row.get(2)?,
            triple
