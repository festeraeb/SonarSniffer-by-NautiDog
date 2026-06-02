//! Magnetic chip derived layers.
//!
//! Ports the derived-layer math from `wh2k_chip_extractor.py` (the 3-channel
//! NSS / VDR / Tilt representation used for cross-domain magnetic candidates):
//!   `_compute_nss`        → [`compute_nss`]        (gradient + Laplacian magnitude)
//!   `_compute_vdr`        → [`compute_vdr`]        (FFT |k| vertical-derivative operator)
//!   `_compute_tilt_angle` → [`compute_tilt_angle`] (atan2(VDR, THDR))
//!   `chip_to_3channel`    → [`chip_to_3channel`]   (stack NSS/VDR/Tilt)
//!
//! The VDR uses a forward 2-D FFT, multiplies by `|k| * 2π` (the first-vertical-
//! derivative operator), and inverse-transforms — reimplemented here with
//! `rustfft` (already a dependency).  Spatial gradients use `np.gradient`'s
//! second-order central differences with one-sided ends, and the Laplacian uses
//! `scipy.ndimage.laplace` (the 4-neighbour stencil with `reflect` boundary).

use ndarray::Array2;
use rustfft::{num_complex::Complex, FftPlanner};

// ── np.gradient ───────────────────────────────────────────────────────────────

/// `np.gradient` along axis 0 (rows): central differences interior, one-sided
/// at the ends.  Unit spacing.
pub fn gradient_axis0(grid: &Array2<f64>) -> Array2<f64> {
    let (h, w) = grid.dim();
    let mut out = Array2::<f64>::zeros((h, w));
    if h == 1 {
        return out; // np.gradient on a length-1 axis is 0
    }
    for c in 0..w {
        for r in 0..h {
            out[[r, c]] = if r == 0 {
                grid[[1, c]] - grid[[0, c]]
            } else if r == h - 1 {
                grid[[h - 1, c]] - grid[[h - 2, c]]
            } else {
                (grid[[r + 1, c]] - grid[[r - 1, c]]) / 2.0
            };
        }
    }
    out
}

/// `np.gradient` along axis 1 (columns): central differences interior, one-sided
/// at the ends.  Unit spacing.
pub fn gradient_axis1(grid: &Array2<f64>) -> Array2<f64> {
    let (h, w) = grid.dim();
    let mut out = Array2::<f64>::zeros((h, w));
    if w == 1 {
        return out;
    }
    for r in 0..h {
        for c in 0..w {
            out[[r, c]] = if c == 0 {
                grid[[r, 1]] - grid[[r, 0]]
            } else if c == w - 1 {
                grid[[r, w - 1]] - grid[[r, w - 2]]
            } else {
                (grid[[r, c + 1]] - grid[[r, c - 1]]) / 2.0
            };
        }
    }
    out
}

/// `scipy.ndimage.laplace`: sum of second differences along each axis using the
/// 4-neighbour stencil with `reflect` boundary handling (scipy default).
///
///   lap[r,c] = (f[r-1,c] + f[r+1,c] - 2 f[r,c]) + (f[r,c-1] + f[r,c+1] - 2 f[r,c])
///
/// with reflected indices at the borders.
pub fn laplace(grid: &Array2<f64>) -> Array2<f64> {
    let (h, w) = grid.dim();
    let mut out = Array2::<f64>::zeros((h, w));
    let refl = |i: isize, n: usize| -> usize {
        // scipy 'reflect' mode (d c b a | a b c d | d c b a)
        if n == 1 {
            return 0;
        }
        let n = n as isize;
        let period = 2 * n;
        let mut k = ((i % period) + period) % period;
        if k >= n {
            k = period - 1 - k;
        }
        k as usize
    };
    for r in 0..h {
        for c in 0..w {
            let up = grid[[refl(r as isize - 1, h), c]];
            let down = grid[[refl(r as isize + 1, h), c]];
            let left = grid[[r, refl(c as isize - 1, w)]];
            let right = grid[[r, refl(c as isize + 1, w)]];
            let center = grid[[r, c]];
            out[[r, c]] = (up + down - 2.0 * center) + (left + right - 2.0 * center);
        }
    }
    out
}

// ── NSS ───────────────────────────────────────────────────────────────────────

/// Normalised Signal Strength: `sqrt(dx² + dy² + dz²)` where dx/dy are spatial
/// gradients and dz is the Laplacian.
///
/// Mirrors Python `_compute_nss`.
pub fn compute_nss(grid: &Array2<f64>) -> Array2<f64> {
    let dx = gradient_axis1(grid); // axis=1
    let dy = gradient_axis0(grid); // axis=0
    let dz = laplace(grid);
    let mut out = Array2::<f64>::zeros(grid.raw_dim());
    ndarray::Zip::from(&mut out)
        .and(&dx)
        .and(&dy)
        .and(&dz)
        .for_each(|o, &x, &y, &z| {
            *o = (x * x + y * y + z * z).sqrt();
        });
    out
}

// ── VDR (FFT |k| operator) ────────────────────────────────────────────────────

/// `np.fft.fftfreq(n)` — sample frequencies for an `n`-point FFT with unit
/// spacing: `[0, 1, …, (n-1)/2, -(n/2), …, -1] / n` (even/odd handled).
fn fftfreq(n: usize) -> Vec<f64> {
    let mut f = vec![0.0; n];
    if n == 0 {
        return f;
    }
    let nf = n as f64;
    // np: first ceil(n/2) entries are 0..(n-1)/2 ; remainder are negative.
    let split = (n - 1) / 2 + 1; // number of non-negative entries
    for (i, fi) in f.iter_mut().enumerate() {
        let val = if i < split {
            i as f64
        } else {
            i as f64 - nf
        };
        *fi = val / nf;
    }
    f
}

/// Vertical Derivative (Reduction): forward FFT, multiply by `|k| * 2π`, inverse
/// FFT, take the real part.
///
/// Mirrors Python `_compute_vdr`:
///   fft = np.fft.fft2(grid)
///   ky = fftfreq(ny)[:,None]; kx = fftfreq(nx)[None,:]
///   k_mag = sqrt(kx² + ky²); k_mag[0,0] = 1e-10
///   vdr = real(ifft2(fft * k_mag * 2π))
pub fn compute_vdr(grid: &Array2<f64>) -> Array2<f64> {
    let (ny, nx) = grid.dim();
    if ny == 0 || nx == 0 {
        return Array2::zeros(grid.raw_dim());
    }

    // Forward 2-D FFT (row FFTs then column FFTs).
    let mut planner = FftPlanner::<f64>::new();
    let fft_row = planner.plan_fft_forward(nx);
    let fft_col = planner.plan_fft_forward(ny);
    let ifft_row = planner.plan_fft_inverse(nx);
    let ifft_col = planner.plan_fft_inverse(ny);

    // Build complex buffer row-major.
    let mut buf: Vec<Vec<Complex<f64>>> = (0..ny)
        .map(|r| {
            let mut row: Vec<Complex<f64>> =
                (0..nx).map(|c| Complex::new(grid[[r, c]], 0.0)).collect();
            fft_row.process(&mut row);
            row
        })
        .collect();
    for c in 0..nx {
        let mut col: Vec<Complex<f64>> = (0..ny).map(|r| buf[r][c]).collect();
        fft_col.process(&mut col);
        for r in 0..ny {
            buf[r][c] = col[r];
        }
    }

    // Multiply by |k| * 2π.
    let ky = fftfreq(ny);
    let kx = fftfreq(nx);
    for r in 0..ny {
        for c in 0..nx {
            let mut k_mag = (kx[c] * kx[c] + ky[r] * ky[r]).sqrt();
            if r == 0 && c == 0 {
                k_mag = 1e-10; // matches k_mag[0,0] = 1e-10
            }
            let op = k_mag * 2.0 * std::f64::consts::PI;
            buf[r][c] *= op;
        }
    }

    // Inverse 2-D FFT (rustfft inverse is unnormalised → divide by ny*nx).
    for c in 0..nx {
        let mut col: Vec<Complex<f64>> = (0..ny).map(|r| buf[r][c]).collect();
        ifft_col.process(&mut col);
        for r in 0..ny {
            buf[r][c] = col[r];
        }
    }
    for r in 0..ny {
        ifft_row.process(&mut buf[r]);
    }

    let norm = (ny * nx) as f64;
    let mut out = Array2::<f64>::zeros((ny, nx));
    for r in 0..ny {
        for c in 0..nx {
            out[[r, c]] = buf[r][c].re / norm;
        }
    }
    out
}

// ── Tilt angle ────────────────────────────────────────────────────────────────

/// Tilt angle: `atan2(VDR, THDR)` where `THDR = sqrt(dx² + dy²)`.
///
/// Mirrors Python `_compute_tilt_angle`.
pub fn compute_tilt_angle(grid: &Array2<f64>) -> Array2<f64> {
    let dx = gradient_axis1(grid);
    let dy = gradient_axis0(grid);
    let vdr = compute_vdr(grid);
    let mut out = Array2::<f64>::zeros(grid.raw_dim());
    ndarray::Zip::from(&mut out)
        .and(&dx)
        .and(&dy)
        .and(&vdr)
        .for_each(|o, &x, &y, &v| {
            let thdr = (x * x + y * y).sqrt();
            *o = v.atan2(thdr + 1e-12);
        });
    out
}

// ── 3-channel stack ───────────────────────────────────────────────────────────

/// Stack [NSS, VDR, Tilt] into a 3-channel representation of shape `(3, H, W)`.
///
/// Mirrors Python `chip_to_3channel`: `np.stack([nss, vdr, tilt], axis=0)`.
/// Returned as `f32` channels to match the ResNet tile format.
pub fn chip_to_3channel(chip: &Array2<f64>) -> [Array2<f32>; 3] {
    let nss = compute_nss(chip).mapv(|v| v as f32);
    let vdr = compute_vdr(chip).mapv(|v| v as f32);
    let tilt = compute_tilt_angle(chip).mapv(|v| v as f32);
    [nss, vdr, tilt]
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fftfreq_matches_numpy_even() {
        // np.fft.fftfreq(4) = [0, 0.25, -0.5, -0.25]
        let f = fftfreq(4);
        let exp = [0.0, 0.25, -0.5, -0.25];
        for (a, b) in f.iter().zip(exp.iter()) {
            assert!((a - b).abs() < 1e-12, "got {a}, want {b}");
        }
    }

    #[test]
    fn fftfreq_matches_numpy_odd() {
        // np.fft.fftfreq(5) = [0, 0.2, 0.4, -0.4, -0.2]
        let f = fftfreq(5);
        let exp = [0.0, 0.2, 0.4, -0.4, -0.2];
        for (a, b) in f.iter().zip(exp.iter()) {
            assert!((a - b).abs() < 1e-12, "got {a}, want {b}");
        }
    }

    #[test]
    fn vdr_constant_field_is_near_zero() {
        // A DC-only (constant) field: all energy at k=0, which is killed by the
        // |k|→1e-10 operator, so VDR ≈ 0 everywhere.
        let grid = Array2::<f64>::from_elem((16, 16), 5.0);
        let vdr = compute_vdr(&grid);
        for v in vdr.iter() {
            assert!(v.abs() < 1e-6, "constant field VDR should be ~0, got {v}");
        }
    }

    #[test]
    fn vdr_operator_amplifies_high_frequency() {
        // The VDR operator is |k|*2π in the frequency domain. A pure high-freq
        // sinusoid should pass through scaled by ~|k|*2π. Build a single-axis
        // cosine and check the VDR amplitude exceeds the input amplitude
        // (since |k|*2π > 1 for the chosen frequency).
        let n = 32usize;
        let mut grid = Array2::<f64>::zeros((n, n));
        // frequency index m along columns → k = m/n. Choose m = 8 → k = 0.25,
        // |k|*2π ≈ 1.571 > 1.
        let m = 8.0;
        for r in 0..n {
            for c in 0..n {
                grid[[r, c]] = (2.0 * std::f64::consts::PI * m * c as f64 / n as f64).cos();
            }
        }
        let vdr = compute_vdr(&grid);
        let in_amp = grid.iter().cloned().fold(0.0_f64, |a, b| a.max(b.abs()));
        let out_amp = vdr.iter().cloned().fold(0.0_f64, |a, b| a.max(b.abs()));
        assert!(in_amp > 0.9 && in_amp < 1.1, "input amplitude ~1");
        // |k|*2π for k=0.25 is ~1.5708 → output amplitude should be amplified.
        assert!(out_amp > 1.3, "high-freq VDR should be amplified, got {out_amp}");
        assert!(out_amp < 1.8, "amplification should be ~|k|*2π≈1.57, got {out_amp}");
    }

    #[test]
    fn vdr_is_finite_everywhere() {
        let mut grid = Array2::<f64>::zeros((12, 10));
        for (i, v) in grid.iter_mut().enumerate() {
            *v = ((i * 13) % 7) as f64;
        }
        let vdr = compute_vdr(&grid);
        assert!(vdr.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn nss_constant_field_is_zero() {
        // Constant field → zero gradients and zero Laplacian → NSS = 0.
        let grid = Array2::<f64>::from_elem((8, 8), 3.3);
        let nss = compute_nss(&grid);
        for v in nss.iter() {
            assert!(v.abs() < 1e-9, "constant NSS should be 0, got {v}");
        }
    }

    #[test]
    fn nss_detects_a_ramp() {
        // A linear ramp along columns has constant dx=1, dy=0, laplace≈0 interior
        // → NSS ≈ 1 in the interior.
        let (h, w) = (10usize, 10usize);
        let mut grid = Array2::<f64>::zeros((h, w));
        for r in 0..h {
            for c in 0..w {
                grid[[r, c]] = c as f64;
            }
        }
        let nss = compute_nss(&grid);
        assert!((nss[[5, 5]] - 1.0).abs() < 1e-9, "ramp NSS interior ≈ 1");
    }

    #[test]
    fn gradient_axis1_linear_ramp() {
        // d/dx of f = c is 1 everywhere (central + one-sided ends).
        let (h, w) = (4usize, 5usize);
        let mut grid = Array2::<f64>::zeros((h, w));
        for r in 0..h {
            for c in 0..w {
                grid[[r, c]] = c as f64;
            }
        }
        let g = gradient_axis1(&grid);
        for v in g.iter() {
            assert!((v - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn three_channel_shape() {
        let grid = Array2::<f64>::from_elem((16, 16), 1.0);
        let ch = chip_to_3channel(&grid);
        assert_eq!(ch.len(), 3);
        for c in &ch {
            assert_eq!(c.dim(), (16, 16));
            assert!(c.iter().all(|v| v.is_finite()));
        }
    }
}
