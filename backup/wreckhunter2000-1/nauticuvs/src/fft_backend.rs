//! FFT backend abstraction.
//!
//! Selects between `rustfft` (pure Rust, default) and `fftw` (C binding, optional)
//! at compile time via Cargo feature flags.

use crate::precision::{Scalar, C};
use rustfft::FftPlanner;

// ── Backend trait ─────────────────────────────────────────────────────────────

/// Internal FFT backend interface.
pub(crate) trait FftBackend {
    /// 2-D forward FFT. `input` and `output` are row-major, length `rows * cols`.
    fn fft2d(input: &[C], rows: usize, cols: usize, output: &mut [C]);
    /// 2-D inverse FFT (normalised). `input` and `output` are row-major.
    fn ifft2d(input: &[C], rows: usize, cols: usize, output: &mut [C]);
}

// ── Active backend selection ──────────────────────────────────────────────────

#[cfg(feature = "fftw")]
pub(crate) type ActiveBackend = FftwBackend;

#[cfg(not(feature = "fftw"))]
pub(crate) type ActiveBackend = RustFftBackend;

// ── RustFFT backend ───────────────────────────────────────────────────────────

/// Pure-Rust FFT backend using the `rustfft` crate.
///
/// Supports both `f32` and `f64` via the `FftNum` trait. No C dependencies.
pub(crate) struct RustFftBackend;

impl FftBackend for RustFftBackend {
    fn fft2d(input: &[C], rows: usize, cols: usize, output: &mut [C]) {
        debug_assert_eq!(input.len(), rows * cols);
        debug_assert_eq!(output.len(), rows * cols);
        output.copy_from_slice(input);
        fft2d_inplace(output, rows, cols, false);
    }

    fn ifft2d(input: &[C], rows: usize, cols: usize, output: &mut [C]) {
        debug_assert_eq!(input.len(), rows * cols);
        debug_assert_eq!(output.len(), rows * cols);
        output.copy_from_slice(input);
        fft2d_inplace(output, rows, cols, true);
        let norm = 1.0 / (rows * cols) as Scalar;
        for v in output.iter_mut() {
            v.re *= norm;
            v.im *= norm;
        }
    }
}

/// Perform an in-place 2-D FFT (or IFFT) using row-column decomposition.
fn fft2d_inplace(data: &mut [C], rows: usize, cols: usize, inverse: bool) {
    let mut planner: FftPlanner<Scalar> = FftPlanner::new();

    // Row-wise FFT.
    let row_fft = if inverse {
        planner.plan_fft_inverse(cols)
    } else {
        planner.plan_fft_forward(cols)
    };
    let mut row_scratch = vec![C::default(); row_fft.get_inplace_scratch_len()];
    for row in 0..rows {
        let start = row * cols;
        row_fft.process_with_scratch(&mut data[start..start + cols], &mut row_scratch);
    }

    // Column-wise FFT (transpose → row FFT → transpose).
    let col_fft = if inverse {
        planner.plan_fft_inverse(rows)
    } else {
        planner.plan_fft_forward(rows)
    };
    let mut col_scratch = vec![C::default(); col_fft.get_inplace_scratch_len()];

    // Extract each column, FFT it, write back.
    let mut col_buf = vec![C::default(); rows];
    for col in 0..cols {
        for row in 0..rows {
            col_buf[row] = data[row * cols + col];
        }
        col_fft.process_with_scratch(&mut col_buf, &mut col_scratch);
        for row in 0..rows {
            data[row * cols + col] = col_buf[row];
        }
    }
}

// ── FFTW backend (optional) ───────────────────────────────────────────────────

#[cfg(feature = "fftw")]
pub(crate) struct FftwBackend;

#[cfg(feature = "fftw")]
impl FftBackend for FftwBackend {
    fn fft2d(input: &[C], rows: usize, cols: usize, output: &mut [C]) {
        // FFTW3 binding — requires the fftw crate and FFTW3 system library.
        // Placeholder: delegate to RustFftBackend until fftw bindings are wired.
        RustFftBackend::fft2d(input, rows, cols, output);
    }

    fn ifft2d(input: &[C], rows: usize, cols: usize, output: &mut [C]) {
        RustFftBackend::ifft2d(input, rows, cols, output);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex;

    #[test]
    fn fft_ifft_roundtrip_4x4() {
        use super::FftBackend as _;
        let rows = 4;
        let cols = 4;
        let n = rows * cols;
        let input: Vec<C> = (0..n)
            .map(|i| Complex::new(i as Scalar, 0.0))
            .collect();

        let mut freq = vec![C::default(); n];
        ActiveBackend::fft2d(&input, rows, cols, &mut freq);

        let mut recovered = vec![C::default(); n];
        ActiveBackend::ifft2d(&freq, rows, cols, &mut recovered);

        for (orig, rec) in input.iter().zip(recovered.iter()) {
            let diff = (orig.re - rec.re).abs();
            assert!(diff < 1e-4, "round-trip error at element: {}", diff);
        }
    }

    #[test]
    fn dc_component_is_sum() {
        use super::FftBackend as _;
        let rows = 4;
        let cols = 4;
        let n = rows * cols;
        let val = 2.0 as Scalar;
        let input: Vec<C> = vec![num_complex::Complex::new(val, 0.0); n];
        let mut freq = vec![C::default(); n];
        ActiveBackend::fft2d(&input, rows, cols, &mut freq);
        let expected_dc = val * n as Scalar;
        assert!(
            (freq[0].re - expected_dc).abs() < 1e-3,
            "DC component: got {}, expected {}",
            freq[0].re,
            expected_dc
        );
    }
}
