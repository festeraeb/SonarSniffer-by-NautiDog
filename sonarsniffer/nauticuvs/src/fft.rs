use ndarray::Array2;
use num_complex::Complex;
use rustfft::FftPlanner;
pub fn fft2_inplace(data: &mut Array2<Complex<f64>>) {
    let (rows, cols) = data.dim();
    let mut planner = FftPlanner::new();
    let fft_row = planner.plan_fft_forward(cols);
    for mut row in data.rows_mut() {
        let slice = row.as_slice_mut().expect("row must be contiguous");
        fft_row.process(slice);
    }
    let fft_col = planner.plan_fft_forward(rows);
    let mut col_buf = vec![Complex::ZERO; rows];
    for c in 0..cols {
        for r in 0..rows {
            col_buf[r] = data[[r, c]];
        }
        fft_col.process(&mut col_buf);
        for r in 0..rows {
            data[[r, c]] = col_buf[r];
        }
    }
}
pub fn ifft2_inplace(data: &mut Array2<Complex<f64>>) {
    let (rows, cols) = data.dim();
    let mut planner = FftPlanner::new();
    let ifft_row = planner.plan_fft_inverse(cols);
    for mut row in data.rows_mut() {
        let slice = row.as_slice_mut().expect("row must be contiguous");
        ifft_row.process(slice);
    }
    let ifft_col = planner.plan_fft_inverse(rows);
    let mut col_buf = vec![Complex::ZERO; rows];
    for c in 0..cols {
        for r in 0..rows {
            col_buf[r] = data[[r, c]];
        }
        ifft_col.process(&mut col_buf);
        for r in 0..rows {
            data[[r, c]] = col_buf[r];
        }
    }
    let norm = 1.0 / (rows * cols) as f64;
    data.mapv_inplace(|v| v * norm);
}
pub fn real_to_complex(data: &Array2<f64>) -> Array2<Complex<f64>> {
    data.mapv(|v| Complex::new(v, 0.0))
}
pub fn complex_to_real(data: &Array2<Complex<f64>>) -> Array2<f64> {
    data.mapv(|v| v.re)
}
#[cfg(test)]
mod fft_tests {
    use super::*;
    #[test]
    fn test_fft2_roundtrip() {
        let n = 64;
        let original = Array2::from_shape_fn((n, n), |(r, c)| {
            Complex::new((r * n + c) as f64 / (n * n) as f64, 0.0)
        });
        let mut data = original.clone();
        fft2_inplace(&mut data);
        ifft2_inplace(&mut data);
        let max_err: f64 = original
            .iter()
            .zip(data.iter())
            .map(|(a, b)| (a - b).norm())
            .fold(0.0f64, f64::max);
        assert!(
            max_err < 1e-10,
            "FFT roundtrip max error {max_err} exceeds tolerance"
        );
    }
    #[test]
    fn test_parseval() {
        let n = 32;
        let data =
            Array2::from_shape_fn((n, n), |(r, c)| Complex::new(((r + c) as f64).sin(), 0.0));
        let spatial_energy: f64 = data.iter().map(|v| v.norm_sqr()).sum();
        let mut freq = data.clone();
        fft2_inplace(&mut freq);
        let freq_energy: f64 = freq.iter().map(|v| v.norm_sqr()).sum();
        let n_total = (n * n) as f64;
        let ratio = freq_energy / (spatial_energy * n_total);
        assert!(
            (ratio - 1.0).abs() < 1e-10,
            "Parseval ratio {ratio} deviates from 1.0"
        );
    }
}