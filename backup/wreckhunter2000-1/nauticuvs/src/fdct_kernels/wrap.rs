//! Wrapping step: fold a windowed frequency-domain wedge into a canonical rectangle.

use crate::precision::C;
use num_complex::Complex;

/// Wrap a windowed frequency-domain slice to a canonical rectangle.
///
/// The wrapping step periodically folds the windowed wedge (which lives in the
/// full frequency plane) into a smaller canonical rectangle by accumulating
/// contributions from all periodic copies described by `wrap_offsets`.
///
/// # Arguments
/// * `src`         — windowed wedge, length `src_rows * src_cols`, row-major
/// * `src_rows`    — rows of the source (full FFT plane)
/// * `src_cols`    — cols of the source (full FFT plane)
/// * `dst`         — canonical rectangle output, length `dst_rows * dst_cols`
/// * `dst_rows`    — rows of the canonical rectangle
/// * `dst_cols`    — cols of the canonical rectangle
/// * `wrap_offsets` — (row_offset, col_offset) pairs; each offset describes one
///                    periodic copy of the plane that contributes to the rectangle
///
/// No heap allocation occurs in this function.
#[cfg_attr(feature = "xla", xla::kernel)]
pub fn wrap_to_canonical(
    src: &[C],
    src_rows: usize,
    src_cols: usize,
    dst: &mut [C],
    dst_rows: usize,
    dst_cols: usize,
    wrap_offsets: &[(isize, isize)],
) {
    debug_assert_eq!(src.len(), src_rows * src_cols);
    debug_assert_eq!(dst.len(), dst_rows * dst_cols);

    // Zero the destination.
    for v in dst.iter_mut() {
        *v = C::default();
    }

    for &(row_off, col_off) in wrap_offsets {
        for dst_row in 0..dst_rows {
            for dst_col in 0..dst_cols {
                // Map canonical (dst_row, dst_col) back to source coordinates.
                let src_row = (dst_row as isize + row_off)
                    .rem_euclid(src_rows as isize) as usize;
                let src_col = (dst_col as isize + col_off)
                    .rem_euclid(src_cols as isize) as usize;

                let src_idx = src_row * src_cols + src_col;
                let dst_idx = dst_row * dst_cols + dst_col;

                dst[dst_idx] = dst[dst_idx] + src[src_idx];
            }
        }
    }
}

/// Unwrap from a canonical rectangle back to the full frequency plane.
///
/// This is the adjoint of `wrap_to_canonical`, used in the inverse pass.
/// Each canonical cell scatters its value back to the corresponding source
/// location for each wrap offset.
///
/// # Arguments
/// * `src`         — canonical rectangle, length `src_rows * src_cols`
/// * `src_rows`    — rows of the canonical rectangle
/// * `src_cols`    — cols of the canonical rectangle
/// * `dst`         — full frequency plane output, length `dst_rows * dst_cols`
/// * `dst_rows`    — rows of the full frequency plane
/// * `dst_cols`    — cols of the full frequency plane
/// * `wrap_offsets` — same offsets used in the forward `wrap_to_canonical` call
///
/// No heap allocation occurs in this function.
#[cfg_attr(feature = "xla", xla::kernel)]
pub fn unwrap_from_canonical(
    src: &[C],
    src_rows: usize,
    src_cols: usize,
    dst: &mut [C],
    dst_rows: usize,
    dst_cols: usize,
    wrap_offsets: &[(isize, isize)],
) {
    debug_assert_eq!(src.len(), src_rows * src_cols);
    debug_assert_eq!(dst.len(), dst_rows * dst_cols);

    for v in dst.iter_mut() {
        *v = C::default();
    }

    for &(row_off, col_off) in wrap_offsets {
        for src_row in 0..src_rows {
            for src_col in 0..src_cols {
                let dst_row = (src_row as isize + row_off)
                    .rem_euclid(dst_rows as isize) as usize;
                let dst_col = (src_col as isize + col_off)
                    .rem_euclid(dst_cols as isize) as usize;

                let src_idx = src_row * src_cols + src_col;
                let dst_idx = dst_row * dst_cols + dst_col;

                dst[dst_idx] = dst[dst_idx] + src[src_idx];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_output_length_correct() {
        let src_rows = 8;
        let src_cols = 8;
        let dst_rows = 4;
        let dst_cols = 4;
        let src: Vec<C> = (0..src_rows * src_cols)
            .map(|i| Complex::new(i as f32, 0.0))
            .collect();
        let mut dst = vec![C::default(); dst_rows * dst_cols];
        let offsets = vec![(0isize, 0isize)];
        wrap_to_canonical(&src, src_rows, src_cols, &mut dst, dst_rows, dst_cols, &offsets);
        assert_eq!(dst.len(), dst_rows * dst_cols);
    }

    #[test]
    fn identity_offset_copies_top_left() {
        // With a single (0,0) offset, wrap should copy the top-left dst_rows×dst_cols
        // block of src into dst.
        let src_rows = 4;
        let src_cols = 4;
        let dst_rows = 2;
        let dst_cols = 2;
        let src: Vec<C> = (0..16).map(|i| Complex::new(i as f32, 0.0)).collect();
        let mut dst = vec![C::default(); 4];
        wrap_to_canonical(&src, src_rows, src_cols, &mut dst, dst_rows, dst_cols, &[(0, 0)]);
        // dst[0] should be src[0], dst[1] = src[1], dst[2] = src[4], dst[3] = src[5]
        assert!((dst[0].re - 0.0).abs() < 1e-6);
        assert!((dst[1].re - 1.0).abs() < 1e-6);
        assert!((dst[2].re - 4.0).abs() < 1e-6);
        assert!((dst[3].re - 5.0).abs() < 1e-6);
    }
}
