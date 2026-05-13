//! Internal weights integration test.
//!
//! Verifies that the detection parameters produce a score > 0.5 on a synthetic
//! dipole grid, without exposing the parameter values in test output.
//!
//! Requirement 8.6

use nauticuvs::{detect_anomaly, DetectionConfig, Scalar};
use ndarray::Array2;

/// Build a synthetic dipole grid: a positive lobe and a negative lobe
/// separated by ~10 pixels, embedded in a flat background.
fn synthetic_dipole_grid(rows: usize, cols: usize) -> Array2<Scalar> {
    let mut grid = Array2::<Scalar>::zeros((rows, cols));
    let cx = cols / 2;
    let cy = rows / 2;
    // Positive lobe
    grid[[cy - 5, cx]] = 150.0;
    // Negative lobe
    grid[[cy + 5, cx]] = -100.0;
    // Weak background noise
    for r in 0..rows {
        for c in 0..cols {
            grid[[r, c]] += ((r + c) % 7) as f32 * 0.1;
        }
    }
    grid
}

#[test]
fn dipole_detection_score_above_threshold() {
    let config = DetectionConfig::from_bytes(
        include_bytes!("test_fixtures/test_params.bin"),
    );
    let grid = synthetic_dipole_grid(64, 64);
    let result = detect_anomaly(&grid, &config).expect("detect_anomaly failed");

    // Score should be > 0.5 for a clear synthetic dipole.
    assert!(
        result.score > 0.5,
        "Expected score > 0.5 for synthetic dipole, got {}",
        result.score
    );

    // Peak location should be finite.
    assert!(result.energy_ratio.is_finite());
    assert!(result.phase_coherence.is_finite());
}

#[test]
fn detection_config_accepts_arbitrary_bytes() {
    // DetectionConfig::from_bytes must never panic on arbitrary input.
    let config = DetectionConfig::from_bytes(&[0u8; 0]);
    let grid = Array2::<Scalar>::zeros((8, 8));
    // Should return an error (blob too short), not panic.
    let result = detect_anomaly(&grid, &config);
    assert!(result.is_err(), "Expected error for empty config blob");
}
