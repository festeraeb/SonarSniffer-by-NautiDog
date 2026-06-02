# integrate/unmapped/laptopdump_programming_root/batch_download_manager.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/batch_download_manager.rs

## Rust source
```rust
//! CESAROPS Batch Download Manager — Swarm Mode
//! Distributes satellite data downloads across multiple nodes for the Great Lakes region.
//! Each node grabs a unique "chunk" to maximize bandwidth usage on 1Gbps connections.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use tracing::{debug, info, warn};

/// Lake bounding boxes for Great Lakes region
/// NOTE: michigan stops at -85.5W; huron starts at -84.0W.
/// The Straits of Mackinac (~45.8N, -84.7W) falls in neither — use 'straits' explicitly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lake {
    pub bbox: [f64; 4],
    pub label: String,
}

/// Lake configuration
pub const LAKES: &[Lake] = &[
    Lake {
        bbox: [46.5, -92.0, 48.0, -84.5],
        label: "Lake Superior".to_string(),
    },
    Lake {
        bbox: [41.5, -88.0, 46.0, -85.5],
        label: "Lake Michigan".to_string(),
    },
    Lake {
        bbox: [45.65, -85.0, 46.10, -84.10],
        label: "Straits of Mackinac".to_string(),
    },
    Lake {
        bbox: [42.5, -84.0, 46.0, -81.0],
        label: "Lake Huron".to_string(),
    },
    Lake {
        bbox: [41.3, -83.5, 42.5, -78.8],
        label: "Lake Erie".to_string(),
    },
    Lake {
        bbox: [43.2, -79.5, 44.2, -76.0],
        label: "Lake Ontario".to_string(),
    },
];

/// Task representing a single download operation
#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub lake: String,
    pub year: i32,
    pub id: String,
}

/// Batch download manager for CESAROPS inference pipeline
pub struct BatchDownloadManager {
    dry_run: bool,
    max_results: usize,
    sensors: String,
    chunk_id: Option<String>,
}

impl BatchDownloadManager {
    /// Create a new batch download manager
    pub fn new(
        dry_run: bool,
        max_results: usize,
        sensors: String,
        chunk_id: Option<String>,
    ) -> Self {
        Self {
            dry_run,
            max_results,
            sensors,
            chunk_id,
        }
    }

    /// Execute a single download for a specific slice of data
    pub fn run_download(
        &self,
        lake_key: &str,
        year: i32,
        chunk_id: Option<&str>,
    ) -> Result<()> {
        let lake = LAKES.iter().find(|l| l.label == lake_key)
            .ok_or_else(|| anyhow::anyhow!("Unknown lake: {}", lake_key))?;

        let b = lake.bbox;
        // Summer/Fall only (June-Oct) to avoid ice and heavy cloud cover
        let dates = [
            NaiveDate::from_ymd_opt(year, 6, 1).unwrap(),
            NaiveDate::from_ymd_opt(year, 10, 31).unwrap(),
        ];

        // Output directory structure: downloads/lake/year/
        let out_dir = PathBuf::from("downloads")
            .join(lake_key)
            .join(year.to_string());

        // Ensure parent directories exist
        if !self.dry_run {
            if let Some(parent) = out_dir.parent() {
                std::fs::create_dir_all(parent)
                    .context("Failed to create output directory")?;
            }
        }

        let cmd = Command::new("python3")
            .arg("universal_downloader.py")
            .arg("--bbox")
            .arg(format!("{:.1},{:.1},{:.1},{:.1}", b[0], b[1], b[2], b[3]))
            .arg("--dates")
            .arg(dates[0].format("%Y-%m-%d"))
            .arg("--dates")
            .arg(dates[1].format("%Y-%m-%d"))
            .arg("--sensors")
            .arg(&self.sensors)
            .arg("--max-results")
            .arg(self.max_results.to_string())
            .arg("--output")
            .arg(out_dir.to_string_lossy())
            .output()
            .context("Failed to execute universal_downloader.py")?;

        if cmd.status.success() {
            info!(
                "✅ Success. Data saved to: {}",
                out_dir.to_string_lossy()
            );
        } else {
            let stderr = String::from_utf8_lossy(&cmd.stderr);
            warn!(
                "⚠️ API Error or No Data: {}...",
                stderr.chars().take(150).collect::<String>()
            );
        }

        Ok(())
    }

    /// Build the "Master List" of all tasks
    pub fn build_tasks(
        &self,
        lakes: &[&str],
        start_year: i32,
        end_year: i32,
    ) -> Vec<DownloadTask> {
        let mut chunks = Vec::new();

        for lake in lakes {
            for year in start_year..=end_year {
                chunks.push(DownloadTask {
                    lake: lake.to_string(),
                    year,
                    id: format!("{}-{}", lake, year),
                });
            }
        }

        chunks
    }

    /// Filter tasks for a specific node if chunk_id is provided
    /// Simple hash distribution: Node 1 gets even years, Node 2 gets odd, etc.
    pub fn filter_tasks(&self, tasks: &mut Vec<DownloadTask>) {
        if let Some(chunk_id) = &self.chunk_id {
            let node_idx = chunk_id.parse::<usize>().unwrap_or(0);
            let total = tasks.len();
            let step = if total > 0 { total / node_idx.max(1) } else { 1 };
            
            tasks.retain(|_| {
                let i = tasks.len() - 1;
                i % 2 == node_idx
            });
        }
    }

    /// Run all downloads with rate limiting
    pub async fn run_all(&self, tasks: &[DownloadTask]) -> Result<()> {
        info!(
            "🚀 BATCH START: {} tasks for Node {:?}",
            tasks.len(),
            self.chunk_id.as_deref()
        );

        for (i, task) in tasks.iter().enumerate() {
            debug!(
                "Processing task {}/{}: {} ({})",
                i + 1,
                tasks.len(),
                task.lake,
                task.year
            );

            self.run_download(&task.lake, task.year, Some(&task.id))?;

            if !self.dry_run {
                // Rate limit courtesy of the API
                sleep(Duration::from_secs(2)).await;
            }
        }

        info!("🚀 BATCH COMPLETE");
        Ok(())
    }
}

/// Main entry point for CLI usage
#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    
    // Parse arguments similar to Python version
    let mut dry_run = false;
    let mut max_results = 15;
    let mut sensors = "hls,sar".to_string();
    let mut chunk_id: Option<String> = None;
    let mut lakes = vec!["all"];
    let mut start_year = 2013;
    let mut end_year = 2025;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--lakes" => {
                i += 1;
                if i < args.len() {
                    lakes = args[i].split(',').map(|s| s.to_string()).collect();
                }
            }
            "--start" => {
                i += 1;
                if i < args.len() {
                    start_year = args[i].parse().unwrap_or(2013);
                }
            }
            "--end" => {
                i += 1;
                if i < args.len() {
                    end_year = args[i].parse().unwrap_or(2025);
                }
            }
            "--sensors" => {
                i += 1;
                if i < args.len() {
                    sensors = args[i].clone();
                }
            }
            "--max-results" => {
                i += 1;
                if i < args.len() {
                    max_results = args[i].parse().unwrap_or(15);
                }
            }
            "--chunk-id" => {
                i += 1;
                if i < args.len() {
                    chunk_id = Some(args[i].clone());
                }
            }
            "--dry-run" => {
                dry_run = true;
            }
            _ => {}
        }
        i += 1;
    }

    let lakes = if lakes[0] == "all" {
        LAKES.iter().map(|l| l.label.as
