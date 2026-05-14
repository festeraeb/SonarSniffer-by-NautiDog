//! Closed-Loop Monitoring — rolling drift metrics and structured query interface.
//!
//! Tracks segment completion metrics, computes rolling drift averages,
//! and exposes a query interface for observability.
//!
//! Requirements covered:
//! - 11.1: Record segment metrics on completion
//! - 11.2: Rolling average drift over last N segments
//! - 11.3: Signal increased steering when rolling avg > 0.03
//! - 11.4: Flag domains requiring >1 retry as potential weaknesses
//! - 11.5: Structured query interface (MonitoringSummary)

use std::collections::VecDeque;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;

/// The segment execution mode — controls drift threshold selection.
///
/// NOTE: This mirrors the design's `SegmentMode`. The existing `SteeringMode` in
/// `drift.rs` serves a similar purpose. These will be consolidated in a future task
/// when the full pipeline wires everything together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentMode {
    /// Logic, math, parameter tuning — strict 0.05 threshold
    Precision,
    /// Research, synthesis, architecture — relaxed 0.15 threshold
    Exploratory,
}

/// Status of a completed segment.
///
/// NOTE: Will be consolidated with pipeline types in a future task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SegmentStatus {
    /// All checks passed.
    Completed,
    /// Budget exhausted but accuracy check passed.
    BudgetExhausted,
    /// Proceeded without nautivecs context (store unreachable).
    Unsteered,
    /// Failed after max retries.
    Failed,
}

/// Metrics recorded for each completed segment (Requirement 11.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentMetrics {
    /// RSU identifier (e.g. "segment_001")
    pub rsu_id: String,
    /// Logical drift score (0.0–1.0)
    pub drift_score: f32,
    /// Total tokens consumed across all phases
    pub tokens_used: u32,
    /// Number of retries before completion
    pub retries: u32,
    /// Wall-clock time for segment execution in milliseconds
    pub elapsed_ms: u64,
    /// Whether this was a precision or exploratory segment
    pub segment_mode: SegmentMode,
    /// Final status of the segment
    pub status: SegmentStatus,
    /// When the segment completed
    pub timestamp: DateTime<Utc>,
}

/// Summary exposed by the monitoring query interface (Requirement 11.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringSummary {
    /// Total number of segments that have been recorded
    pub total_segments_processed: usize,
    /// Average drift across all recorded segments
    pub average_drift: f32,
    /// Sum of all retries across all segments
    pub total_retries: u32,
    /// Number of segments that ended with BudgetExhausted status
    pub budget_exhaustion_count: u32,
    /// (precision_count, exploratory_count)
    pub segments_by_mode: (usize, usize),
}

/// Tracks rolling drift metrics and exposes a structured query interface.
///
/// The Monitor records metrics for every completed segment and maintains
/// a rolling window of drift scores for trend detection.
pub struct Monitor {
    /// Rolling window of recent segment drift scores (last N entries)
    drift_window: VecDeque<f32>,
    /// All recorded segment metrics (append-only log)
    metrics: Vec<SegmentMetrics>,
}

impl Monitor {
    /// Create a new Monitor with empty state.
    pub fn new() -> Self {
        Self {
            drift_window: VecDeque::new(),
            metrics: Vec::new(),
        }
    }

    /// Record metrics for a completed segment (Requirement 11.1).
    ///
    /// Logs the segment completion, updates the rolling drift window,
    /// and flags domains requiring >1 retry as potential weaknesses (Requirement 11.4).
    pub fn record(&mut self, metrics: SegmentMetrics) {
        // Update rolling drift window
        self.drift_window.push_back(metrics.drift_score);

        // Flag domains requiring >1 retry as potential weaknesses (Requirement 11.4)
        if metrics.retries > 1 {
            warn!(
                rsu_id = %metrics.rsu_id,
                retries = metrics.retries,
                mode = ?metrics.segment_mode,
                "Segment required >1 retry — flagging as potential weakness"
            );
        }

        self.metrics.push(metrics);
    }

    /// Compute rolling average drift over the last `window` segments (Requirement 11.2).
    ///
    /// If fewer than `window` segments have been recorded, averages over all available.
    /// Returns 0.0 if no segments have been recorded.
    pub fn rolling_drift_average(&self, window: usize) -> f32 {
        if self.drift_window.is_empty() {
            return 0.0;
        }

        let count = window.min(self.drift_window.len());
        let start = self.drift_window.len().saturating_sub(count);

        let sum: f32 = self.drift_window.iter().skip(start).sum();
        sum / count as f32
    }

    /// Check if steering intensity should be increased (Requirement 11.3).
    ///
    /// Returns true when the rolling average drift over the last 5 segments
    /// exceeds 0.03, indicating a systemic drift trend that needs correction.
    pub fn should_increase_steering(&self) -> bool {
        self.rolling_drift_average(5) > 0.03
    }

    /// Query all metrics for the structured query interface (Requirement 11.5).
    ///
    /// Returns a summary of all recorded segment metrics including totals,
    /// averages, and breakdowns by mode.
    pub fn query_metrics(&self) -> MonitoringSummary {
        let total_segments_processed = self.metrics.len();

        let average_drift = if self.metrics.is_empty() {
            0.0
        } else {
            let sum: f32 = self.metrics.iter().map(|m| m.drift_score).sum();
            sum / self.metrics.len() as f32
        };

        let total_retries: u32 = self.metrics.iter().map(|m| m.retries).sum();

        let budget_exhaustion_count = self
            .metrics
            .iter()
            .filter(|m| m.status == SegmentStatus::BudgetExhausted)
            .count() as u32;

        let precision_count = self
            .metrics
            .iter()
            .filter(|m| m.segment_mode == SegmentMode::Precision)
            .count();
        let exploratory_count = self
            .metrics
            .iter()
            .filter(|m| m.segment_mode == SegmentMode::Exploratory)
            .count();

        MonitoringSummary {
            total_segments_processed,
            average_drift,
            total_retries,
            budget_exhaustion_count,
            segments_by_mode: (precision_count, exploratory_count),
        }
    }
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_metrics(rsu_id: &str, drift: f32, retries: u32, mode: SegmentMode, status: SegmentStatus) -> SegmentMetrics {
        SegmentMetrics {
            rsu_id: rsu_id.to_string(),
            drift_score: drift,
            tokens_used: 1000,
            retries,
            elapsed_ms: 500,
            segment_mode: mode,
            status,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_empty_monitor_returns_zero_drift() {
        let monitor = Monitor::new();
        assert_eq!(monitor.rolling_drift_average(5), 0.0);
        assert!(!monitor.should_increase_steering());
    }

    #[test]
    fn test_record_adds_to_metrics() {
        let mut monitor = Monitor::new();
        let m = make_metrics("segment_001", 0.02, 0, SegmentMode::Precision, SegmentStatus::Completed);
        monitor.record(m);
        assert_eq!(monitor.metrics.len(), 1);
        assert_eq!(monitor.drift_window.len(), 1);
    }

    #[test]
    fn test_rolling_drift_average_over_window() {
        let mut monitor = Monitor::new();
        // Push 5 segments with known drift scores
        for i in 0..5 {
            let drift = 0.01 * (i + 1) as f32; // 0.01, 0.02, 0.03, 0.04, 0.05
            monitor.record(make_metrics(
                &format!("segment_{:03}", i),
                drift,
                0,
                SegmentMode::Precision,
                SegmentStatus::Completed,
            ));
        }
        // Average of last 5: (0.01+0.02+0.03+0.04+0.05)/5 = 0.03
        let avg = monitor.rolling_drift_average(5);
        assert!((avg - 0.03).abs() < 1e-6, "Expected ~0.03, got {}", avg);
    }

    #[test]
    fn test_rolling_drift_average_smaller_window() {
        let mut monitor = Monitor::new();
        for i in 0..5 {
            let drift = 0.01 * (i + 1) as f32;
            monitor.record(make_metrics(
                &format!("segment_{:03}", i),
                drift,
                0,
                SegmentMode::Precision,
                SegmentStatus::Completed,
            ));
        }
        // Average of last 3: (0.03+0.04+0.05)/3 = 0.04
        let avg = monitor.rolling_drift_average(3);
        assert!((avg - 0.04).abs() < 1e-6, "Expected ~0.04, got {}", avg);
    }

    #[test]
    fn test_rolling_drift_average_fewer_than_window() {
        let mut monitor = Monitor::new();
        monitor.record(make_metrics("segment_001", 0.02, 0, SegmentMode::Precision, SegmentStatus::Completed));
        monitor.record(make_metrics("segment_002", 0.04, 0, SegmentMode::Precision, SegmentStatus::Completed));
        // Only 2 segments, window=5 → averages over all 2
        let avg = monitor.rolling_drift_average(5);
        assert!((avg - 0.03).abs() < 1e-6, "Expected ~0.03, got {}", avg);
    }

    #[test]
    fn test_should_increase_steering_below_threshold() {
        let mut monitor = Monitor::new();
        // All low drift — should NOT trigger
        for i in 0..5 {
            monitor.record(make_metrics(
                &format!("segment_{:03}", i),
                0.01,
                0,
                SegmentMode::Precision,
                SegmentStatus::Completed,
            ));
        }
        assert!(!monitor.should_increase_steering());
    }

    #[test]
    fn test_should_increase_steering_above_threshold() {
        let mut monitor = Monitor::new();
        // All high drift — should trigger
        for i in 0..5 {
            monitor.record(make_metrics(
                &format!("segment_{:03}", i),
                0.05,
                0,
                SegmentMode::Precision,
                SegmentStatus::Completed,
            ));
        }
        assert!(monitor.should_increase_steering());
    }

    #[test]
    fn test_should_increase_steering_at_boundary() {
        let mut monitor = Monitor::new();
        // Exactly 0.03 average — should NOT trigger (> 0.03, not >=)
        for i in 0..5 {
            monitor.record(make_metrics(
                &format!("segment_{:03}", i),
                0.03,
                0,
                SegmentMode::Precision,
                SegmentStatus::Completed,
            ));
        }
        assert!(!monitor.should_increase_steering());
    }

    #[test]
    fn test_query_metrics_empty() {
        let monitor = Monitor::new();
        let summary = monitor.query_metrics();
        assert_eq!(summary.total_segments_processed, 0);
        assert_eq!(summary.average_drift, 0.0);
        assert_eq!(summary.total_retries, 0);
        assert_eq!(summary.budget_exhaustion_count, 0);
        assert_eq!(summary.segments_by_mode, (0, 0));
    }

    #[test]
    fn test_query_metrics_mixed() {
        let mut monitor = Monitor::new();
        monitor.record(make_metrics("segment_001", 0.02, 0, SegmentMode::Precision, SegmentStatus::Completed));
        monitor.record(make_metrics("segment_002", 0.04, 2, SegmentMode::Exploratory, SegmentStatus::Completed));
        monitor.record(make_metrics("segment_003", 0.06, 1, SegmentMode::Precision, SegmentStatus::BudgetExhausted));

        let summary = monitor.query_metrics();
        assert_eq!(summary.total_segments_processed, 3);
        assert!((summary.average_drift - 0.04).abs() < 1e-6);
        assert_eq!(summary.total_retries, 3); // 0 + 2 + 1
        assert_eq!(summary.budget_exhaustion_count, 1);
        assert_eq!(summary.segments_by_mode, (2, 1)); // 2 precision, 1 exploratory
    }
}
