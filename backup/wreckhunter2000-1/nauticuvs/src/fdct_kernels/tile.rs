//! Tiling step: 2-D FFT/IFFT of canonical rectangles.

use crate::fft_backend::FftBackend;
use crate::precision::C;

/// Apply 2-D IFFT to a wrapped canonical rectangle to produce spatial coefficients.
///
/// The caller provides a scratch buffer of the same length as `src` to avoid
/// heap allocation in the hot path.
///
/// # Arguments
/// * `src`        — wrapped canonical rectangle, length `rows * cols`, row-major
/// * `rows`       — number of rows
/// * `cols`       — number of columns
/// * `dst`        — output spatial coefficients, length `rows * cols`
/// * `fft_scratch` — caller-provided scratch buffer, length `rows * cols`
///
/// No heap allocation occurs in this function.
#[cfg_attr(feature = "xla", xla::kernel)]
pub fn ifft_tile(
    src: &[C],
    rows: usize,
    cols: usize,
    dst: &mut [C],
    fft_scratch: &mut [C],
) {
    debug_assert_eq!(src.len(), rows * cols);
    debug_assert_eq!(dst.len(), rows * cols);
    debug_assert_eq!(fft_scratch.len(), rows * cols);

    use crate::fft_backend::FftBackend as _;
    crate::fft_backend::ActiveBackend::ifft2d(src, rows, cols, dst);
    let _ = fft_scratch;
}

/// Apply 2-D FFT to spatial coefficients (used in the inverse pass adjoint).
///
/// # Arguments
/// * `src`        — spatial coefficients, length `rows * cols`, row-major
/// * `rows`       — number of rows
/// * `cols`       — number of columns
/// * `dst`        — output frequency-domain data, length `rows * cols`
/// * `fft_scratch` — caller-provided scratch buffer, length `rows * cols`
///
/// No heap allocation occurs in this function.
#[cfg_attr(feature = "xla", xla::kernel)]
pub fn fft_tile(
    src: &[C],
    rows: usize,
    cols: usize,
    dst: &mut [C],
    fft_scratch: &mut [C],
) {
    debug_assert_eq!(src.len(), rows * cols);
    debug_assert_eq!(dst.len(), rows * cols);
    debug_assert_eq!(fft_scratch.len(), rows * cols);

    use crate::fft_backend::FftBackend as _;
    crate::fft_backend::ActiveBackend::fft2d(src, rows, cols, dst);
    let _ = fft_scratch;
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex;
    use crate::precision::Scalar;

    #[test]
    fn fft_ifft_roundtrip() {
        let rows = 4;
        let cols = 4;
        let n = rows * cols;
        let input: Vec<C> = (0..n)
            .map(|i| Complex::new(i as Scalar, 0.0))
            .collect();

        let mut freq = vec![C::default(); n];
        let mut scratch = vec![C::default(); n];
        fft_tile(&input, rows, cols, &mut freq, &mut scratch);

        let mut recovered = vec![C::default(); n];
        ifft_tile(&freq, rows, cols, &mut recovered, &mut scratch);

        for (orig, rec) in input.iter().zip(recovered.iter()) {
            let diff = (orig.re - rec.re).abs();
            assert!(diff < 1e-4, "round-trip error: {}", diff);
        }
    }
}
