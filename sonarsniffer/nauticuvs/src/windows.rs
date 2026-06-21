use std::f64::consts::PI;
use ndarray::Array2;
#[inline]
pub fn meyer_nu(t: f64) -> f64 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let t2 = t * t;
    let t3 = t2 * t;
    let t4 = t3 * t;
    (t4 * (35.0 - 84.0 * t + 70.0 * t2 - 20.0 * t3)).clamp(0.0, 1.0)
}
#[inline]
fn sqrt_ramp(x: f64) -> f64 {
    if x <= 0.0 {
        0.0
    } else {
        x.sqrt()
    }
}
pub fn scale_boundaries(num_scales: usize) -> Vec<f64> {
    let mut boundaries = Vec::with_capacity(num_scales + 1);
    boundaries.push(0.0);
    for j in 0..num_scales {
        let exp = (num_scales - 1 - j) as f64;
        boundaries.push(0.5 * 2.0f64.powf(-exp));
    }
    boundaries
}
pub fn build_radial_window(
    radial: &Array2<f64>,
    scale_idx: usize,
    num_scales: usize,
) -> Array2<f64> {
    let bounds = scale_boundaries(num_scales);
    let n = radial.nrows();
    let mut w = Array2::zeros((n, n));
    if scale_idx == 0 {
        let b1 = bounds[1];
        let b2 = bounds[2].min(0.5); // safety
        for i in 0..n {
            for j in 0..n {
                let r = radial[[i, j]];
                if r <= b1 {
                    w[[i, j]] = 1.0;
                } else if r < b2 {
                    let t = (r - b1) / (b2 - b1);
                    w[[i, j]] = sqrt_ramp(1.0 - meyer_nu(t));
                }
            }
        }
        return w;
    }
    if scale_idx == num_scales - 1 {
        let bj_1 = bounds[num_scales - 1];
        let bj = bounds[num_scales];
        for i in 0..n {
            for j in 0..n {
                let r = radial[[i, j]];
                if r >= bj {
                    w[[i, j]] = 1.0;
                } else if r > bj_1 {
                    let t = (r - bj_1) / (bj - bj_1);
                    w[[i, j]] = sqrt_ramp(meyer_nu(t));
                }
            }
        }
        return w;
    }
    let b_lo = bounds[scale_idx];
    let b_mid = bounds[scale_idx + 1];
    let b_hi = bounds[scale_idx + 2];
    for i in 0..n {
        for j in 0..n {
            let r = radial[[i, j]];
            if r <= b_lo || r >= b_hi {
                continue;
            }
            w[[i, j]] = if r <= b_mid {
                let t = (r - b_lo) / (b_mid - b_lo);
                sqrt_ramp(meyer_nu(t))
            } else {
                let t = (r - b_mid) / (b_hi - b_mid);
                sqrt_ramp(1.0 - meyer_nu(t))
            };
        }
    }
    w
}
pub fn build_angular_window(theta: &Array2<f64>, dir_idx: usize, num_dirs: usize) -> Array2<f64> {
    let n = theta.nrows();
    let sector_width = 2.0 * PI / num_dirs as f64;
    let center = -PI + (dir_idx as f64 + 0.5) * sector_width;
    let mut w = Array2::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            let mut dtheta = theta[[i, j]] - center;
            dtheta = dtheta.rem_euclid(2.0 * PI);
            if dtheta > PI {
                dtheta -= 2.0 * PI;
            }
            let abs_dt = dtheta.abs();
            if abs_dt < sector_width {
                let t = abs_dt / sector_width;
                w[[i, j]] = sqrt_ramp(1.0 - meyer_nu(t));
            }
        }
    }
    w
}
pub fn build_combined_window(
    rad_w: &Array2<f64>,
    theta: &Array2<f64>,
    dir_idx: usize,
    num_dirs: usize,
) -> Array2<f64> {
    let ang_w = build_angular_window(theta, dir_idx, num_dirs);
    let n = rad_w.nrows();
    let mut combined = Array2::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            combined[[i, j]] = rad_w[[i, j]] * ang_w[[i, j]];
        }
    }
    combined
}
#[cfg(test)]
mod window_tests {
    use super::*;
    #[test]
    fn test_meyer_nu_endpoints() {
        assert!((meyer_nu(0.0) - 0.0).abs() < 1e-14);
        assert!((meyer_nu(1.0) - 1.0).abs() < 1e-14);
    }
    #[test]
    fn test_meyer_nu_symmetry() {
        for i in 0..=100 {
            let t = i as f64 / 100.0;
            let sum = meyer_nu(t) + meyer_nu(1.0 - t);
            assert!(
                (sum - 1.0).abs() < 1e-12,
                "ν({t}) + ν({}) = {sum}, expected 1.0",
                1.0 - t
            );
        }
    }
    #[test]
    fn test_scale_boundaries() {
        let b = scale_boundaries(5);
        assert_eq!(b.len(), 6);
        assert!((b[0] - 0.0).abs() < 1e-14);
        assert!((b[5] - 0.5).abs() < 1e-14);
        for i in 1..b.len() {
            assert!(b[i] > b[i - 1]);
        }
    }
    #[test]
    fn test_radial_pou() {
        let num_scales = 5;
        let n = 64;
        let (xi_row, xi_col) = crate::utils::freq_grid_2d_f64(n);
        let radial = crate::utils::radial_freq_f64(&xi_row, &xi_col);
        let mut pou = Array2::<f64>::zeros((n, n));
        for s in 0..num_scales {
            let w = build_radial_window(&radial, s, num_scales);
            for i in 0..n {
                for j in 0..n {
                    pou[[i, j]] += w[[i, j]] * w[[i, j]];
                }
            }
        }
        let mut max_deviation = 0.0f64;
        for i in 0..n {
            for j in 0..n {
                if radial[[i, j]] > 1e-10 {
                    let dev = (pou[[i, j]] - 1.0).abs();
                    if dev > max_deviation {
                        max_deviation = dev;
                    }
                }
            }
        }
        assert!(
            max_deviation < 1e-10,
            "Radial POU max deviation: {max_deviation}"
        );
    }
    #[test]
    fn test_full_frame_pou_128_scales_4() {
        let num_scales = 4;
        let n = 128;
        let cfg = crate::config::CurveletConfig::new(num_scales).unwrap();
        let (xi_row, xi_col) = crate::utils::freq_grid_2d_f64(n);
        let radial = crate::utils::radial_freq_f64(&xi_row, &xi_col);
        let theta = crate::utils::angular_freq_f64(&xi_row, &xi_col);
        let mut pou = Array2::<f64>::zeros((n, n));
        let coarse = build_radial_window(&radial, 0, num_scales);
        let fine = build_radial_window(&radial, num_scales - 1, num_scales);
        for i in 0..n {
            for j in 0..n {
                pou[[i, j]] += coarse[[i, j]].powi(2) + fine[[i, j]].powi(2);
            }
        }
        for d in 0..cfg.num_detail_scales() {
            let scale_idx = d + 1;
            let num_dirs = cfg.directions_at_detail_scale(d);
            let rad_w = build_radial_window(&radial, scale_idx, num_scales);
            for l in 0..num_dirs {
                let w = build_combined_window(&rad_w, &theta, l, num_dirs);
                for i in 0..n {
                    for j in 0..n {
                        pou[[i, j]] += w[[i, j]].powi(2);
                    }
                }
            }
        }
        let mut min_pou = f64::INFINITY;
        let mut max_pou = 0.0f64;
        let mut bad = 0usize;
        for i in 0..n {
            for j in 0..n {
                let v = pou[[i, j]];
                if !v.is_finite() {
                    bad += 1;
                }
                if v < min_pou {
                    min_pou = v;
                }
                if v > max_pou {
                    max_pou = v;
                }
            }
        }
        assert_eq!(bad, 0, "non-finite POU cells");
        assert!(
            min_pou > 1e-12,
            "POU has near-zero cells: min={min_pou} max={max_pou}"
        );
        let mut max_dev = 0.0f64;
        for i in 0..n {
            for j in 0..n {
                if radial[[i, j]] > 1e-10 {
                    max_dev = max_dev.max((pou[[i, j]] - 1.0).abs());
                }
            }
        }
        assert!(max_dev < 1e-8, "frame POU max deviation {max_dev}");
    }
    #[test]
    fn test_angular_pou() {
        let num_dirs = 16;
        let n = 64;
        let (xi_row, xi_col) = crate::utils::freq_grid_2d_f64(n);
        let theta = crate::utils::angular_freq_f64(&xi_row, &xi_col);
        let mut pou = Array2::<f64>::zeros((n, n));
        for l in 0..num_dirs {
            let w = build_angular_window(&theta, l, num_dirs);
            for i in 0..n {
                for j in 0..n {
                    pou[[i, j]] += w[[i, j]] * w[[i, j]];
                }
            }
        }
        let mut max_deviation = 0.0f64;
        for i in 0..n {
            for j in 0..n {
                if i == 0 && j == 0 {
                    continue;
                }
                let dev = (pou[[i, j]] - 1.0).abs();
                if dev > max_deviation {
                    max_deviation = dev;
                }
            }
        }
        assert!(
            max_deviation < 1e-10,
            "Angular POU max deviation: {max_deviation}"
        );
    }
}