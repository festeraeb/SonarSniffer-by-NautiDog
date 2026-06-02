use chrono::Utc;
use rusqlite::{params, Connection};

use crate::model::ExportCandidate;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS candidates (
  id TEXT PRIMARY KEY,
  lat REAL, lon REAL, source TEXT, confidence REAL,
  long_ft REAL, short_ft REAL, depth_ft REAL, relief_ft REAL,
  signature TEXT, notes TEXT, thumb_png TEXT, metrics_json TEXT,
  first_seen TEXT, last_seen TEXT
);
";

pub fn open(path: &std::path::Path) -> Result<Connection, rusqlite::Error> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

pub fn upsert(conn: &Connection, c: &ExportCandidate) -> Result<(), rusqlite::Error> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    conn.execute(
        "INSERT INTO candidates (
            id, lat, lon, source, confidence,
            long_ft, short_ft, depth_ft, relief_ft,
            signature, notes, thumb_png, metrics_json,
            first_seen, last_seen
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?14)
        ON CONFLICT(id) DO UPDATE SET
            lat=excluded.lat, lon=excluded.lon, source=excluded.source,
            confidence=excluded.confidence,
            long_ft=excluded.long_ft, short_ft=excluded.short_ft,
            depth_ft=excluded.depth_ft, relief_ft=excluded.relief_ft,
            signature=excluded.signature, notes=excluded.notes,
            thumb_png=excluded.thumb_png, metrics_json=excluded.metrics_json,
            last_seen=excluded.last_seen",
        params![
            c.id,
            c.lat,
            c.lon,
            c.source,
            c.confidence,
            c.long_ft,
            c.short_ft,
            c.depth_ft,
            c.relief_ft,
            c.signature,
            c.notes,
            c.thumb_png,
            c.metrics_json,
            now,
        ],
    )?;
    Ok(())
}

pub fn load_all(conn: &Connection) -> Result<Vec<ExportCandidate>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, lat, lon, source, confidence,
                long_ft, short_ft, depth_ft, relief_ft,
                signature, notes, thumb_png, metrics_json
         FROM candidates ORDER BY source, id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ExportCandidate {
            id: row.get(0)?,
            lat: row.get(1)?,
            lon: row.get(2)?,
            source: row.get(3)?,
            confidence: row.get(4)?,
            long_ft: row.get(5)?,
            short_ft: row.get(6)?,
            depth_ft: row.get(7)?,
            relief_ft: row.get(8)?,
            signature: row.get(9)?,
            notes: row.get(10)?,
            thumb_png: row.get(11)?,
            metrics_json: row.get(12)?,
        })
    })?;
    rows.collect()
}
