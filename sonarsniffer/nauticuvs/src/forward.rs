use ndarray::Array2;
use num_complex::Complex;
#[cfg(feature = "parallel")]
use rayon::prelude::*;
use crate::coeffs::CurveletCoeffs;
use crate::config::CurveletConfig;
use crate::error::CurveletError;
use crate::fft;
use crate::utils;
use crate::windows;
fn extract_subband(
    f_hat: &Array2<Complex<f64>>,
    w_arr: &Array2<f64>,
    inv_sqrt_pou: &Array2<f64>,
    n: usize,
) -> Array2<Complex<f64>> {
    let mut subband = Array2::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            let w = w_arr[[i, j]] * inv_sqrt_pou[[i, j]];
            subband[[i, j]] = f_hat[[i, j]] * w;
        }
    }
    fft::ifft2_inplace(&mut subband);
    subband
}
pub fn forward_transform(
    image: &Array2<f32>,
    config: &CurveletConfig,
) -> Result<CurveletCoeffs, CurveletError> {
    utils::validate_image(image)?;
    let (orig_rows, orig_cols) = image.dim();
    let max_dim = orig_rows.max(orig_cols);
    let n = max_dim.next_power_of_two();
    let padded = utils::zero_pad(image, n, n);
    let mut cfg = config.clone();
    cfg.original_rows = orig_rows;
    cfg.original_cols = orig_cols;
    cfg.padded_size = n;
    let mut f_hat = fft::real_to_complex(&padded);
    fft::fft2_inplace(&mut f_hat);
    let (xi_row, xi_col) = utils::freq_grid_2d_f64(n);
    let radial = utils::radial_freq_f64(&xi_row, &xi_col);
    let theta = utils::angular_freq_f64(&xi_row, &xi_col);
    let num_scales = cfg.num_scales;
    let coarse_window = windows::build_radial_window(&radial, 0, num_scales);
    let fine_window = windows::build_radial_window(&radial, num_scales - 1, num_scales);
    let mut pou = Array2::<f64>::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            pou[[i, j]] += coarse_window[[i, j]].powi(2);
            pou[[i, j]] += fine_window[[i, j]].powi(2);
        }
    }
    let num_detail = cfg.num_detail_scales();
    let mut detail_windows: Vec<Vec<Array2<f64>>> = Vec::with_capacity(num_detail);
    for d in 0..num_detail {
        let scale_idx = d + 1;
        let num_dirs = cfg.directions_at_detail_scale(d);
        let rad_w = windows::build_radial_window(&radial, scale_idx, num_scales);
        #[cfg(feature = "parallel")]
        let dirs_windows: Vec<Array2<f64>> = (0..num_dirs)
            .into_par_iter()
            .map(|l| windows::build_combined_window(&rad_w, &theta, l, num_dirs))
            .collect();
        #[cfg(not(feature = "parallel"))]
        let dirs_windows: Vec<Array2<f64>> = (0..num_dirs)
            .map(|l| windows::build_combined_window(&rad_w, &theta, l, num_dirs))
            .collect();
        for w in &dirs_windows {
            for i in 0..n {
                for j in 0..n {
                    pou[[i, j]] += w[[i, j]].powi(2);
                }
            }
        }
        detail_windows.push(dirs_windows);
    }
    pou.mapv_inplace(|v| if v < 1e-30 { 1.0 } else { v });
    let inv_sqrt_pou = pou.mapv(|v| 1.0 / v.sqrt());
    let coarse_spec = extract_subband(&f_hat, &coarse_window, &inv_sqrt_pou, n);
    let mut detail_coeffs: Vec<Vec<Array2<Complex<f64>>>> = Vec::with_capacity(num_detail);
    for scale_windows in &detail_windows {
        #[cfg(feature = "parallel")]
        let dir_coeffs: Vec<Array2<Complex<f64>>> = scale_windows
            .par_iter()
            .map(|w_arr| extract_subband(&f_hat, w_arr, &inv_sqrt_pou, n))
            .collect();
        #[cfg(not(feature = "parallel"))]
        let dir_coeffs: Vec<Array2<Complex<f64>>> = scale_windows
            .iter()
            .map(|w_arr| extract_subband(&f_hat, w_arr, &inv_sqrt_pou, n))
            .collect();
        detail_coeffs.push(dir_coeffs);
    }
    let fine_spec = extract_subband(&f_hat, &fine_window, &inv_sqrt_pou, n);
    Ok(CurveletCoeffs {
        coarse: coarse_spec,
        detail: detail_coeffs,
        fine: fine_spec,
        config: cfg,
    })
}