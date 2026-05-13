//! Triple-Lock Detection Pipeline — the core orchestration logic.
//!
//! Scout (1060) → Validator (P1000) → Jitter (TPU) → Confirmed
//! Each lock must pass before the next is invoked.
//! This eliminates false positives: sandbars fool vision but not thermal jitter.

use anyhow::Result;
use tracing::{info, warn};

use crate::types::{GeoTile, DetectionResult, MissionAction};
use crate::workers::{ScoutClient, ValidatorClient, JitterClient};

pub struct TripleLockPipeline {
    scout: ScoutClient,
    validator: ValidatorClient,
    jitter: JitterClient,
    // Thresholds
    scout_threshold: f32,
    validator_threshold: f32,
    jitter_threshold: f32,
}

impl TripleLockPipeline {
    pub fn new() -> Self {
        Self {
            scout: ScoutClient::new(),
            validator: ValidatorClient::new(),
            jitter: JitterClient::new(),
            scout_threshold: 0.6,
            validator_threshold: 0.5,
            jitter_threshold: 0.7,
        }
    }

    /// Run the full triple-lock pipeline on a tile.
    /// Returns early if any lock fails — saves compute on the later stages.
    pub async fn process_tile(&self, tile: &GeoTile) -> Result<DetectionResult> {
        let start = chrono::Utc::now().timestamp();

        // === LOCK 1: SCOUT (1060 / Florence-2) ===
        info!("[{}] Lock 1: Scout analyzing...", tile.id);
        let scout_report = match self.scout.analyze(tile).await {
            Ok(report) => report,
            Err(e) => {
                warn!("[{}] Scout failed: {}. Skipping tile.", tile.id, e);
                return Ok(DetectionResult {
                    tile_id: tile.id.clone(),
                    action: MissionAction::Standby,
                    scout: None,
                    validator: None,
                    jitter: None,
                    overall_confidence: 0.0,
                    timestamp: start,
                });
            }
        };

        if !scout_report.has_anomaly || scout_report.confidence < self.scout_threshold {
            info!("[{}] Lock 1 FAILED: confidence {:.2} < {:.2}", tile.id, scout_report.confidence, self.scout_threshold);
            return Ok(DetectionResult {
                tile_id: tile.id.clone(),
                action: MissionAction::Standby,
                scout: Some(scout_report.clone()),
                validator: None,
                jitter: None,
                overall_confidence: scout_report.confidence * 0.3,
                timestamp: start,
            });
        }
        info!("[{}] Lock 1 PASSED: {} ({:.2})", tile.id, scout_report.anomaly_type, scout_report.confidence);

        // === LOCK 2: CROSS-VALIDATOR (P1000 / Moondream2) ===
        info!("[{}] Lock 2: Validator confirming...", tile.id);
        let val_report = match self.validator.validate(tile).await {
            Ok(report) => report,
            Err(e) => {
                warn!("[{}] Validator failed: {}. Promoting to Investigate.", tile.id, e);
                return Ok(DetectionResult {
                    tile_id: tile.id.clone(),
                    action: MissionAction::Investigate,
                    scout: Some(scout_report.clone()),
                    validator: None,
                    jitter: None,
                    overall_confidence: scout_report.confidence * 0.5,
                    timestamp: start,
                });
            }
        };

        if !val_report.has_anomaly || val_report.confidence < self.validator_threshold {
            info!("[{}] Lock 2 FAILED: confidence {:.2} < {:.2}", tile.id, val_report.confidence, self.validator_threshold);
            return Ok(DetectionResult {
                tile_id: tile.id.clone(),
                action: MissionAction::Standby,
                scout: Some(scout_report.clone()),
                validator: Some(val_report.clone()),
                jitter: None,
                overall_confidence: (scout_report.confidence + val_report.confidence) * 0.25,
                timestamp: start,
            });
        }
        info!("[{}] Lock 2 PASSED: {} / {} ({:.2})", tile.id, val_report.shape_analysis, val_report.material_guess, val_report.confidence);

        // === LOCK 3: JITTER ANALYST (TPU VM) ===
        // This is the kill shot. Sandbars don't have thermal oscillation.
        info!("[{}] Lock 3: Jitter analysis (TPU)...", tile.id);
        let jitter_sig = match self.jitter.check(tile).await {
            Ok(sig) => sig,
            Err(e) => {
                warn!("[{}] Jitter failed: {}. TPU may be offline. Promoting to Investigate.", tile.id, e);
                return Ok(DetectionResult {
                    tile_id: tile.id.clone(),
                    action: MissionAction::Investigate,
                    scout: Some(scout_report.clone()),
                    validator: Some(val_report.clone()),
                    jitter: None,
                    overall_confidence: (scout_report.confidence + val_report.confidence) * 0.4,
                    timestamp: start,
                });
            }
        };

        if jitter_sig.certainty < self.jitter_threshold {
            info!("[{}] Lock 3 FAILED: certainty {:.2} < {:.2} (likely natural feature)", tile.id, jitter_sig.certainty, self.jitter_threshold);
            return Ok(DetectionResult {
                tile_id: tile.id.clone(),
                action: MissionAction::Standby,
                scout: Some(scout_report.clone()),
                validator: Some(val_report.clone()),
                jitter: Some(jitter_sig.clone()),
                overall_confidence: 0.2, // All vision agreed but physics says no
                timestamp: start,
            });
        }

        // === ALL THREE LOCKS PASSED ===
        let overall = (scout_report.confidence + val_report.confidence + jitter_sig.certainty) / 3.0;
        info!(
            "[{}] ALL LOCKS PASSED — CONFIRMED DETECTION. Material: {}, Depth: {}ft, Confidence: {:.2}",
            tile.id, jitter_sig.material, jitter_sig.depth_estimate_ft, overall
        );

        Ok(DetectionResult {
            tile_id: tile.id.clone(),
            action: MissionAction::Confirmed,
            scout: Some(scout_report.clone()),
            validator: Some(val_report.clone()),
            jitter: Some(jitter_sig.clone()),
            overall_confidence: overall,
            timestamp: start,
        })
    }

    /// Check health of all worker nodes
    pub async fn check_workers(&self) -> (bool, bool, bool) {
        let scout_ok = self.scout.health().await;
        let val_ok = self.validator.health().await;
        let jitter_ok = self.jitter.health().await;
        (scout_ok, val_ok, jitter_ok)
    }
}
