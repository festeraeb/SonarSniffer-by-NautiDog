# integrate/unmapped/laptopdump_wreckhunter_build/populate_database.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/lake_michigan/populate_database.rs

## Rust source
```rust
//! Database population from processing results
//!
//! This module provides functionality to load JSON processing results
//! into a SQLite database for the Wreckhunter Lake Michigan census.
//!
//! Usage:
//!     cargo run --bin populate-database -- <results.json>

use chrono::{DateTime, Utc};
use rusqlite::{Connection, Result as SqliteResult};
use serde_json::Value;
use std::path::Path;
use std::fs;

/// Database path for the Lake Michigan census
pub const DB_PATH: &str = "wreckhunter2000/LAKE_MICHIGAN_CENSUS_2026.db";

/// Initialize database schema if needed
pub fn init_db(conn: &Connection) -> SqliteResult<()> {
    // Check if anomaly_hits table exists
    let exists = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='anomaly_hits'",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .map(|name| name.is_some())?;

    if !exists {
        log::info!("Creating database schema...");

        conn.execute_batch(
            "
            -- Anomaly hits (all detections)
            CREATE TABLE anomaly_hits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tile_path TEXT,
                sensor_type TEXT,
                zscore REAL,
                anomaly_count INTEGER,
                lat REAL,
                lon REAL,
                detected_at TEXT
            );

            -- Stationary anchors (repeatable detections)
            CREATE TABLE stationary_anchors (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                lat REAL,
                lon REAL,
                combined_score REAL,
                detection_count INTEGER,
                status TEXT
            );

            -- New arrivals (single detections)
            CREATE TABLE new_arrivals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                lat REAL,
                lon REAL,
                score REAL,
                status TEXT
            );

            -- SWOT passes
            CREATE TABLE swot_passes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pass_date TEXT,
                granule_id TEXT,
                coverage_area TEXT
            );
            "
        )?;

        log::info!("Schema created");
    } else {
        log::info!("Schema exists");
    }

    Ok(())
}

/// Populate database from processing results JSON
pub fn populate_from_results(results_file: &Path, conn: &Connection) -> SqliteResult<()> {
    log::info!("Loading results from: {:?}", results_file);

    let results_content = fs::read_to_string(results_file)?;
    let results: Value = serde_json::from_str(&results_content)?;

    let run_timestamp = results
        .get("run_timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let tiles_processed = results
        .get("processed")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    log::info!("Run: {}", run_timestamp);
    log::info!("Tiles: {}", tiles_processed);
    log::info!();

    // Insert anomaly hits
    log::info!("Inserting anomaly hits...");
    let mut anomaly_count = 0;

    let results_array = results
        .get("results")
        .and_then(|v| v.as_array())
        .unwrap_or(&Vec::<Value>::new());

    for tile_result in results_array {
        let tile_path = tile_result
            .get("tile")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let sensors = tile_result
            .get("sensors")
            .and_then(|v| v.as_object())
            .unwrap_or(&serde_json::Map::new());

        for (sensor_name, sensor_data) in sensors {
            if let Some(anomaly_count_val) = sensor_data.get("anomaly_count")
                .and_then(|v| v.as_i64())
            {
                let zscore = sensor_data.get("max_zscore")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);

                let detected_at = Utc::now().format("%Y-%m-%dT%H:%M:%S%.fZ").to_string();

                conn.execute(
                    "INSERT INTO anomaly_hits (tile_path, sensor_type, zscore, anomaly_count, detected_at)
                     VALUES (?, ?, ?, ?, ?)",
                    [
                        tile_path,
                        sensor_name,
                        zscore,
                        anomaly_count_val,
                        detected_at,
                    ],
                )?;

                anomaly_count += 1;
            }
        }
    }

    conn.commit()?;
    log::info!("Inserted {} anomaly hits", anomaly_count);
    log::info!();

    // Summary
    log::info!("Database Summary:");

    let anomaly_hits_count = conn.query_row(
        "SELECT COUNT(*) FROM anomaly_hits",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    log::info!("  anomaly_hits: {}", anomaly_hits_count);

    let stationary_anchors_count = conn.query_row(
        "SELECT COUNT(*) FROM stationary_anchors",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    log::info!("  stationary_anchors: {}", stationary_anchors_count);

    let new_arrivals_count = conn.query_row(
        "SELECT COUNT(*) FROM new_arrivals",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    log::info!("  new_arrivals: {}", new_arrivals_count);

    let swot_passes_count = conn.query_row(
        "SELECT COUNT(*) FROM swot_passes",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    log::info!("  swot_passes: {}", swot_passes_count);

    Ok(())
}

/// Main entry point for the database population script
pub fn main() -> SqliteResult<()> {
    eprintln!("======================================================================");
    eprintln!("DATABASE POPULATOR");
    eprintln!("======================================================================");
    eprintln!();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <results.json>", args[0]);
        eprintln!();
        eprintln!("Example:");
        eprintln!("  {} outputs/full_lake_run/20260402_081510/full_results.json", args[0]);
        return Ok(());
    }

    let results_file = Path::new(&args[1]);

    if !results_file.exists() {
        eprintln!("ERROR: Results file not found: {:?}", results_file);
        return Ok(());
    }

    // Ensure DB directory exists
    let db_path = Path::new(DB_PATH);
    if let Some(parent) = db_path.parent() {
        parent.mkdir_all(true).ok();
    }

    // Connect to DB
    eprintln!("Database: {:?}", db_path);
    let conn = Connection::open(db_path)?;

    let _ = (|| {
        // Initialize schema
        init_db(&conn)?;
        eprintln!();

        // Populate from results
        populate_from_results(results_file, &conn)?;
        eprintln!();

        eprintln!("======================================================================");
        eprintln!("DATABASE POPULATED SUCCESSFULLY");
        eprintln!("======================================================================");

        Ok(())
    })();

    conn.close()?;
    Ok(())
}
```

## Forge wire
- **Pipeline integration**: Called by `full_lake_michigan_run.py` after processing completes, passing the JSON results path
- **Database persistence**: Stores anomaly detections, stationary anchors, new arrivals, and SWOT pass metadata for downstream analysis
- **Schema idempotency**: Checks and creates tables only once per run, safe for repeated invocations

## Risks
- **JSON schema drift**: If `full_lake_michigan_run.py` changes its output structure, deserialization will fail silently with `unwrap_or` defaults
- **SQLite file locking**: Concurrent runs may fail if the database is already open; consider using a connection pool or file locking
- **Path assumptions**: Hardcoded `wreckhunter2000/` prefix may not exist in all deployment environments; add environment variable override
- **No validation**: Missing `tile`, `sensor_name`, or `anomaly_count` fields will be silently skipped; add schema validation for production
