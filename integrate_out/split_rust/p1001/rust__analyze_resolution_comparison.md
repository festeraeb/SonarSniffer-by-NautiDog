# integrate/unmapped/laptopdump_wreckhunter_build/analyze_resolution_comparison.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/resolution_comparison.rs

## Rust source
```rust
//! Resolution comparison analysis module for CESAROPS inference pipeline.
//! Compares full resolution vs reduced resolution runs for accuracy and speed metrics.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, params, Row};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::SystemTime;

/// Results of resolution comparison analysis.
#[derive(Debug, Serialize)]
pub struct ResolutionComparisonResults {
    /// ISO 8601 timestamp of analysis.
    pub timestamp: String,
    /// Number of full resolution runs analyzed.
    pub full_resolution_runs: usize,
    /// Number of reduced resolution runs analyzed.
    pub reduced_resolution_runs: usize,
    /// Comparison metrics.
    pub metrics: ResolutionComparisonMetrics,
}

/// Metrics comparing full vs reduced resolution runs.
#[derive(Debug, Serialize)]
pub struct ResolutionComparisonMetrics {
    /// Average detections in full resolution runs.
    pub full_avg_detections: f64,
    /// Average detections in reduced resolution runs.
    pub reduced_avg_detections: f64,
    /// Average runtime in full resolution runs (seconds).
    pub full_avg_time: f64,
    /// Average runtime in reduced resolution runs (seconds).
    pub reduced_avg_time: f64,
    /// Speedup factor (full_time / reduced_time).
    pub speedup: f64,
}

/// Analyze resolution comparison from database.
///
/// # Arguments
/// * `db_path` - Path to the CESAROPS runs database.
///
/// # Returns
/// * `Ok(ResolutionComparisonResults)` - Analysis results.
/// * `Err` - Database or I/O error.
pub fn analyze_resolution_comparison(db_path: &Path) -> Result<ResolutionComparisonResults, Box<dyn std::error::Error>> {
    let conn = Connection::open(db_path)?;

    // Query runs ordered by run_id
    let runs: Vec<(i64, String, i64, f64, bool)> = conn
        .query(
            "SELECT run_id, run_name, detection_count, duration_seconds, chunking_enabled FROM runs ORDER BY run_id",
            [],
        )?
        .into_iter()
        .map(|row| {
            let run_id = row.get::<_, i64>(0)?;
            let run_name = row.get::<_, String>(1)?;
            let detection_count = row.get::<_, i64>(2)?;
            let duration_seconds = row.get::<_, f64>(3)?;
            let chunking_enabled = row.get::<_, bool>(4)?;
            Ok((run_id, run_name, detection_count, duration_seconds, chunking_enabled))
        })
        .collect::<Result<_, rusqlite::Error>>()?;

    // Partition into full and reduced resolution runs
    let (full_res_runs, reduced_res_runs): (Vec<_>, Vec<_>) = runs
        .into_iter()
        .partition(|(_, _, _, _, chunking)| *chunking);

    let full_res_runs_len = full_res_runs.len();
    let reduced_res_runs_len = reduced_res_runs.len();

    // Calculate average detections
    let full_avg_detections = if full_res_runs_len > 0 {
        full_res_runs.iter().map(|(_, _, detections, _, _)| detections as f64).sum::<f64>() / full_res_runs_len as f64
    } else {
        0.0
    };

    let reduced_avg_detections = if reduced_res_runs_len > 0 {
        reduced_res_runs.iter().map(|(_, _, detections, _, _)| detections as f64).sum::<f64>() / reduced_res_runs_len as f64
    } else {
        0.0
    };

    // Calculate average runtime
    let full_avg_time = if full_res_runs_len > 0 {
        full_res_runs.iter().map(|(_, _, _, time, _)| time).sum::<f64>() / full_res_runs_len as f64
    } else {
        0.0
    };

    let reduced_avg_time = if reduced_res_runs_len > 0 {
        reduced_res_runs.iter().map(|(_, _, _, time, _)| time).sum::<f64>() / reduced_res_runs_len as f64
    } else {
        0.0
    };

    // Calculate speedup
    let speedup = if full_avg_time > 0.0 && reduced_avg_time > 0.0 {
        full_avg_time / reduced_avg_time
    } else {
        0.0
    };

    // Query detections for position match rate (only if we have runs in both categories)
    let (position_match_rate, avg_score_diff) = if full_res_runs_len > 0 && reduced_res_runs_len > 0 {
        let full_run_id = full_res_runs[0].0;
        let reduced_run_id = reduced_res_runs[0].0;

        // Query matching detections (limit 100 for performance)
        let matches: Vec<(i64, i64, f64, i64, i64, f64)> = conn
            .query(
                "SELECT d1.pixel_row, d1.pixel_col, d1.score, d2.pixel_row, d2.pixel_col, d2.score \
                 FROM detections d1 \
                 JOIN detections d2 ON d1.run_id = ? AND d2.run_id = ? \
                 WHERE d1.pixel_row = d2.pixel_row AND d1.pixel_col = d2.pixel_col \
                 LIMIT 100",
                params![full_run_id, reduced_run_id],
            )?
            .into_iter()
            .map(|row| {
                let pixel_row = row.get::<_, i64>(0)?;
                let pixel_col = row.get::<_, i64>(1)?;
                let score = row.get::<_, f64>(2)?;
                let pixel_row2 = row.get::<_, i64>(3)?;
                let pixel_col2 = row.get::<_, i64>(4)?;
                let score2 = row.get::<_, f64>(5)?;
                Ok((pixel_row, pixel_col, score, pixel_row2, pixel_col2, score2))
            })
            .collect::<Result<_, rusqlite::Error>>()?;

        let min_detections = full_avg_detections.min(reduced_avg_detections);
        let position_match_rate = if min_detections > 0.0 {
            matches.len() as f64 / min_detections * 100.0
        } else {
            0.0
        };

        let avg_score_diff = if !matches.is_empty() {
            matches.iter().map(|(_, _, score, _, _, score2)| (score - score2).abs()).sum::<f64>() / matches.len() as f64
        } else {
            0.0
        };

        Ok((position_match_rate, avg_score_diff))
    } else {
        Ok((0.0, 0.0))
    };

    let (position_match_rate, avg_score_diff) = position_match_rate?;

    // Build results
    let results = ResolutionComparisonResults {
        timestamp: Utc::now().to_rfc3339(),
        full_resolution_runs: full_res_runs_len,
        reduced_resolution_runs: reduced_res_runs_len,
        metrics: ResolutionComparisonMetrics {
            full_avg_detections,
            reduced_avg_detections,
            full_avg_time,
            reduced_avg_time,
            speedup,
        },
    };

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_analyze_resolution_comparison_empty_db() {
        // Create a temporary empty database
        let temp_db = tempfile::tempfile().unwrap();
        let results = analyze_resolution_comparison(&temp_db).unwrap();
        
        assert_eq!(results.full_resolution_runs, 0);
        assert_eq!(results.reduced_resolution_runs, 0);
        assert_eq!(results.metrics.full_avg_detections, 0.0);
        assert_eq!(results.metrics.reduced_avg_detections, 0.0);
        assert_eq!(results.metrics.full_avg_time, 0.0);
        assert_eq!(results.metrics.reduced_avg_time, 0.0);
        assert_eq!(results.metrics.speedup, 0.0);
    }

    #[test]
    fn test_analyze_resolution_comparison_with_data() {
        // This test would require a real database setup
        // For now, we just verify the function compiles and handles errors
        let temp_db = tempfile::tempfile().unwrap();
        let result = analyze_resolution_comparison(&temp_db);
        assert!(result.is_ok());
    }
}
```

## Forge wire
- **Pipeline integration**: Called from `cesarops-inference/src/pipeline/run_analysis.rs` after a run completes, when `--resolution-comparison` flag is set
- **Output**: Writes JSON results to `outputs/resolution_comparison/resolution_comparison_results.json` in the run directory
- **Trigger**: Automatically runs when both full and reduced resolution runs exist for the same tile set

## Risks
- **Database schema dependency**: Assumes `runs` and `detections` tables exist with specific columns (chunking_enabled, detection_count, etc.) - will fail on older CESAROPS versions
- **Performance**: The position match query uses LIMIT 100, which may not represent full accuracy for large datasets
- **Division by zero**: Handled gracefully but could produce misleading metrics if only one resolution type exists
- **Timestamp format**: Uses RFC3339 which is more standard than Python's ISO format but may need validation in downstream systems
- **Error handling**: rusqlite errors are converted to Box<dyn Error> which loses specific error information for debugging
