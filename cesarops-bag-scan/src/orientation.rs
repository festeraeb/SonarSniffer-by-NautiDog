//! PCA orientation: heading / length / width + compass bearing + 180 ambiguity.
//!
//! The PCA principal-axis math is lifted from `bag_mesh.rs::pca_axis`
//! (exposed via [`crate::grid::pca_axis_from_coords`]). The compass-bearing
//! conversion is ported from `calc_orientation.py`:
//!
//! ```python
//! # array Y goes down, X goes right.
//! dx = principal_axis[0]
//! dy = -principal_axis[1]   # flip Y since array goes down
//! compass_rad = math.atan2(dx, dy)
//! compass_bearing = math.degrees(compass_rad) % 360
//! # reported as: bearing / (bearing + 180) % 360   (the 180-deg ambiguity)
//! ```
//!
//! Here the PCA eigenvector is computed in (row, col) space. We map it to the
//! same (x=col, y=row) convention `calc_orientation.py` uses before applying
//! the compass formula.

use crate::grid::pca_axis_from_coords;
use crate::types::WreckCandidate;

/// Orientation result for a cluster.
#[derive(Debug, Clone, Copy)]
pub struct Orientation {
    /// True-north compass bearing of the principal axis, degrees [0, 360).
    pub compass_bearing_deg: f64,
    /// The 180-degree ambiguity partner, degrees [0, 360).
    pub compass_bearing_alt_deg: f64,
    /// Length along the principal axis, meters.
    pub length_m: f64,
    /// Width along the minor axis, meters.
    pub width_m: f64,
}

/// Compute orientation from a cluster's (row, col) pixels and the cell size.
pub fn orientation_from_pixels(pixels: &[(usize, usize)], cell_size_m: f64) -> Orientation {
    let coords: Vec<(f64, f64)> = pixels.iter().map(|&(r, c)| (r as f64, c as f64)).collect();
    let (_heading_rc, length_m, width_m) = pca_axis_from_coords(&coords, cell_size_m);

    // Recover the principal eigenvector in (row, col) space to apply the exact
    // calc_orientation.py compass formula (which needs the vector, not the
    // bag_mesh heading convention).
    let (er, ec) = principal_eigenvector_rowcol(&coords);
    // calc_orientation.py works in (x=col, y=row):
    //   principal_axis[0] = x-component = ec (col)
    //   principal_axis[1] = y-component = er (row)
    let dx = ec;
    let dy = -er; // flip Y since array rows go down
    let compass_bearing = dx.atan2(dy).to_degrees().rem_euclid(360.0);
    let compass_bearing_alt = (compass_bearing + 180.0).rem_euclid(360.0);

    Orientation {
        compass_bearing_deg: compass_bearing,
        compass_bearing_alt_deg: compass_bearing_alt,
        length_m,
        width_m,
    }
}

/// Fill a candidate's heading fields in place from its (re-derivable) extent.
/// Used by the pipeline after anomaly detection; the candidate already carries
/// length/width, so this only sets the compass heading + ambiguity.
pub fn annotate_candidate(cand: &mut WreckCandidate, pixels: &[(usize, usize)], cell_size_m: f64) {
    let o = orientation_from_pixels(pixels, cell_size_m);
    cand.heading_deg = o.compass_bearing_deg;
    cand.heading_alt_deg = o.compass_bearing_alt_deg;
    // If PCA gives a more precise length/width than the bbox, prefer it.
    if o.length_m > 0.0 {
        cand.length_m = o.length_m;
        cand.width_m = o.width_m;
    }
}

/// Principal eigenvector of the (row, col) covariance, returned as (er, ec).
/// Mirrors the eigenvector selection in `bag_mesh.rs::pca_axis`.
fn principal_eigenvector_rowcol(coords: &[(f64, f64)]) -> (f64, f64) {
    if coords.len() < 2 {
        return (1.0, 0.0);
    }
    let n = coords.len() as f64;
    let mean_r: f64 = coords.iter().map(|c| c.0).sum::<f64>() / n;
    let mean_c: f64 = coords.iter().map(|c| c.1).sum::<f64>() / n;

    let mut cov_rr = 0.0;
    let mut cov_rc = 0.0;
    let mut cov_cc = 0.0;
    for &(r, c) in coords {
        let dr = r - mean_r;
        let dc = c - mean_c;
        cov_rr += dr * dr;
        cov_rc += dr * dc;
        cov_cc += dc * dc;
    }
    cov_rr /= n;
    cov_rc /= n;
    cov_cc /= n;

    let trace = cov_rr + cov_cc;
    let det = cov_rr * cov_cc - cov_rc * cov_rc;
    let disc = (trace * trace / 4.0 - det).max(0.0).sqrt();
    let lambda1 = trace / 2.0 + disc;

    if cov_rc.abs() > 1e-10 {
        (lambda1 - cov_cc, cov_rc)
    } else if cov_rr >= cov_cc {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_bar_points_east_west() {
        // A bar extending along columns (east-west). In array space this is
        // along +X (col). Compass: dx=1, dy=0 -> atan2(1,0)=90 deg (East).
        let mut pix = Vec::new();
        for c in 0..30 {
            pix.push((10usize, c));
        }
        let o = orientation_from_pixels(&pix, 1.0);
        // East-west bar -> ~90 or ~270 (ambiguity).
        let near90 = (o.compass_bearing_deg - 90.0).abs() < 10.0
            || (o.compass_bearing_deg - 270.0).abs() < 10.0;
        assert!(near90, "bearing={}", o.compass_bearing_deg);
        // 180-degree ambiguity partner.
        assert!(
            ((o.compass_bearing_deg - o.compass_bearing_alt_deg).abs() - 180.0).abs() < 1e-6
        );
        assert!(o.length_m > o.width_m);
    }

    #[test]
    fn vertical_bar_points_north_south() {
        // A bar extending along rows (north-south in array, rows go down).
        let mut pix = Vec::new();
        for r in 0..30 {
            pix.push((r, 10usize));
        }
        let o = orientation_from_pixels(&pix, 1.0);
        // North-south bar -> ~0/360 or ~180.
        let near_ns = o.compass_bearing_deg.abs() < 10.0
            || (o.compass_bearing_deg - 180.0).abs() < 10.0
            || (o.compass_bearing_deg - 360.0).abs() < 10.0;
        assert!(near_ns, "bearing={}", o.compass_bearing_deg);
        assert!(o.length_m > o.width_m);
    }
}
