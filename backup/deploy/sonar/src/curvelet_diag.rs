//! Runtime diagnostics for the curvelet denoising pipeline.
//!
//! Every call through `curvelet_denoise_gray_image_tagged` appends an entry here.
//! The Tauri command `get_curvelet_diagnostics` returns the log so the
//! frontend can display what actually ran (transform size, threshold used,
//! errors, timings).

use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Default)]
pub struct CurveletDiagEntry {
    /// Tag identifying the call site (e.g. "waterfall_ch0", "preview", "estimate")
    pub tag: String,
    /// Input image dimensions
    pub width: usize,
    pub height: usize,
    /// Number of curvelet scales used
    pub num_scales: usize,
    /// Threshold that was applied (0.0 = estimation-only run)
    pub threshold_applied: f64,
    /// MAD-estimated universal threshold (0.0 if estimation failed)
    pub suggested_threshold: f64,
    /// Wall time for the entire curvelet round-trip (ms)
    pub elapsed_ms: u64,
    /// Any error message (empty = success)
    pub error: String,
}

static DIAG_LOG: Mutex<Vec<CurveletDiagEntry>> = Mutex::new(Vec::new());

/// Append a diagnostic entry.  Never panics — silently drops on lock failure.
pub fn push(entry: CurveletDiagEntry) {
    if let Ok(mut guard) = DIAG_LOG.lock() {
        // Keep at most 200 entries so memory stays bounded
        if guard.len() >= 200 {
            guard.drain(0..100);
        }
        guard.push(entry);
    }
}

/// Drain and return all accumulated diagnostics (clears the log).
pub fn drain() -> Vec<CurveletDiagEntry> {
    DIAG_LOG.lock().map(|mut g| g.drain(..).collect()).unwrap_or_default()
}

/// Peek without clearing.
pub fn snapshot() -> Vec<CurveletDiagEntry> {
    DIAG_LOG.lock().map(|g| g.clone()).unwrap_or_default()
}
