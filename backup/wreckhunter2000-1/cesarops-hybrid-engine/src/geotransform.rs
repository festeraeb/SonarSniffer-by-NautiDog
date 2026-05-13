//! WGS84 → ECEF → Local ENU coordinate transformation.
//!
//! Handles the transformation of raw WGS84 ellipsoidal coordinates
//! (Latitude/Longitude/Ellipsoidal Height) into a localized tangent plane
//! grid (ENU: East, North, Up) for the sub-surface signature engine.
//!
//! Routed to AVX-512 Xeon Silver cores — iterative transcendental steps
//! with sequential precision dependencies. Never cast to f32.

use rayon::prelude::*;

// ── WGS84 Ellipsoid Constants ─────────────────────────────────────────────────

/// Semi-major axis (metres) — equatorial radius of the Earth.
const WGS84_A: f64 = 6_378_137.0;

/// Flattening — defines how much the Earth is squished at the poles.
const WGS84_F: f64 = 1.0 / 298.257_223_563;

/// Semi-minor axis (metres) — polar radius.
const WGS84_B: f64 = WGS84_A * (1.0 - WGS84_F);

/// First eccentricity squared — measures deviation from a perfect sphere.
const E_SQ: f64 = (WGS84_A * WGS84_A - WGS84_B * WGS84_B) / (WGS84_A * WGS84_A);

// ── Coordinate types ──────────────────────────────────────────────────────────

/// A point on the WGS84 ellipsoid — the GPS datum.
#[derive(Debug, Clone, Copy)]
pub struct Wgs84Coord {
    /// Latitude in decimal degrees (positive = north).
    pub latitude: f64,
    /// Longitude in decimal degrees (positive = east).
    pub longitude: f64,
    /// Height above the WGS84 ellipsoid in metres.
    pub altitude: f64,
}

/// A point in the local East-North-Up tangent plane.
/// Origin is at the scan anchor point; axes are metric.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalEnuCoord {
    /// Metres east of the anchor.
    pub east: f64,
    /// Metres north of the anchor.
    pub north: f64,
    /// Metres above the anchor (positive = up).
    pub up: f64,
}

// ── Batch transform ───────────────────────────────────────────────────────────

/// Transform an entire slice of WGS84 coordinates into local ENU grid positions.
///
/// Optimised for AVX-512: processes in 8-element chunks (8 × f64 = 512 bits)
/// so the LLVM backend can unroll directly into zmm registers.
///
/// # Arguments
/// * `inputs`  — raw GPS coordinates from the GeoTIFF or sensor feed
/// * `anchor`  — the scan centre point (all ENU values are relative to this)
/// * `outputs` — pre-allocated output buffer (same length as inputs)
///
/// # Precision guarantee
/// All intermediate calculations use f64. No f32 casts occur anywhere in this
/// pipeline — catastrophic cancellation in ECEF subtraction would destroy
/// sub-metre accuracy if we allowed precision loss.
pub fn batch_wgs84_to_local_enu(
    inputs: &[Wgs84Coord],
    anchor: Wgs84Coord,
    outputs: &mut [LocalEnuCoord],
) {
    assert_eq!(inputs.len(), outputs.len(), "Input and output slices must match");

    // Step 1: Compute the anchor point in ECEF (Earth-Centered, Earth-Fixed).
    let (ax, ay, az) = wgs84_to_ecef(anchor);

    // Step 2: Pre-compute rotation matrix elements from the anchor's lat/lon.
    let lat_rad = anchor.latitude.to_radians();
    let lon_rad = anchor.longitude.to_radians();

    let sin_lat = lat_rad.sin();
    let cos_lat = lat_rad.cos();
    let sin_lon = lon_rad.sin();
    let cos_lon = lon_rad.cos();

    // Step 3: Parallel transform — 8-element chunks for AVX-512 alignment.
    // Each chunk fills one zmm register (8 × 64-bit = 512 bits).
    outputs
        .par_chunks_mut(8)
        .zip(inputs.par_chunks(8))
        .for_each(|(out_chunk, in_chunk)| {
            for i in 0..out_chunk.len().min(in_chunk.len()) {
                let (x, y, z) = wgs84_to_ecef(in_chunk[i]);

                // ECEF delta from anchor — this is where precision matters most.
                // The subtraction of two large numbers (6.3M metres) to get a small
                // delta (metres) is catastrophic cancellation if done in f32.
                let dx = x - ax;
                let dy = y - ay;
                let dz = z - az;

                // Rotation matrix: ECEF deltas → Local Tangent Plane (ENU).
                // This is a 3×3 orthogonal rotation — no precision loss.
                out_chunk[i].east  = -sin_lon * dx + cos_lon * dy;
                out_chunk[i].north = -sin_lat * cos_lon * dx - sin_lat * sin_lon * dy + cos_lat * dz;
                out_chunk[i].up    =  cos_lat * cos_lon * dx + cos_lat * sin_lon * dy + sin_lat * dz;
            }
        });
}

/// Transform a single WGS84 coordinate to local ENU relative to an anchor.
pub fn wgs84_to_local_enu(coord: Wgs84Coord, anchor: Wgs84Coord) -> LocalEnuCoord {
    let mut out = [LocalEnuCoord::default()];
    batch_wgs84_to_local_enu(&[coord], anchor, &mut out);
    out[0]
}

/// Inverse: local ENU back to WGS84 (approximate, for small offsets < 100km).
pub fn local_enu_to_wgs84(enu: LocalEnuCoord, anchor: Wgs84Coord) -> Wgs84Coord {
    let lat_rad = anchor.latitude.to_radians();
    let lon_rad = anchor.longitude.to_radians();

    // Approximate: for small ENU offsets, use the local radius of curvature.
    let sin_lat = lat_rad.sin();
    let n = WGS84_A / (1.0 - E_SQ * sin_lat * sin_lat).sqrt();
    let m = WGS84_A * (1.0 - E_SQ) / (1.0 - E_SQ * sin_lat * sin_lat).powf(1.5);

    let dlat = enu.north / m;
    let dlon = enu.east / (n * lat_rad.cos());
    let dalt = enu.up;

    Wgs84Coord {
        latitude:  anchor.latitude + dlat.to_degrees(),
        longitude: anchor.longitude + dlon.to_degrees(),
        altitude:  anchor.altitude + dalt,
    }
}

// ── ECEF conversion ───────────────────────────────────────────────────────────

/// Transform a WGS84 coordinate to Earth-Centered, Earth-Fixed (ECEF) Cartesian.
///
/// This is the core geodetic calculation — all values remain f64 throughout.
/// The radius of curvature `N` involves a square root of a near-unity value,
/// which is where f32 would introduce ~1m error at mid-latitudes.
#[inline(always)]
fn wgs84_to_ecef(coord: Wgs84Coord) -> (f64, f64, f64) {
    let lat = coord.latitude.to_radians();
    let lon = coord.longitude.to_radians();

    let sin_lat = lat.sin();
    let cos_lat = lat.cos();

    // Radius of curvature in the prime vertical.
    // N ≈ 6.38M metres — the subtraction (1 - e²sin²φ) is near 1.0,
    // so the sqrt is well-conditioned. But the final multiplication
    // with altitude requires full f64 to preserve sub-metre accuracy.
    let n = WGS84_A / (1.0 - E_SQ * sin_lat * sin_lat).sqrt();

    let x = (n + coord.altitude) * cos_lat * lon.cos();
    let y = (n + coord.altitude) * cos_lat * lon.sin();
    let z = (n * (1.0 - E_SQ) + coord.altitude) * sin_lat;

    (x, y, z)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecef_roundtrip_origin() {
        // Equator, prime meridian, sea level.
        let coord = Wgs84Coord { latitude: 0.0, longitude: 0.0, altitude: 0.0 };
        let (x, y, z) = wgs84_to_ecef(coord);
        // At equator/prime meridian: x ≈ WGS84_A, y ≈ 0, z ≈ 0
        assert!((x - WGS84_A).abs() < 1.0, "x={}", x);
        assert!(y.abs() < 1.0, "y={}", y);
        assert!(z.abs() < 1.0, "z={}", z);
    }

    #[test]
    fn enu_at_anchor_is_zero() {
        let anchor = Wgs84Coord { latitude: 42.5, longitude: -83.5, altitude: 0.0 };
        let enu = wgs84_to_local_enu(anchor, anchor);
        assert!(enu.east.abs() < 1e-6, "east={}", enu.east);
        assert!(enu.north.abs() < 1e-6, "north={}", enu.north);
        assert!(enu.up.abs() < 1e-6, "up={}", enu.up);
    }

    #[test]
    fn enu_north_offset() {
        // 1 degree north ≈ 111km
        let anchor = Wgs84Coord { latitude: 42.0, longitude: -83.0, altitude: 0.0 };
        let point  = Wgs84Coord { latitude: 43.0, longitude: -83.0, altitude: 0.0 };
        let enu = wgs84_to_local_enu(point, anchor);
        // North should be ~111km, east should be ~0
        assert!((enu.north - 111_000.0).abs() < 500.0, "north={}", enu.north);
        assert!(enu.east.abs() < 100.0, "east={}", enu.east);
    }

    #[test]
    fn batch_matches_single() {
        let anchor = Wgs84Coord { latitude: 42.5, longitude: -83.5, altitude: 0.0 };
        let inputs = vec![
            Wgs84Coord { latitude: 42.51, longitude: -83.49, altitude: 10.0 },
            Wgs84Coord { latitude: 42.52, longitude: -83.48, altitude: 20.0 },
        ];
        let mut batch_out = vec![LocalEnuCoord::default(); 2];
        batch_wgs84_to_local_enu(&inputs, anchor, &mut batch_out);

        for (i, input) in inputs.iter().enumerate() {
            let single = wgs84_to_local_enu(*input, anchor);
            assert!((batch_out[i].east - single.east).abs() < 1e-9);
            assert!((batch_out[i].north - single.north).abs() < 1e-9);
            assert!((batch_out[i].up - single.up).abs() < 1e-9);
        }
    }

    #[test]
    fn inverse_roundtrip() {
        let anchor = Wgs84Coord { latitude: 42.5, longitude: -83.5, altitude: 0.0 };
        let point  = Wgs84Coord { latitude: 42.501, longitude: -83.499, altitude: 5.0 };
        let enu = wgs84_to_local_enu(point, anchor);
        let recovered = local_enu_to_wgs84(enu, anchor);
        // Should recover within ~1m accuracy for small offsets
        assert!((recovered.latitude - point.latitude).abs() < 1e-5,
            "lat error: {}", (recovered.latitude - point.latitude).abs());
        assert!((recovered.longitude - point.longitude).abs() < 1e-5,
            "lon error: {}", (recovered.longitude - point.longitude).abs());
    }
}
