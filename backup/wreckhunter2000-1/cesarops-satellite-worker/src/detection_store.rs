//! SQLite-backed detection store for temporal persistence matching.
//!
//! Each detection is a (lat, lon, confidence, pass_type, tile_id, timestamp) tuple.
//! The cluster matcher queries this store to find temporally persistent features.

use anyhow::Result;
use serde::Serialize;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite, Row};
use tracing::info;

#[derive(Debug, Clone, Serialize)]
pub struct DetectionRow {
    pub detection_id: String,
    pub lat: f64,
    pub lon: f64,
    pub confidence: f32,
    pub pass_type: String,
    pub tile_id: Option<String>,
    pub run_id: Option<String>,
    pub pixel_row: u32,
    pub pixel_col: u32,
    pub timestamp: i64,
}

pub struct DetectionStore {
    pool: Pool<Sqlite>,
}

impl DetectionStore {
    pub async fn open(db_path: &str) -> Result<Self> {
        let url = format!("sqlite://{db_path}");
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await?;

        // Create tables if they don't exist
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS tpu_detections (
                detection_id TEXT PRIMARY KEY,
                lat REAL NOT NULL,
                lon REAL NOT NULL,
                confidence REAL NOT NULL,
                pass_type TEXT NOT NULL,
                tile_id TEXT,
                run_id TEXT,
                pixel_row INTEGER,
                pixel_col INTEGER,
                timestamp INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_tpu_detections_location
            ON tpu_detections(lat, lon)
            "#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_tpu_detections_timestamp
            ON tpu_detections(timestamp)
            "#,
        )
        .execute(&pool)
        .await?;

        info!("Detection store initialized: {}", db_path);
        Ok(Self { pool })
    }

    pub async fn insert(&self, row: &DetectionRow) -> String {
        sqlx::query(
            r#"
            INSERT INTO tpu_detections
                (detection_id, lat, lon, confidence, pass_type, tile_id, run_id, pixel_row, pixel_col, timestamp)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&row.detection_id)
        .bind(row.lat)
        .bind(row.lon)
        .bind(row.confidence as f64)
        .bind(&row.pass_type)
        .bind(&row.tile_id)
        .bind(&row.run_id)
        .bind(row.pixel_row as i64)
        .bind(row.pixel_col as i64)
        .bind(row.timestamp)
        .execute(&self.pool)
        .await
        .ok();

        row.detection_id.clone()
    }

    pub async fn query_detections(
        &self,
        lat_min: Option<f64>,
        lon_min: Option<f64>,
        lat_max: Option<f64>,
        lon_max: Option<f64>,
        pass_type: Option<&str>,
        limit: usize,
    ) -> Vec<DetectionRow> {
        let mut sql = String::from(
            "SELECT detection_id, lat, lon, confidence, pass_type, tile_id, run_id, pixel_row, pixel_col, timestamp
             FROM tpu_detections WHERE 1=1",
        );

        if let Some(v) = lat_min {
            sql.push_str(&format!(" AND lat >= {v}"));
        }
        if let Some(v) = lon_min {
            sql.push_str(&format!(" AND lon >= {v}"));
        }
        if let Some(v) = lat_max {
            sql.push_str(&format!(" AND lat <= {v}"));
        }
        if let Some(v) = lon_max {
            sql.push_str(&format!(" AND lon <= {v}"));
        }
        if let Some(pt) = pass_type {
            sql.push_str(&format!(" AND pass_type = '{pt}'"));
        }

        sql.push_str(&format!(" ORDER BY timestamp DESC LIMIT {limit}"));

        let rows: Vec<sqlx::sqlite::SqliteRow> = sqlx::query(&sql)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

        rows.into_iter()
            .map(|r| {
                let pixel_row: i64 = r.try_get("pixel_row").unwrap_or(0);
                let pixel_col: i64 = r.try_get("pixel_col").unwrap_or(0);
                DetectionRow {
                    detection_id: r.try_get("detection_id").unwrap_or_default(),
                    lat: r.try_get("lat").unwrap_or(0.0),
                    lon: r.try_get("lon").unwrap_or(0.0),
                    confidence: r.try_get::<f64, _>("confidence").unwrap_or(0.0) as f32,
                    pass_type: r.try_get("pass_type").unwrap_or_default(),
                    tile_id: r.try_get("tile_id").ok(),
                    run_id: r.try_get("run_id").ok(),
                    pixel_row: pixel_row as u32,
                    pixel_col: pixel_col as u32,
                    timestamp: r.try_get("timestamp").unwrap_or(0),
                }
            })
            .collect()
    }
}
