use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRequest {
    pub gpus: Vec<String>,
    #[serde(default)]
    pub research_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub id: Option<i64>,
    pub gpu: String,
    pub scan_type: String,
    pub result: String,
    pub research_mode: bool,
    pub research_notes: String,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct SelfTrainingDb {
    conn: Arc<Mutex<Connection>>,
}

impl SelfTrainingDb {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", &"WAL")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS scan_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gpu TEXT NOT NULL,
                scan_type TEXT NOT NULL,
                result TEXT NOT NULL,
                research_mode TEXT NOT NULL,
                research_notes TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS research_findings (
                id TEXT PRIMARY KEY,
                domain TEXT,
                title TEXT NOT NULL,
                source_url TEXT NOT NULL,
                doi TEXT,
                abstract_snippet TEXT,
                extracted_technique TEXT,
                hypothesis_status TEXT,
                ingested_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS research_hypotheses (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                suggested_band_weights TEXT,
                suggested_confidence_threshold REAL,
                status TEXT NOT NULL,
                test_result TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn insert_scan_result(&self, result: &ScanResult) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT INTO scan_results (gpu, scan_type, result, research_mode, research_notes, timestamp)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
            params![
                result.gpu,
                result.scan_type,
                result.result,
                result.research_mode.to_string(),
                result.research_notes,
                result.timestamp,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn recent_scan_results(&self, limit: usize) -> rusqlite::Result<Vec<ScanResult>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, gpu, scan_type, result, research_mode, research_notes, timestamp
            FROM scan_results
            ORDER BY timestamp DESC
            LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(ScanResult {
                id: row.get(0)?,
                gpu: row.get(1)?,
                scan_type: row.get(2)?,
                result: row.get(3)?,
                research_mode: row.get::<_, String>(4)? == "true",
                research_notes: row.get(5)?,
                timestamp: row.get(6)?,
            })
        })?;

        rows.collect()
    }

    pub fn insert_research_finding(
        &self,
        id: &str,
        domain: &str,
        title: &str,
        source_url: &str,
        doi: Option<&str>,
        abstract_snippet: &str,
        extracted_technique: Option<&str>,
        hypothesis_status: Option<&str>,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT OR IGNORE INTO research_findings
            (id, domain, title, source_url, doi, abstract_snippet, extracted_technique, hypothesis_status)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
            params![
                id,
                domain,
                title,
                source_url,
                doi,
                abstract_snippet,
                extracted_technique,
                hypothesis_status,
            ],
        )?;
        Ok(())
    }
}
