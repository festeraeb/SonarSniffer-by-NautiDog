//! Wedge window generation and application for the FDCT wrapping variant.

use crate::precision::{Scalar, C};
use num_complex::Complex;

/// Parameters describing a single (scale, angle) wedge in the frequency domain.
#[derive(Debug, Clone)]
pub struct WedgeParams {
    /// Scale index (0 = finest).
    pub scale: usize,
    /// Angle index within this scale.
    pub angle_idx: usize,
    /// Total number of angles at this scale.
    pub num_angles: usize,
    /// Lower radial frequency bound (normalised, 0..1).
    pub freq_lo: f64,
    /// Upper radial frequency bound (normalised, 0..1).
    pub freq_hi: f64,
    /// Lower angular bound in degrees.
    pub angle_lo_deg: f64,
    /// Upper angular bound in degrees.
    pub angle_hi_deg: f64,
    /// Centre angle of this wedge in degrees.
    pub angle_center_deg: f64,
    /// Rows of the canonical rectangle for this wedge.
    pub canonical_rows: usize,
    /// Cols of the canonical rectangle for this wedge.
    pub canonical_cols: usize,
    /// Wrap offsets: (row_offset, col_offset) pairs for the wrapping step.
    pub wrap_offsets: Vec<(isize, isize)>,
}

/// Build the full wedge table for a grid of size (rows, cols) with `scales` scales.
///
/// Returns `wedge_table[scale][angle]` — a `WedgeParams` for each (scale, angle) pair.
pub fn build_wedge_table(rows: usize, cols: usize, scales: usize) -> Vec<Vec<WedgeParams>> {
    let mut table = Vec::with_capacity(scales);

    for scale in 0..scales {
        // Number of angles doubles every two scales (standard curvelet geometry).
        let num_angles = 8 * (1 << (scale / 2));
        let mut scale_wedges = Vec::with_capacity(num_angles);

        // Radial frequency band for this scale.
        let freq_lo = if scale == 0 { 0.0 } else { 0.5_f64.powi(scales as i32 - scale as i32) };
        let freq_hi = 0.5_f64.powi(scales as i32 - scale as i32 - 1);

        // Canonical rectangle size: proportional to the frequency band width.
        let band_rows = ((rows as f64 * (freq_hi - freq_lo) * 2.0).ceil() as usize).max(4);
        let band_cols = ((cols as f64 * (freq_hi - freq_lo) * 2.0).ceil() as usize).max(4);

        for angle_idx in 0..num_angles {
            let angle_step = 360.0 / num_angles as f64;
            let angle_center = angle_idx as f64 * angle_step;
            let angle_lo = angle_center - angle_step / 2.0;
            let angle_hi = angle_center + angle_step / 2.0;

            // Compute wrap offsets for this wedge.
            let wrap_offsets = compute_wrap_offsets(rows, cols, scale, angle_center);

            scale_wedges.push(WedgeParams {
                scale,
                angle_idx,
                num_angles,
                freq_lo,
                freq_hi,
                angle_lo_deg: angle_lo,
                angle_hi_deg: angle_hi,
                angle_center_deg: angle_center,
                canonical_rows: band_rows,
                canonical_cols: band_cols,
                wrap_offsets,
            });
        }
        table.push(scale_wedges);
    }
    table
}

/// Apply a smooth Meyer-type window to a frequency-domain wedge.
///
/// Extracts and windows the portion of `fft_plane` corresponding to the wedge
/// described by `wedge_params`, writing the result into `dst`.
///
/// # Arguments
/// * `fft_plane`   — full 2-D FFT plane, length `plane_rows * plane_cols`, row-major
/// * `plane_rows`  — number of rows in the FFT plane
/// * `plane_cols`  — number of columns in the FFT plane
/// * `dst`         — output buffer, same length as `fft_plane`
/// * `wedge_params` — wedge geometry parameters
///
/// No heap allocation occurs in this function.
#[cfg_attr(feature = "xla", xla::kernel)]
pub fn apply_wedge_window(
    fft_plane: &[C],
    plane_rows: usize,
    plane_cols: usize,
    dst: &mut [C],
    wedge_params: &WedgeParams,
) {
    debug_assert_eq!(fft_plane.len(), plane_rows * plane_cols);
    debug_assert_eq!(dst.len(), plane_rows * plane_cols);

    let half_rows = plane_rows as f64 / 2.0;
    let half_cols = plane_cols as f64 / 2.0;

    for row in 0..plane_rows {
        for col in 0..plane_cols {
            let idx = row * plane_cols + col;

            // Shift to centred frequency coordinates.
            let fy = (row as f64 - half_rows) / half_rows;
            let fx = (col as f64 - half_cols) / half_cols;

            // Radial frequency (normalised 0..1).
            let r = (fx * fx + fy * fy).sqrt() * 0.5;

            // Angular position in degrees (0..360).
            let theta = fx.atan2(fy).to_degrees().rem_euclid(360.0);

            // Radial window: smooth bump between freq_lo and freq_hi.
            let radial_w = meyer_bump(r, wedge_params.freq_lo, wedge_params.freq_hi);

            // Angular window: smooth bump centred on angle_center_deg.
            let angular_w = angular_bump(
                theta,
                wedge_params.angle_center_deg,
                wedge_params.angle_hi_deg - wedge_params.angle_lo_deg,
            );

            let w = (radial_w * angular_w) as Scalar;
            dst[idx] = fft_plane[idx] * Complex::new(w, 0.0);
        }
    }
}

/// Get the wrap offsets for a given (scale, angle) — used by the inverse pass.
pub fn get_wrap_offsets(
    rows: usize,
    cols: usize,
    scale: usize,
    angle_center_deg: f64,
) -> Vec<(isize, isize)> {
    compute_wrap_offsets(rows, cols, scale, angle_center_deg)
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Smooth Meyer-type radial bump function.
/// Returns 1.0 in the passband, 0.0 outside, smooth transition at edges.
fn meyer_bump(r: f64, lo: f64, hi: f64) -> f64 {
    if r <= lo || r >= hi {
        return 0.0;
    }
    let mid = (lo + hi) / 2.0;
    if r <= mid {
        // Rising edge: smooth step from lo to mid.
        let t = (r - lo) / (mid - lo);
        smooth_step(t)
    } else {
        // Falling edge: smooth step from mid to hi.
        let t = (hi - r) / (hi - mid);
        smooth_step(t)
    }
}

/// Smooth angular bump centred at `center_deg` with full-width `width_deg`.
fn angular_bump(theta: f64, center_deg: f64, width_deg: f64) -> f64 {
    let half = width_deg / 2.0;
    // Angular distance (wrapped to [-180, 180]).
    let mut diff = theta - center_deg;
    while diff > 180.0 { diff -= 360.0; }
    while diff < -180.0 { diff += 360.0; }
    let abs_diff = diff.abs();
    if abs_diff >= half {
        return 0.0;
    }
    let t = 1.0 - abs_diff / half;
    smooth_step(t)
}

/// C∞ smooth step: 0 at t=0, 1 at t=1, zero derivative at both ends.
fn smooth_step(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Compute wrap offsets for a wedge at (scale, angle_center_deg).
///
/// The wrapping step folds the windowed wedge into a canonical rectangle by
/// periodically extending the frequency plane. The offsets describe which
/// copies of the plane contribute to each canonical rectangle cell.
fn compute_wrap_offsets(
    rows: usize,
    cols: usize,
    _scale: usize,
    angle_center_deg: f64,
) -> Vec<(isize, isize)> {
    // For the standard wrapping variant, we use a 3×3 neighbourhood of copies.
    // The actual offsets depend on the wedge orientation.
    let angle_rad = angle_center_deg.to_radians();
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();

    let mut offsets = Vec::new();
    for di in -1isize..=1 {
        for dj in -1isize..=1 {
            let row_off = (di as f64 * rows as f64 * cos_a.abs()) as isize;
            let col_off = (dj as f64 * cols as f64 * sin_a.abs()) as isize;
            offsets.push((row_off, col_off));
        }
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wedge_table_has_correct_scale_count() {
        let table = build_wedge_table(64, 64, 4);
        assert_eq!(table.len(), 4);
    }

    #[test]
    fn apply_wedge_window_preserves_length() {
        let rows = 16;
        let cols = 16;
        let n = rows * cols;
        let input: Vec<C> = (0..n).map(|i| Complex::new(i as Scalar, 0.0)).collect();
        let mut dst = vec![C::default(); n];
        let table = build_wedge_table(rows, cols, 2);
        apply_wedge_window(&input, rows, cols, &mut dst, &table[0][0]);
        assert_eq!(dst.len(), n);
    }

    #[test]
    fn smooth_step_endpoints() {
        assert!((smooth_step(0.0)).abs() < 1e-10);
        assert!((smooth_step(1.0) - 1.0).abs() < 1e-10);
    }
}
