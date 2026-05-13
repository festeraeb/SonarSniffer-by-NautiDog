//! DirectionalMask trait and built-in implementations.

use crate::precision::Scalar;

/// Supplies per-angle, per-scale multiplicative weights for curvelet reconstruction.
///
/// Weights are applied **only during the inverse pass**. The forward pass stores
/// unweighted coefficients so that different masks can be applied to the same
/// `CoefficientStore` without re-running the forward pass.
///
/// # Contract
/// - `weight()` should return values in [0.0, 1.0].
/// - Values outside this range are clamped by the engine before application.
pub trait DirectionalMask: Send + Sync {
    /// Returns a multiplicative weight for the given scale and angle.
    ///
    /// # Arguments
    /// * `scale`     — scale index (0 = finest detail)
    /// * `angle_deg` — centre angle of the curvelet wedge in degrees [0, 360)
    fn weight(&self, scale: usize, angle_deg: f64) -> Scalar;
}

// ── IdentityMask ──────────────────────────────────────────────────────────────

/// Returns 1.0 for all inputs — preserves existing behaviour when no mask is supplied.
pub struct IdentityMask;

impl DirectionalMask for IdentityMask {
    #[inline]
    fn weight(&self, _scale: usize, _angle_deg: f64) -> Scalar {
        1.0
    }
}

// ── StripeSuppressor ──────────────────────────────────────────────────────────

/// Suppresses flight-line striping noise in aeromagnetic data.
///
/// Returns:
/// - `0.0` for angles within ±15° of the flight-path azimuth (suppress stripes)
/// - `1.0` for angles within ±15° of the perpendicular (amplify dipole squiggles)
/// - Linear interpolation in the transition zones between 0.0 and 1.0
pub struct StripeSuppressor {
    /// Flight-path azimuth in degrees [0, 360).
    pub flight_azimuth_deg: f64,
}

impl DirectionalMask for StripeSuppressor {
    fn weight(&self, _scale: usize, angle_deg: f64) -> Scalar {
        let azimuth = self.flight_azimuth_deg.rem_euclid(360.0);

        // Angular distance from the flight-path azimuth (wrapped to [0, 180]).
        let diff = angular_distance(angle_deg, azimuth);

        // Perpendicular is at azimuth ± 90°.
        let perp_diff = (diff - 90.0).abs();

        // Transition zone half-width: 15°.
        const HALF: f64 = 15.0;

        if diff <= HALF {
            // Within ±15° of flight path → suppress (weight 0.0).
            0.0
        } else if perp_diff <= HALF {
            // Within ±15° of perpendicular → amplify (weight 1.0).
            1.0
        } else if diff <= 90.0 - HALF {
            // Transition from suppress to amplify.
            let t = (diff - HALF) / (90.0 - 2.0 * HALF);
            smooth_step(t) as Scalar
        } else {
            // Transition from amplify back toward suppress (second half of circle).
            let t = (perp_diff - HALF) / (90.0 - 2.0 * HALF);
            (1.0 - smooth_step(t)) as Scalar
        }
    }
}

// ── ComplementMask ────────────────────────────────────────────────────────────

/// Returns `1.0 - inner.weight(scale, angle_deg)` for all inputs.
///
/// Used to verify the mask + complement = identity reconstruction invariant:
/// `reconstruct(mask) + reconstruct(complement) == reconstruct(identity)`
pub struct ComplementMask<'a> {
    pub inner: &'a dyn DirectionalMask,
}

impl<'a> DirectionalMask for ComplementMask<'a> {
    fn weight(&self, scale: usize, angle_deg: f64) -> Scalar {
        let w = self.inner.weight(scale, angle_deg).clamp(0.0, 1.0);
        1.0 - w
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Angular distance between two angles in degrees, wrapped to [0, 180].
fn angular_distance(a: f64, b: f64) -> f64 {
    let diff = (a - b).rem_euclid(360.0);
    if diff > 180.0 { 360.0 - diff } else { diff }
}

/// C∞ smooth step: 0 at t=0, 1 at t=1.
fn smooth_step(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_mask_always_one() {
        let m = IdentityMask;
        for angle in [0.0, 45.0, 90.0, 135.0, 180.0, 270.0, 359.9] {
            assert_eq!(m.weight(0, angle), 1.0);
            assert_eq!(m.weight(3, angle), 1.0);
        }
    }

    #[test]
    fn stripe_suppressor_suppresses_flight_path() {
        let m = StripeSuppressor { flight_azimuth_deg: 0.0 };
        // Exactly on flight path → 0.0
        assert_eq!(m.weight(0, 0.0), 0.0);
        assert_eq!(m.weight(0, 180.0), 0.0);
    }

    #[test]
    fn stripe_suppressor_amplifies_perpendicular() {
        let m = StripeSuppressor { flight_azimuth_deg: 0.0 };
        // Exactly perpendicular → 1.0
        assert_eq!(m.weight(0, 90.0), 1.0);
        assert_eq!(m.weight(0, 270.0), 1.0);
    }

    #[test]
    fn complement_mask_sums_to_one() {
        let inner = StripeSuppressor { flight_azimuth_deg: 45.0 };
        let comp = ComplementMask { inner: &inner };
        for angle in [0.0, 30.0, 45.0, 90.0, 135.0, 180.0, 270.0] {
            let w = inner.weight(0, angle);
            let c = comp.weight(0, angle);
            assert!(
                (w + c - 1.0).abs() < 1e-5,
                "mask + complement != 1.0 at angle {}: {} + {} = {}",
                angle, w, c, w + c
            );
        }
    }

    #[test]
    fn weights_in_range() {
        let m = StripeSuppressor { flight_azimuth_deg: 30.0 };
        for angle in (0..360).map(|i| i as f64) {
            let w = m.weight(0, angle);
            assert!(w >= 0.0 && w <= 1.0, "weight out of range at {}: {}", angle, w);
        }
    }
}
