//! Public anomaly detection interface.
//!
//! Workers supply detection parameters via the opaque `DetectionConfig` type.
//! The parameter schema is never visible in the public API — it lives entirely
//! in the private `internal_weights` module.

use ndarray::Array2;
use crate::precision::Scalar;

/// Opaque container for wreck-signature detection parameters.
///
/// Construct from a serialised byte blob produced by the calibration pipeline.
/// The blob format is defined in `internal_weights` and is not part of the
/// public API.
///
/// # Example
/// ```rust
/// let config = nauticuvs::DetectionConfig::from_bytes(&[]);
/// // Pass config to detect_anomaly with a real parameter blob at runtime.
/// ```
pub struct DetectionConfig(Vec<u8>);

impl DetectionConfig {
    /// Construct from a serialised parameter blob.
    ///
    /// The blob is validated lazily when `detect_anomaly` is called.
    /// This function never panics on arbitrary input.
    pub fn from_bytes(blob: &[u8]) -> Self {
        DetectionConfig(blob.to_vec())
    }
}

/// Result of an anomaly detection pass.
#[derive(Debug, Clone)]
pub struct DetectionResult {
    /// Composite anomaly score in [0.0, 1.0].
    /// Values > 0.5 indicate a probable human-made metal structure.
    pub score: Scalar,
    /// Row of the peak anomaly in the input grid.
    pub peak_row: usize,
    /// Column of the peak anomaly in the input grid.
    pub peak_col: usize,
    /// Curvelet energy ratio at the peak location.
    pub energy_ratio: Scalar,
    /// Phase coherence at the peak location.
    pub phase_coherence: Scalar,
}

/// Run the wreck-signature detection pipeline on a 2-D grid.
///
/// Internally:
/// 1. Runs `curvelet_forward` on the grid.
/// 2. Deserialises the `DetectionConfig` blob via `internal_weights::load_params`.
/// 3. Scores each candidate location using the calibrated parameters.
/// 4. Returns the highest-scoring candidate.
///
/// Returns `Err` if the parameter blob is malformed or the grid is empty.
pub fn detect_anomaly(
    grid: &Array2<Scalar>,
    config: &DetectionConfig,
) -> Result<DetectionResult, crate::curvelet::CurveletError> {
    use crate::internal_weights;

    // Deserialise parameters — error maps to CurveletError::FftError for now
    // (a dedicated DetectionError type can be added later without breaking the API).
    let params = internal_weights::load_params(&config.0)
        .map_err(|e| crate::curvelet::CurveletError::FftError(e.to_string()))?;

    // Run the curvelet forward pass.
    let store = crate::curvelet::curvelet_forward(grid, 4)?;

    // Compute energy ratio across all detail scales.
    let mut total_energy = 0.0_f64;
    for (scale_idx, scale_subbands) in store.detail.iter().enumerate() {
        let scale_w = params.scale_weights.get(scale_idx).copied().unwrap_or(1.0) as f64;
        for subband in scale_subbands {
            let energy: f64 = subband.iter().map(|c| c.norm_sqr() as f64).sum();
            total_energy += energy * scale_w;
        }
    }
    let fine_energy: f64 = store.fine.iter().map(|c| c.norm_sqr() as f64).sum();
    total_energy += fine_energy;

    // Normalise energy ratio.
    let n = (grid.nrows() * grid.ncols()) as f64;
    let energy_ratio = (total_energy / n.max(1.0)) as Scalar;

    // Compute phase coherence at scale 0 (finest detail).
    let coherence_map = store.phase_coherence(0, 2)
        .unwrap_or_else(|_| Array2::zeros((1, 1)));

    let (peak_row, peak_col, peak_coherence) = coherence_map
        .indexed_iter()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|((r, c), &v)| (r, c, v))
        .unwrap_or((0, 0, 0.0));

    // Composite score: weighted combination of energy ratio and phase coherence.
    let energy_score = if energy_ratio > params.dipole_energy_threshold {
        (energy_ratio / params.dipole_energy_threshold).min(1.0)
    } else {
        energy_ratio / params.dipole_energy_threshold
    };

    let coherence_score = if peak_coherence > params.phase_coherence_min {
        1.0
    } else {
        peak_coherence / params.phase_coherence_min.max(1e-6)
    };

    let score = (0.6 * energy_score as f64 + 0.4 * coherence_score as f64)
        .clamp(0.0, 1.0) as Scalar;

    Ok(DetectionResult {
        score,
        peak_row,
        peak_col,
        energy_ratio,
        phase_coherence: peak_coherence,
    })
}
