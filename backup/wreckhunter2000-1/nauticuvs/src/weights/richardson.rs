//! Richardson Number thermal-physics weighting for SAR reconstruction.

use serde::{Deserialize, Serialize};
use crate::precision::Scalar;

/// Errors from constructing a `RichardsonProfile`.
#[derive(Debug, thiserror::Error)]
pub enum RichardsonError {
    #[error("Profile has {0} depth layers; minimum is 2")]
    TooFewLayers(usize),
    #[error("Profile has {0} depth layers; maximum is 1024")]
    TooManyLayers(usize),
    #[error("depths_m, n_squared, and shear must all have the same length")]
    MismatchedLengths,
}

/// A vertical profile of buoyancy frequency and horizontal shear at each depth layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RichardsonProfile {
    /// Depth of each layer in metres (ascending order).
    pub depths_m: Vec<f64>,
    /// Brunt–Väisälä buoyancy frequency squared N² at each layer (s⁻²).
    pub n_squared: Vec<f64>,
    /// Vertical shear of horizontal velocity ∂u/∂z at each layer (s⁻¹).
    pub shear: Vec<f64>,
}

impl RichardsonProfile {
    /// Construct a profile. Returns `Err` if layer count < 2 or > 1024, or if
    /// the three vectors have different lengths.
    pub fn new(
        depths_m: Vec<f64>,
        n_squared: Vec<f64>,
        shear: Vec<f64>,
    ) -> Result<Self, RichardsonError> {
        let n = depths_m.len();
        if n < 2 {
            return Err(RichardsonError::TooFewLayers(n));
        }
        if n > 1024 {
            return Err(RichardsonError::TooManyLayers(n));
        }
        if n_squared.len() != n || shear.len() != n {
            return Err(RichardsonError::MismatchedLengths);
        }
        Ok(Self { depths_m, n_squared, shear })
    }
}

/// Computes per-layer Richardson Numbers and maps them to reconstruction weights.
///
/// Richardson Number: `Ri = N² / (∂u/∂z)²`
/// - `Ri < 0.25` → turbulent mixing → weight 1.0 (amplify)
/// - `Ri > 1.0`  → stable stratification → weight 0.0 (suppress)
/// - `0.25 ≤ Ri ≤ 1.0` → linear interpolation
/// - `∂u/∂z == 0` → `Ri = +∞` → weight 0.0 (perfectly stable)
pub struct RichardsonWeighter {
    profile: RichardsonProfile,
    /// Pre-computed Ri values per layer. `f64::INFINITY` where shear == 0.
    ri: Vec<f64>,
}

impl RichardsonWeighter {
    /// Construct from a `RichardsonProfile`, pre-computing all Ri values.
    pub fn new(profile: RichardsonProfile) -> Self {
        let ri = profile.n_squared.iter()
            .zip(profile.shear.iter())
            .map(|(&n2, &s)| {
                if s == 0.0 {
                    f64::INFINITY
                } else {
                    n2 / (s * s)
                }
            })
            .collect();
        Self { profile, ri }
    }

    /// Returns a weight in [0.0, 1.0] for the given depth.
    ///
    /// Interpolates between the two nearest depth layers. Clamps to the
    /// profile's depth range if `depth_m` is outside it.
    pub fn weight_for_depth(&self, depth_m: f64) -> Scalar {
        let depths = &self.profile.depths_m;
        let n = depths.len();

        // Find the bracketing layers via binary search.
        let idx = depths.partition_point(|&d| d <= depth_m);

        let ri = if idx == 0 {
            // Above the shallowest layer — use the first layer's Ri.
            self.ri[0]
        } else if idx >= n {
            // Below the deepest layer — use the last layer's Ri.
            self.ri[n - 1]
        } else {
            // Interpolate between layers idx-1 and idx.
            let d0 = depths[idx - 1];
            let d1 = depths[idx];
            let t = if (d1 - d0).abs() < 1e-15 {
                0.0
            } else {
                (depth_m - d0) / (d1 - d0)
            };
            let ri0 = self.ri[idx - 1];
            let ri1 = self.ri[idx];
            // Handle infinity: if either endpoint is infinite, result is infinite.
            if ri0.is_infinite() || ri1.is_infinite() {
                f64::INFINITY
            } else {
                ri0 + t * (ri1 - ri0)
            }
        };

        ri_to_weight(ri) as Scalar
    }
}

/// Map a Richardson Number to a reconstruction weight.
///
/// - `Ri < 0.25`  → 1.0 (turbulent, amplify)
/// - `Ri > 1.0`   → 0.0 (stable, suppress)
/// - `0.25 ≤ Ri ≤ 1.0` → linear interpolation from 1.0 to 0.0
fn ri_to_weight(ri: f64) -> f64 {
    if ri.is_nan() {
        return 0.0;
    }
    if ri < 0.25 {
        1.0
    } else if ri > 1.0 {
        0.0
    } else {
        // Linear interpolation: 1.0 at Ri=0.25, 0.0 at Ri=1.0
        1.0 - (ri - 0.25) / (1.0 - 0.25)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_few_layers() {
        let err = RichardsonProfile::new(vec![0.0], vec![1.0], vec![0.5]).unwrap_err();
        assert!(matches!(err, RichardsonError::TooFewLayers(1)));
    }

    #[test]
    fn too_many_layers() {
        let n = 1025;
        let err = RichardsonProfile::new(
            vec![0.0; n], vec![1.0; n], vec![0.5; n],
        ).unwrap_err();
        assert!(matches!(err, RichardsonError::TooManyLayers(1025)));
    }

    #[test]
    fn mismatched_lengths() {
        let err = RichardsonProfile::new(
            vec![0.0, 10.0], vec![1.0], vec![0.5, 0.3],
        ).unwrap_err();
        assert!(matches!(err, RichardsonError::MismatchedLengths));
    }

    #[test]
    fn turbulent_layer_weight_one() {
        // Ri = 1.0 / (4.0^2) = 0.0625 < 0.25 → weight 1.0
        let profile = RichardsonProfile::new(
            vec![0.0, 100.0],
            vec![1.0, 1.0],
            vec![4.0, 4.0],
        ).unwrap();
        let w = RichardsonWeighter::new(profile);
        let weight = w.weight_for_depth(50.0);
        assert!((weight as f64 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn stable_layer_weight_zero() {
        // Ri = 1.0 / (0.5^2) = 4.0 > 1.0 → weight 0.0
        let profile = RichardsonProfile::new(
            vec![0.0, 100.0],
            vec![1.0, 1.0],
            vec![0.5, 0.5],
        ).unwrap();
        let w = RichardsonWeighter::new(profile);
        let weight = w.weight_for_depth(50.0);
        assert!((weight as f64).abs() < 1e-6);
    }

    #[test]
    fn zero_shear_gives_weight_zero() {
        // ∂u/∂z = 0 → Ri = +∞ → weight 0.0
        let profile = RichardsonProfile::new(
            vec![0.0, 100.0],
            vec![1.0, 1.0],
            vec![0.0, 0.0],
        ).unwrap();
        let w = RichardsonWeighter::new(profile);
        let weight = w.weight_for_depth(50.0);
        assert!((weight as f64).abs() < 1e-6);
    }

    #[test]
    fn midpoint_ri_interpolates() {
        // Ri = 0.625 (midpoint of [0.25, 1.0]) → weight ≈ 0.5
        let ri_val = 0.625_f64;
        let weight = super::ri_to_weight(ri_val);
        assert!((weight - 0.5).abs() < 1e-6, "weight={}", weight);
    }
}
