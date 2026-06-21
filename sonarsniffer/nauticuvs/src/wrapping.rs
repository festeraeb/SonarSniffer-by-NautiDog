#![allow(dead_code)]
use ndarray::Array2;
use num_complex::Complex;
pub fn wrapping_rectangle_size(scale_idx: usize, num_scales: usize, n: usize) -> (usize, usize) {
    if scale_idx == 0 {
        let size = (n >> (num_scales - 2)).max(4);
        return (size, size);
    }
    if scale_idx == num_scales - 1 {
        return (n, n);
    }
    let detail_idx = scale_idx - 1;
    let num_detail = num_scales - 2;
    let level_from_finest = num_detail - 1 - detail_idx;
    let length_shift = level_from_finest / 2;
    let width_shift = level_from_finest;
    let length = (n >> length_shift).max(4);
    let width = (n >> width_shift).max(4);
    (length, width)
}
pub fn wrap_to_rectangle(
    input: &Array2<Complex<f32>>,
    target_rows: usize,
    target_cols: usize,
) -> Array2<Complex<f32>> {
    let (in_rows, in_cols) = input.dim();
    let mut output = Array2::zeros((target_rows, target_cols));
    for r in 0..in_rows {
        for c in 0..in_cols {
            let tr = r % target_rows;
            let tc = c % target_cols;
            output[[tr, tc]] += input[[r, c]];
        }
    }
    output
}
pub fn unwrap_from_rectangle(
    wrapped: &Array2<Complex<f32>>,
    n_rows: usize,
    n_cols: usize,
) -> Array2<Complex<f32>> {
    let (wr, wc) = wrapped.dim();
    let mut output = Array2::zeros((n_rows, n_cols));
    for r in 0..n_rows {
        for c in 0..n_cols {
            output[[r, c]] = wrapped[[r % wr, c % wc]];
        }
    }
    output
}
#[cfg(test)]
mod wrapping_tests {
    use super::*;
    use num_complex::Complex;
    #[test]
    fn test_wrap_unwrap_identity_when_same_size() {
        let n = 16;
        let data: Array2<Complex<f32>> =
            Array2::from_shape_fn((n, n), |(r, c)| Complex::new((r * n + c) as f32, 0.0));
        let wrapped = wrap_to_rectangle(&data, n, n);
        assert_eq!(data, wrapped);
    }
    #[test]
    fn test_wrap_accumulation() {
        let n = 8;
        let ones = Array2::from_elem((n, n), Complex::new(1.0f32, 0.0));
        let wrapped = wrap_to_rectangle(&ones, 4, 4);
        for &v in wrapped.iter() {
            assert!((v.re - 4.0).abs() < 1e-10);
        }
    }
    #[test]
    fn test_rectangle_sizes_monotonic() {
        let n = 256;
        let num_scales = 5;
        let mut prev_area = 0;
        for s in 0..num_scales {
            let (rows, cols) = wrapping_rectangle_size(s, num_scales, n);
            let area = rows * cols;
            assert!(
                area >= prev_area,
                "scale {s} area {area} < previous {prev_area}"
            );
            prev_area = area;
        }
    }
}