# integrate/unmapped/laptopdump_programming_root/audit_wrecks_db.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/audit_wrecks_db.rs

## Rust source
```rust
//! Audit wrecks.db for coordinate quality - identify genuine vs centroid GPS entries
//!
//! This module provides database audit functionality for the cesarops-inference pipeline.
//! It validates GPS coordinate quality by identifying centroid clusters vs unique entries.

use rusqlite::{Connection, Result as SqliteResult};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::fs;

/// Configuration for the database audit
#[derive(Debug, Clone)]
pub struct AuditConfig {
    /// Path to the SQLite database
    pub db_path: String,
    /// Output directory for results
    pub output_dir: String,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            db_path: "wrecks.db".to_string(),
            output_dir: "outputs".to_string(),
        }
    }
}

impl AuditConfig {
    /// Create a new audit configuration from a path
    pub fn new<P: AsRef<Path>>(db_path: P, output_dir: P) -> Self {
        Self {
            db_path: db_path.as_ref().to_string_lossy().to_string(),
            output_dir: output_dir.as_ref().to_string_lossy().to_string(),
        }
    }
}

/// Represents a single GPS coordinate entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinateEntry {
    pub name: Option<String>,
    pub lat: f64,
    pub lon: f64,
}

/// Audit results containing all findings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditResults {
    pub tables: Vec<String>,
    pub features_count: Option<usize>,
    pub features_columns: Vec<(usize, String)>,
    pub lat_column: Option<String>,
    pub lon_column: Option<String>,
    pub name_column: Option<String>,
    pub gps_count: Option<usize>,
    pub clusters: Vec<(f64, f64, usize)>,
    pub total_clustered: usize,
    pub singletons: Vec<CoordinateEntry>,
    pub coord_source_column: Option<String>,
    pub coord_sources: Vec<(String, usize)>,
}

impl AuditResults {
    /// Create new audit results
    pub fn new() -> Self {
        Self {
            tables: Vec::new(),
            features_count: None,
            features_columns: Vec::new(),
            lat_column: None,
            lon_column: None,
            name_column: None,
            gps_count: None,
            clusters: Vec::new(),
            total_clustered: 0,
            singletons: Vec::new(),
            coord_source_column: None,
            coord_sources: Vec::new(),
        }
    }
}

/// Main audit runner
pub struct AuditRunner {
    config: AuditConfig,
}

impl AuditRunner {
    /// Create a new audit runner
    pub fn new(config: AuditConfig) -> Self {
        Self { config }
    }

    /// Run the full audit and return results
    pub fn run(&self) -> SqliteResult<AuditResults> {
        let conn = Connection::open(&self.config.db_path)?;
        let mut results = AuditResults::new();

        // List all tables
        let tables: Vec<String> = conn
            .query_row("SELECT name FROM sqlite_master WHERE type='table'", [], |row| {
                row.get(0)
            })
            .map(|mut rows| {
                let mut tables = Vec::new();
                while let Ok(Some(row)) = rows.next() {
                    tables.push(row.get::<String>(0).unwrap());
                }
                tables
            })?;
        results.tables = tables;

        // For each table, get schema and row count
        for table in &tables {
            let columns: Vec<(usize, String)> = conn
                .query_row(&format!("PRAGMA table_info({})", table), [], |row| {
                    let idx: usize = row.get(0)?;
                    let name: String = row.get(1)?;
                    Ok((idx, name))
                })
                .map(|mut rows| {
                    let mut cols = Vec::new();
                    while let Ok(Some(row)) = rows.next() {
                        cols.push((row.get::<usize>(0).unwrap(), row.get::<String>(1).unwrap()));
                    }
                    cols
                })?;
            results.features_columns.extend(columns);

            let count: Option<usize> = conn
                .query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |row| {
                    row.get(0)
                })
                .ok();
            results.features_count = Some(count.unwrap_or(0));
        }

        // Focus on features table if it exists
        if !tables.contains(&"features".to_string()) {
            return Ok(results);
        }

        // Get full schema for features table
        let features_columns: Vec<(usize, String)> = conn
            .query_row("PRAGMA table_info(features)", [], |row| {
                let idx: usize = row.get(0)?;
                let name: String = row.get(1)?;
                Ok((idx, name))
            })
            .map(|mut rows| {
                let mut cols = Vec::new();
                while let Ok(Some(row)) = rows.next() {
                    cols.push((row.get::<usize>(0).unwrap(), row.get::<String>(1).unwrap()));
                }
                cols
            })?;
        results.features_columns = features_columns.clone();

        // Find lat/lon columns
        let all_cols: Vec<(usize, String)> = features_columns.clone();
        let lat_col = all_cols
            .iter()
            .find(|(_, name)| name.to_lowercase().contains("lat"))
            .map(|(_, name)| name.clone());
        let lon_col = all_cols
            .iter()
            .find(|(_, name)| name.to_lowercase().contains("lon") || name.to_lowercase().contains("lng"))
            .map(|(_, name)| name.clone());
        let name_col = all_cols
            .iter()
            .find(|(_, name)| name.to_lowercase().contains("name"))
            .map(|(_, name)| name.clone());

        results.lat_column = lat_col.clone();
        results.lon_column = lon_col.clone();
        results.name_column = name_col.clone();

        if lat_col.is_none() || lon_col.is_none() {
            return Ok(results);
        }

        // Count rows with non-null GPS
        let lat_col_str = lat_col.as_ref().unwrap();
        let lon_col_str = lon_col.as_ref().unwrap();
        let gps_count: Option<usize> = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM features WHERE {} IS NOT NULL AND {} IS NOT NULL",
                    lat_col_str, lon_col_str
                ),
                [],
                |row| row.get(0),
            )
            .ok();
        results.gps_count = gps_count;

        // Find centroid clusters
        let cluster_rows: Vec<(f64, f64, usize)> = conn
            .query_row(
                &format!(
                    "SELECT ROUND({}, 2), ROUND({}, 2), COUNT(*) as cnt FROM features WHERE {} IS NOT NULL GROUP BY ROUND({}, 2), ROUND({}, 2) ORDER BY cnt DESC LIMIT 30",
                    lat_col_str, lon_col_str, lat_col_str, lon_col_str
                ),
                [],
                |row| {
                    let lat: f64 = row.get(0)?;
                    let lon: f64 = row.get(1)?;
                    let cnt: usize = row.get(2)?;
                    Ok((lat, lon, cnt))
                },
            )
            .map(|mut rows| {
                let mut clusters = Vec::new();
                while let Ok(Some(row)) = rows.next() {
                    clusters.push((row.get::<f64>(0).unwrap(), row.get::<f64>(1).unwrap(), row.get::<usize>(2).unwrap()));
                }
                clusters
            })?;
        results.clusters = cluster_rows.clone();

        let mut total_clustered = 0;
        for (_, _, cnt) in &cluster_rows {
            if *cnt > 5 {
                total_clustered += *cnt;
            }
        }
        results.total_clustered = total_clustered;

        // Find singletons
        let name_col_str = name_col.as_ref().unwrap_or("id");
        let lat_col_str = lat_col.as_ref().unwrap();
        let lon_col_str = lon_col.as_ref().unwrap();

        let singletons: Vec<CoordinateEntry> = conn
            .query_row(
                &format!(
                    "SELECT {}, {}, {} FROM features f WHERE {} IS NOT NULL AND (SELECT COUNT(*) FROM features f2 WHERE ROUND(f2.{}, 3) = ROUND(f.{}, 3) AND ROUND(f2.{}, 3) = ROUND(f.{}, 3)) = 1 ORDER BY {} LIMIT 100",
                    name_col_str, lat_col_str, lon_col_str, lat_col_str, lat_col_str, lat_col_str, lon_col_str, lon_col_str, lat_col_str
                ),
                [],
                |row| {
                    let name: Option<String> = row.get(0)?;
                    let lat: f64 = row.get(1)?;
                    let lon: f64 = row.get(2)?;
                    Ok(CoordinateEntry {
                        name,
                        lat,
                        lon,
                    })
                },
            )
            .map(|mut rows| {
                let mut singletons = Vec::new();
                while let Ok(Some(row)) = rows.next() {
                    singletons.push(row.get::<CoordinateEntry>(0).unwrap());
                }
                singletons
            })?;
        results.singletons = singletons.clone();

        // Check for coordinate_source or confidence column
        let coord_source_col = all_cols
            .iter()
            .find(|(_, name)| name.to_lowercase().contains("source") || name.to_lowercase().contains("conf"))
            .map(|(_, name)| name.clone());
        results.coord_source_column = coord_source_col.clone();

        if let Some(coord_source_col) = &coord_source_col {
            let coord_sources: Vec<(String, usize)> = conn
                .query_row(
                    &format!("SELECT DISTINCT {}, COUNT(*) FROM features GROUP BY {}", coord_source_col, coord_source_col),
                    [],
                    |row| {
                        let source: String = row.get(0)?;
                        let count: usize = row.get(1)?;
                        Ok((source, count))
                    },
                )
                .map(|mut rows| {
                    let mut sources = Vec::new();
                    while let Ok(Some(row)) = rows.next() {
                        sources.push((row.get::<String>(0).unwrap(), row.get::<usize>(1).unwrap()));
                    }
                    sources
                })?;
            results.coord_sources = coord_sources;
        }

        // Export singletons to JSON
        if !singletons.is_empty() {
            let output_path = format!("{}/wrecks_db_singletons.json", self.config.output_dir);
            let output_dir = Path::new(&self.config.output_dir);
            if !output_dir.exists() {
                fs::create_dir_all(output_dir)?;
            }
            let json_output = serde_json::to_string_pretty(&singletons)?;
            fs::write(&output_path, json_output)?;
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_runner_creation() {
        let config = AuditConfig::new("test.db", "outputs");
        let runner = AuditRunner::new(config);
        assert!(runner.config.db_path == "test.db");
    }
}
```

## Forge wire
- **Data Quality Pipeline**: Called after initial data ingestion to validate GPS coordinate quality before model training
- **Feature Store Enrichment**: Results feed into the feature store to flag centroid vs genuine GPS entries
- **Model Validation**: Audit results inform the model validation pipeline about data quality issues

## Risks
- **SQLite Version**: rusqlite may have different behavior than Python sqlite3 on edge cases
- **Large Datasets**: The LIMIT 100 on singletons could miss important outliers in production
- **Path Handling**: Windows vs Linux path separators need careful handling in production
- **JSON Serialization**: CoordinateEntry struct must match Python output format for pipeline compatibility
