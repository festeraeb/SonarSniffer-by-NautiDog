//! Backward compatibility smoke test.
//!
//! Reproduces the exact call pattern from `cesarops-aeromagnetic-worker/src/main.rs`.
//! If this test fails to COMPILE, the aeromagnetic worker is broken.
//! If this test fails to RUN, the transform produces incorrect results.
//!
//! Requirements: 9.1, 9.2, 9.4

use nauticuvs::{curvelet_forward, Scalar};
use ndarray::Array2;

/// Exact call pattern from the aeromagnetic worker.
/// This test is a compile-time guard — field name changes break it immediately.
#[test]
fn aeromagnetic_worker_call_pattern_compiles_and_runs() {
    // Exact code from cesarops-aeromagnetic-worker/src/main.rs:
    let window = Array2::<Scalar>::zeros((64, 64));
    let coeffs = curvelet_forward(&window, 4).unwrap();

    // Access detail subbands — field name `detail` is frozen.
    let detail_energy: f64 = coeffs.detail[0][0]
        .iter()
        .map(|c| c.norm_sqr() as f64)
        .sum();

    // Access fine coefficients — field name `fine` is frozen.
    let fine_energy: f64 = coeffs.fine
        .iter()
        .map(|c| c.norm_sqr() as f64)
        .sum();

    // Both should be finite (not NaN or Inf).
    assert!(detail_energy.is_finite(), "detail energy is not finite: {}", detail_energy);
    assert!(fine_energy.is_finite(), "fine energy is not finite: {}", fine_energy);

    // num_scales should match the requested scale count.
    assert_eq!(coeffs.num_scales, 4);

    // detail should have 4 scales.
    assert_eq!(coeffs.detail.len(), 4, "expected 4 detail scales");

    // Each scale should have at least one subband.
    for (s, scale_subbands) in coeffs.detail.iter().enumerate() {
        assert!(!scale_subbands.is_empty(), "scale {} has no subbands", s);
    }
}

/// Verify the Scalar type alias is accessible and matches f32 by default.
#[test]
fn scalar_type_alias_is_f32_by_default() {
    use nauticuvs::Scalar;
    // With no feature flags, Scalar should be f32.
    assert_eq!(std::mem::size_of::<Scalar>(), 4, "Scalar should be f32 (4 bytes) by default");
}

/// Verify CoefficientStore fields are accessible with the expected types.
#[test]
fn coefficient_store_field_types() {
    use nauticuvs::Scalar;
    use num_complex::Complex;

    let window = Array2::<Scalar>::zeros((32, 32));
    let coeffs = curvelet_forward(&window, 2).unwrap();

    // detail: Vec<Vec<Subband>>
    let _: &Vec<Vec<nauticuvs::Subband>> = &coeffs.detail;

    // fine: Vec<Complex<Scalar>>
    let _: &Vec<Complex<Scalar>> = &coeffs.fine;

    // geo_transform: Option<GeoTransform>
    let _: &Option<nauticuvs::GeoTransform> = &coeffs.geo_transform;
    assert!(coeffs.geo_transform.is_none());
}

/// Verify Subband buffers are 64-byte aligned.
#[test]
fn subband_buffers_are_aligned() {
    let window = Array2::<Scalar>::zeros((32, 32));
    let coeffs = curvelet_forward(&window, 2).unwrap();

    for (s, scale_subbands) in coeffs.detail.iter().enumerate() {
        for (a, sub) in scale_subbands.iter().enumerate() {
            assert!(
                sub.is_aligned(),
                "Subband at scale={} angle={} is not 64-byte aligned",
                s, a
            );
        }
    }
}
