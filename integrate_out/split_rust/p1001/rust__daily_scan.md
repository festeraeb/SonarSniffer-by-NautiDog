# integrate/unmapped/laptopdump_wreckhunter_build/daily_scan.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/daily_scan.rs

## Rust source
```rust
//! Daily scan automation for CESAROPS inference pipeline
//! 
//! This module orchestrates the daily satellite tile processing workflow:
//! 1. Initialize database connection
//! 2. Discover new TIFF tiles from data directory
//! 3. Process tiles with GPU/TPU inference
//! 4. Log detections to database (INTERNAL classification)
//! 5. Generate live KMZ feed
//!
//! Usage:
//!     cesarops-inference --daily-scan
//!
//! Cron integration:
//!     0 10 * * * /usr/bin/cargo run --bin cesarops-inference -- daily-scan >> logs/daily_scan.log 2>&1

use std::path::{Path, PathBuf};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, Utc};
use cesarops_engine::{
    process_tile,
    init_db,
    DatabaseConnection,
    TileProcessor,
    DetectionResult,
};
use cesarops_logging::{
    Logger,
    LogLevel,
    LogEntry,
};
use cesarops_database::{
    insert_detections,
    get_db_path,
};
use cesarops_kmz::{
    generate_kmz_feed,
    KmzFeedConfig,
};

/// Configuration for daily scan operations
#[derive(Debug, Clone)]
pub struct DailyScanConfig {
    /// Path to database file
    pub db_path: PathBuf,
    /// Directory containing TIFF tiles
    pub data_dir: PathBuf,
    /// Directory for log files
    pub log_dir: PathBuf,
    /// Output directory for KMZ feeds
    pub kmz_output: PathBuf,
    /// Maximum tiles to process per run (default: 10 for testing)
    pub max_tiles: usize,
}

impl Default for DailyScanConfig {
    fn default() -> Self {
        Self {
            db_path: Path::new("wreckhunter2000/LAKE_MICHIGAN_CENSUS_2026.db").into(),
            data_dir: Path::new("wreckhunter2000/data").into(),
            log_dir: Path::new("logs").into(),
            kmz_output: Path::new("outputs/live_feed.kmz").into(),
            max_tiles: 10,
        }
    }
}

impl DailyScanConfig {
    /// Create config from current working directory
    pub fn from_current_dir() -> Result<Self, Box<dyn std::error::Error>> {
        let base = std::env::current_dir()?;
        Ok(Self {
            db_path: base.join("wreckhunter2000/LAKE_MICHIGAN_CENSUS_2026.db"),
            data_dir: base.join("wreckhunter2000/data"),
            log_dir: base.join("logs"),
            kmz_output: base.join("outputs/live_feed.kmz"),
            max_tiles: 10,
        })
    }
}

/// Daily scan result summary
#[derive(Debug, Clone)]
pub struct DailyScanResult {
    /// Number of tiles processed
    pub tiles_processed: usize,
    /// Total anomaly detections
    pub total_detections: u64,
    /// High confidence tile count
    pub high_confidence: usize,
    /// Medium confidence tile count
    pub medium_confidence: usize,
    /// Low confidence tile count
    pub low_confidence: usize,
    /// Timestamp of scan completion
    pub scan_timestamp: DateTime<Utc>,
}

impl DailyScanResult {
    /// Create result from scan statistics
    pub fn new(
        tiles_processed: usize,
        total_detections: u64,
        high_confidence: usize,
        medium_confidence: usize,
        low_confidence: usize,
    ) -> Self {
        Self {
            tiles_processed,
            total_detections,
            high_confidence,
            medium_confidence,
            low_confidence,
            scan_timestamp: Utc::now(),
        }
    }
}

/// Daily scan runner
pub struct DailyScanRunner {
    config: DailyScanConfig,
    logger: Logger,
}

impl DailyScanRunner {
    /// Create new daily scan runner
    pub fn new(config: DailyScanConfig) -> Result<Self, Box<dyn std::error::Error>> {
        // Ensure log directory exists
        fs::create_dir_all(&config.log_dir)?;
        fs::create_dir_all(&config.kmz_output.parent().unwrap())?;

        let logger = Logger::new(
            config.log_dir.join(format!(
                "daily_scan_{}.log",
                Utc::now().format("%Y-%m-%d")
            )),
            LogLevel::Info,
        )?;

        Ok(Self { config, logger })
    }

    /// Execute daily scan pipeline
    pub fn run(&self) -> Result<DailyScanResult, Box<dyn std::error::Error>> {
        self.logger.log(&LogEntry::info("Starting CESAROPS daily scan"));

        // Step 1: Initialize database
        self.logger.log(&LogEntry::info("Initializing database..."));
        let db = init_db(&self.config.db_path)?;
        self.logger.log(&LogEntry::info("Database initialized successfully"));

        // Step 2: Discover tiles
        self.logger.log(&LogEntry::info("Discovering tiles to process..."));
        let tiff_files = discover_tiles(&self.config.data_dir)?;
        
        if tiff_files.is_empty() {
            self.logger.log(&LogEntry::warn("No TIFF files found in data directory"));
            self.logger.log(&LogEntry::info(
                format!("Place tiles in: {:?}", self.config.data_dir)
            ));
            return Ok(DailyScanResult::new(
                0,
                0,
                0,
                0,
                0,
            ));
        }

        let total_tiles = tiff_files.len();
        let tiles_to_process = std::cmp::min(self.config.max_tiles, total_tiles);
        self.logger.log(&LogEntry::info(
            format!("Found {} tiles, processing up to {}", total_tiles, tiles_to_process)
        ));

        // Step 3: Process tiles
        self.logger.log(&LogEntry::info("Processing tiles..."));
        let mut total_detections = 0u64;
        let mut high_confidence = 0usize;
        let mut medium_confidence = 0usize;
        let mut low_confidence = 0usize;

        for (i, tile_path) in tiff_files.iter().take(tiles_to_process).enumerate() {
            let tile_name = tile_path.file_name().unwrap_or("unknown").to_string_lossy();
            self.logger.log(&LogEntry::info(
                format!("[{}/{}] Processing tile: {}", i + 1, tiles_to_process, tile_name)
            ));

            match process_tile_with_retry(tile_path, &db) {
                Ok(result) => {
                    if let Some(count) = result.gpu.anomaly_count {
                        total_detections += count as u64;

                        // Classify confidence based on detection count
                        if count > 100_000 {
                            high_confidence += 1;
                        } else if count > 10_000 {
                            medium_confidence += 1;
                        } else {
                            low_confidence += 1;
                        }

                        self.logger.log(&LogEntry::info(
                            format!("    → {} anomalies", count)
                        ));
                    }
                }
                Err(e) => {
                    self.logger.log(&LogEntry::error(
                        format!("    ✗ Error processing tile: {}", e)
                    ));
                }
            }
        }

        self.logger.log(&LogEntry::info(
            format!(
                "Total anomalies: {}, High: {}, Medium: {}, Low: {}",
                total_detections, high_confidence, medium_confidence, low_confidence
            )
        ));

        // Step 4: Log detections to database
        self.logger.log(&LogEntry::info("Logging detections to database..."));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        insert_detections(
            &db,
            &self.config.db_path,
            total_detections,
            now,
            "INTERNAL",
        )?;
        self.logger.log(&LogEntry::info("Detections logged successfully"));

        // Step 5: Generate KMZ feed
        self.logger.log(&LogEntry::info("Generating live KMZ feed..."));
        let kmz_config = KmzFeedConfig {
            output_path: self.config.kmz_output.clone(),
            feed_url: "http://localhost:8080/feed.kmz".to_string(),
            sorter_url: "http://localhost:8080/sorter".to_string(),
        };
        
        generate_kmz_feed(&kmz_config, &db)?;
        self.logger.log(&LogEntry::info("KMZ feed generated successfully"));

        // Summary
        self.logger.log(&LogEntry::info(
            format!(
                "SCAN COMPLETE - Tiles: {}, Detections: {}, Status: INTERNAL",
                tiles_to_process, total_detections
            )
        ));

        Ok(DailyScanResult::new(
            tiles_to_process,
            total_detections,
            high_confidence,
            medium_confidence,
            low_confidence,
        ))
    }
}

/// Discover all TIFF files in the data directory
fn discover_tiles(data_dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut tiles = Vec::new();
    
    if data_dir.exists() && data_dir.is_dir() {
        for entry in fs::read_dir(data_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.extension().map_or(false, |ext| ext == "tif") ||
               path.extension().map_or(false, |ext| ext == "tiff") {
                tiles.push(path);
            }
        }
    }
    
    tiles.sort();
    Ok(tiles)
}

/// Process a single tile with retry logic
fn process_tile_with_retry(
    tile_path: &Path,
    db: &DatabaseConnection,
) -> Result<DetectionResult, Box<dyn std::error::Error>> {
    const MAX_RETRIES: u32 = 3;
    const RETRY_DELAY_MS: u64 = 1000;

    for attempt in 1..=MAX_RETRIES {
        match process_tile(tile_path, db) {
            Ok(result) => return Ok(result),
            Err(e) => {
                if attempt < MAX_RETRIES {
                    let delay = RETRY_DELAY_MS * attempt as u64;
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                    self_logger::warn!(
                        "Tile processing failed, retrying (attempt {}/{}): {}",
                        attempt,
                        MAX_RETRIES,
                        e
                    );
                } else {
                    return Err(e.into());
                }
            }
        }
    }
    
    Err("Max retries exceeded".into())
}
```

## Forge wire
- **Forge pipeline integration**: Called from `cesarops-inference/src/bin/daily_scan.rs` as a standalone binary or as a pipeline step
- **Pipeline trigger**: Scheduled via cron or triggered by Forge's event system when new tiles arrive
- **Output hooks**: Writes to database, generates KMZ, and can emit webhook notifications via `cesarops_webhooks` module

## Risks
- **Database path hardcoding**: Uses relative paths that may break in containerized deployments; should use environment variables or config files
- **Tile processing limits**: Hardcoded `max_tiles: 10` for testing; production should use configurable limits from environment
- **Error handling**: Tile processing errors are logged but not persisted; failed tiles are silently skipped
- **KMZ generation**: Assumes local server running on `localhost:8080`; needs proper URL configuration for production
- **Memory usage**: Processing many tiles in a loop without streaming could exhaust memory; consider async processing with `tokio`
