//! FDCT curvelet energy per window (CPU, full-precision nauticuvs).

use ndarray::Array2;
use nauticuvs::curvelet_forward;

pub struct CurveletScore {
    pub energy_ratio: f64,
    pub ac_energy: f64,
    pub backend: &'static str,
}

pub fn score_window_f32(window: &[f32], rows: usize, cols: usize, scales: usize) -> CurveletScore {
    if rows < 4 || cols < 4 || window.len() != rows * cols {
        return CurveletScore {
            energy_ratio: 0.0,
            ac_energy: 0.0,
            backend: "skip",
        };
    }
    let arr: Array2<f32> =
        Array2::from_shape_fn((rows, cols), |(r, c)| window[r * cols + c]);
    match curvelet_forward(&arr, scales) {
        Ok(coeffs) => {
            let mut ac = 0.0f64;
            for scale in &coeffs.detail {
                for sb in scale {
                    ac += sb.iter().map(|c| c.norm_sqr()).sum::<f64>();
                }
            }
            ac += coeffs.fine.iter().map(|c| c.norm_sqr()).sum::<f64>();
            let coarse: f64 = coeffs.coarse.iter().map(|c| c.norm_sqr()).sum();
            let ratio = if coarse > 1e-12 {
                ac / coarse
            } else {
                ac / (window.len() as f64 + 1.0)
            };
            CurveletScore {
                energy_ratio: ratio,
                ac_energy: ac,
                backend: "nauticuvs-fdct",
            }
        }
        Err(_) => log_proxy(window, rows, cols),
    }
}

fn log_proxy(window: &[f32], rows: usize, cols: usize) -> CurveletScore {
    let mut peak = 0.0f32;
    let mut vals = vec![0.0f32; window.len()];
    let min = window.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = window.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let denom = max - min + 1e-9;
    for (i, v) in window.iter().enumerate() {
        vals[i] = (v - min) / denom;
    }
    for s in [1.5f32, 2.5, 4.0] {
        let mut lap = vec![0.0f32; window.len()];
        let sigma = s;
        let k = (sigma * 3.0).ceil() as i32;
        for r in 0..rows {
            for c in 0..cols {
                let mut acc = 0.0f32;
                let mut wsum = 0.0f32;
                for dr in -k..=k {
                    for dc in -k..=k {
                        let nr = r as i32 + dr;
                        let nc = c as i32 + dc;
                        if nr >= 0 && nr < rows as i32 && nc >= 0 && nc < cols as i32 {
                            let g = (-(dr * dr + dc * dc) as f32 / (2.0 * sigma * sigma)).exp();
                            acc += vals[nr as usize * cols + nc as usize] * g;
                            wsum += g;
                        }
                    }
                }
                let v = acc / wsum;
                lap[r * cols + c] = v;
            }
        }
        for i in 0..window.len() {
            let laplacian = {
                let r = i / cols;
                let c = i % cols;
                let v = lap[i];
                let l = if c > 0 { lap[i - 1] } else { v };
                let ri = if r > 0 { lap[i - cols] } else { v };
                v - 0.5 * (l + ri)
            };
            peak = peak.max(laplacian.abs());
        }
    }
    let mean: f32 = window.iter().sum::<f32>() / window.len() as f32;
    let std = (window.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / window.len() as f32).sqrt() + 1e-9;
    CurveletScore {
        energy_ratio: (peak / std) as f64,
        ac_energy: peak as f64,
        backend: "log-proxy",
    }
}

pub fn extract_window(grid: &[f32], w: u32, h: u32, row: u32, col: u32, size: u32) -> Vec<f32> {
    let half = size / 2;
    let mut patch = vec![0.0f32; (size * size) as usize];
    for dy in 0..size {
        for dx in 0..size {
            let r = row as i32 + dy as i32 - half as i32;
            let c = col as i32 + dx as i32 - half as i32;
            let v = if r >= 0 && c >= 0 && r < h as i32 && c < w as i32 {
                grid[(r as u32 * w + c as u32) as usize]
            } else {
                0.0
            };
            patch[(dy * size + dx) as usize] = v;
        }
    }
    patch
}
