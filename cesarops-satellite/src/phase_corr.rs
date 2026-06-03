//! Sub-pixel translational coregistration via 2D phase correlation (FFT).

use crate::engine_error::EngineError;
use ndarray::Array2;
use num_complex::Complex;
use rustfft::FftPlanner;

pub const EPS_CROSS_POWER: f64 = 1e-12;
pub const MAX_CORR_DIM: usize = 512;

/// Phase-correlation shift (dx, dy) in **pixel** units: apply to `target` as
/// `aligned(x,y) = target(x + dx, y + dy)` to match `reference` (sub-pixel bilinear).
pub fn phase_shift_subpixel(
    reference: &[f64],
    target: &[f64],
    width: usize,
    height: usize,
) -> Result<(f64, f64), EngineError> {
    if reference.len() != target.len() || reference.len() != width * height {
        return Err(EngineError::Dim(format!(
            "expected {} elements, got ref={} tgt={}",
            width * height,
            reference.len(),
            target.len()
        )));
    }
    if width < 8 || height < 8 {
        return Err(EngineError::Msg("grid too small for FFT correlation".into()));
    }

    let (dw, dh, ref_ds, tgt_ds) = downsample_pair(reference, target, width, height, MAX_CORR_DIM);
    let (dx_ds, dy_ds) = phase_shift_same_size(&ref_ds, &tgt_ds, dw, dh)?;
    let scale_x = width as f64 / dw as f64;
    let scale_y = height as f64 / dh as f64;
    Ok((dx_ds * scale_x, dy_ds * scale_y))
}

/// Shift detection on fixed-size row-major `f64` grids (already downsampled).
pub fn phase_shift_same_size(
    reference: &[f64],
    target: &[f64],
    width: usize,
    height: usize,
) -> Result<(f64, f64), EngineError> {
    let ref_real = array_from_row_major(reference, width, height)?;
    let tgt_real = array_from_row_major(target, width, height)?;
    let mut f_ref = ref_real.mapv(|v| Complex::new(v, 0.0));
    let mut f_tgt = tgt_real.mapv(|v| Complex::new(v, 0.0));
    fft2_inplace(&mut f_ref);
    fft2_inplace(&mut f_tgt);

    let mut cross = Array2::<Complex<f64>>::zeros((height, width));
    for r in 0..height {
        for c in 0..width {
            let prod = f_ref[[r, c]] * f_tgt[[r, c]].conj();
            let denom = prod.norm() + EPS_CROSS_POWER;
            cross[[r, c]] = prod / denom;
        }
    }
    ifft2_inplace(&mut cross);
    let surface = complex_to_real(&cross);
    peak_subpixel_shift(&surface, width, height)
}

/// Warp `src` toward `reference` using sub-pixel shift (dx, dy) in pixel coords.
pub fn warp_clarity_plane(src: &Array2<f32>, dx: f64, dy: f64) -> Array2<f32> {
    let (h, w) = src.dim();
    let mut out = Array2::<f32>::from_elem((h, w), f32::NAN);
    for r in 0..h {
        for c in 0..w {
            let sr = r as f64 + dy;
            let sc = c as f64 + dx;
            out[[r, c]] = sample_bilinear(src, sc, sr);
        }
    }
    out
}

fn downsample_pair(
    reference: &[f64],
    target: &[f64],
    width: usize,
    height: usize,
    max_dim: usize,
) -> (usize, usize, Vec<f64>, Vec<f64>) {
    let factor = (width.max(height) as f64 / max_dim as f64).ceil() as usize;
    let factor = factor.max(1);
    let dw = (width + factor - 1) / factor;
    let dh = (height + factor - 1) / factor;
    let ref_ds = block_mean_downsample(reference, width, height, factor);
    let tgt_ds = block_mean_downsample(target, width, height, factor);
    (dw, dh, ref_ds, tgt_ds)
}

fn block_mean_downsample(data: &[f64], width: usize, height: usize, factor: usize) -> Vec<f64> {
    let dw = (width + factor - 1) / factor;
    let dh = (height + factor - 1) / factor;
    let mut out = vec![0.0; dw * dh];
    for or in 0..dh {
        for oc in 0..dw {
            let mut sum = 0.0;
            let mut n = 0usize;
            for r in (or * factor)..((or + 1) * factor).min(height) {
                for c in (oc * factor)..((oc + 1) * factor).min(width) {
                    let v = data[r * width + c];
                    if v.is_finite() {
                        sum += v;
                        n += 1;
                    }
                }
            }
            out[or * dw + oc] = if n > 0 { sum / n as f64 } else { f64::NAN };
        }
    }
    out
}

fn array_from_row_major(data: &[f64], width: usize, height: usize) -> Result<Array2<f64>, EngineError> {
    if data.len() != width * height {
        return Err(EngineError::Dim("row-major length mismatch".into()));
    }
    let mut arr = Array2::<f64>::zeros((height, width));
    for r in 0..height {
        for c in 0..width {
            arr[[r, c]] = data[r * width + c];
        }
    }
    Ok(arr)
}

fn fft2_inplace(data: &mut Array2<Complex<f64>>) {
    let (rows, cols) = data.dim();
    let mut planner = FftPlanner::new();
    let fft_row = planner.plan_fft_forward(cols);
    for mut row in data.rows_mut() {
        fft_row.process(row.as_slice_mut().unwrap());
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

fn ifft2_inplace(data: &mut Array2<Complex<f64>>) {
    let (rows, cols) = data.dim();
    let mut planner = FftPlanner::new();
    let ifft_row = planner.plan_fft_inverse(cols);
    for mut row in data.rows_mut() {
        ifft_row.process(row.as_slice_mut().unwrap());
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

fn complex_to_real(data: &Array2<Complex<f64>>) -> Array2<f64> {
    data.mapv(|v| v.re)
}

/// Peak on correlation surface → sub-pixel (dx, dy) with fftshift-style wrap.
fn peak_subpixel_shift(surface: &Array2<f64>, width: usize, height: usize) -> Result<(f64, f64), EngineError> {
    let (mut pr, mut pc, mut peak) = (0usize, 0usize, f64::NEG_INFINITY);
    for r in 0..height {
        for c in 0..width {
            let v = surface[[r, c]];
            if v.is_finite() && v > peak {
                peak = v;
                pr = r;
                pc = c;
            }
        }
    }
    if !peak.is_finite() {
        return Err(EngineError::InvalidPeak { row: pr, col: pc });
    }

    let hr = height as f64 / 2.0;
    let hc = width as f64 / 2.0;
    let mut dx = pc as f64;
    let mut dy = pr as f64;
    if dx > hc {
        dx -= width as f64;
    }
    if dy > hr {
        dy -= height as f64;
    }

    if pr > 0 && pr + 1 < height && pc > 0 && pc + 1 < width {
        let (sub_dx, sub_dy) = parabolic_3x3(surface, pr, pc, width, height)?;
        dx += sub_dx;
        dy += sub_dy;
    }
    // Correlation peak → shift to apply to `target` to align with `reference`.
    Ok((-dx, -dy))
}

fn parabolic_3x3(
    surface: &Array2<f64>,
    pr: usize,
    pc: usize,
    width: usize,
    height: usize,
) -> Result<(f64, f64), EngineError> {
    if pr == 0 && pc == 0 {
        // Peak at origin = tiles are already co-registered, no shift needed.
        return Ok((0.0, 0.0));
    }
    if pr == 0 || pc == 0 || pr + 1 >= height || pc + 1 >= width {
        return Err(EngineError::InvalidPeak { row: pr, col: pc });
    }
    let z0 = surface[[pr, pc]];
    let zxm = surface[[pr, pc - 1]];
    let zxp = surface[[pr, pc + 1]];
    let zym = surface[[pr - 1, pc]];
    let zyp = surface[[pr + 1, pc]];
    let denom_x = 2.0 * (2.0 * z0 - zxm - zxp);
    let denom_y = 2.0 * (2.0 * z0 - zym - zyp);
    let sub_dx = if denom_x.abs() > EPS_CROSS_POWER {
        (zxm - zxp) / denom_x
    } else {
        0.0
    };
    let sub_dy = if denom_y.abs() > EPS_CROSS_POWER {
        (zym - zyp) / denom_y
    } else {
        0.0
    };
    Ok((sub_dx.clamp(-0.5, 0.5), sub_dy.clamp(-0.5, 0.5)))
}

fn sample_bilinear(src: &Array2<f32>, x: f64, y: f64) -> f32 {
    let (h, w) = src.dim();
    if x < 0.0 || y < 0.0 || x >= (w - 1) as f64 || y >= (h - 1) as f64 {
        return f32::NAN;
    }
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let fx = (x - x0 as f64) as f32;
    let fy = (y - y0 as f64) as f32;
    let v00 = src[[y0, x0]];
    let v10 = src[[y0, x0 + 1]];
    let v01 = src[[y0 + 1, x0]];
    let v11 = src[[y0 + 1, x0 + 1]];
    if !v00.is_finite() || !v10.is_finite() || !v01.is_finite() || !v11.is_finite() {
        return f32::NAN;
    }
    let top = v00 * (1.0 - fx) + v10 * fx;
    let bot = v01 * (1.0 - fx) + v11 * fx;
    top * (1.0 - fy) + bot * fy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_integer_shift_on_synthetic() {
        let w = 64;
        let h = 64;
        let mut ref_grid = vec![0.0f64; w * h];
        for r in 10..30 {
            for c in 10..30 {
                ref_grid[r * w + c] = 1.0;
            }
        }
        let mut tgt_grid = vec![0.0f64; w * h];
        let dx_true = 3i32;
        let dy_true = 2i32;
        for r in 10..30 {
            for c in 10..30 {
                let tr = r as i32 + dy_true;
                let tc = c as i32 + dx_true;
                if tr >= 0 && tr < h as i32 && tc >= 0 && tc < w as i32 {
                    tgt_grid[tr as usize * w + tc as usize] = 1.0;
                }
            }
        }
        let (dx, dy) = phase_shift_subpixel(&ref_grid, &tgt_grid, w, h).expect("shift");
        assert!((dx - dx_true as f64).abs() < 0.6, "dx={dx}");
        assert!((dy - dy_true as f64).abs() < 0.6, "dy={dy}");
    }
}
