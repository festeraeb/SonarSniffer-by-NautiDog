//! CPU adaptive z-score + edge screen (device-agnostic).

use crate::geo::GridMeta;
use crate::knobs::DetectionLevels;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AdaptiveCandidate {
    pub label_id: u32,
    pub col: u32,
    pub row: u32,
    pub center_lat: f64,
    pub center_lon: f64,
    pub pixel_count: u32,
    pub width_m: f32,
    pub height_m: f32,
    pub score: f32,
    pub local_z_max: f32,
    pub edge_z_max: f32,
    pub amplitude_peak_abs: f32,
}

fn box_mean(src: &[f32], w: u32, h: u32, wy: u32, wx: u32) -> Vec<f32> {
    let mut out = vec![0.0f32; (w * h) as usize];
    let rad_y = (wy / 2) as i32;
    let rad_x = (wx / 2) as i32;
    for row in 0..h as i32 {
        for col in 0..w as i32 {
            let mut sum = 0.0f32;
            let mut cnt = 0.0f32;
            for dy in -rad_y..=rad_y {
                for dx in -rad_x..=rad_x {
                    let r = row + dy;
                    let c = col + dx;
                    if r >= 0 && r < h as i32 && c >= 0 && c < w as i32 {
                        sum += src[(r as u32 * w + c as u32) as usize];
                        cnt += 1.0;
                    }
                }
            }
            out[(row as u32 * w + col as u32) as usize] = sum / cnt.max(1.0);
        }
    }
    out
}

fn local_zscore(src: &[f32], w: u32, h: u32, wy: u32, wx: u32) -> Vec<f32> {
    let mean = box_mean(src, w, h, wy, wx);
    let sq: Vec<f32> = src.iter().map(|x| x * x).collect();
    let mean_sq = box_mean(&sq, w, h, wy, wx);
    let mut z = vec![0.0f32; (w * h) as usize];
    for i in 0..z.len() {
        let m = mean[i];
        let var = (mean_sq[i] - m * m).max(0.0);
        let std = var.sqrt().max(1e-6);
        z[i] = (src[i] - m) / std;
    }
    z
}

fn vertical_derivative(grid: &[f32], w: u32, h: u32, m_per_px: f32) -> Vec<f32> {
    let mut out = vec![0.0f32; grid.len()];
    for row in 0..h {
        for col in 0..w {
            let idx = (row * w + col) as usize;
            let left = if col > 0 { grid[idx - 1] } else { grid[idx] };
            let right = if col + 1 < w { grid[idx + 1] } else { grid[idx] };
            let up = if row > 0 { grid[idx - w as usize] } else { grid[idx] };
            let down = if row + 1 < h {
                grid[idx + w as usize]
            } else {
                grid[idx]
            };
            let gx = (right - left) / (2.0 * m_per_px);
            let gy = (down - up) / (2.0 * m_per_px);
            out[idx] = (gx * gx + gy * gy).sqrt();
        }
    }
    out
}

pub fn run_adaptive(
    grid: &[f32],
    w: u32,
    h: u32,
    m_per_px: f32,
    meta: &GridMeta,
    levels: &DetectionLevels,
) -> Vec<AdaptiveCandidate> {
    let mut work: Vec<f32> = grid.to_vec();
    if levels.use_vertical_derivative {
        work = vertical_derivative(&work, w, h, m_per_px);
    }
    let (wy, wx) = levels.window_px(m_per_px);
    let abs_arr: Vec<f32> = work.iter().map(|v| v.abs()).collect();
    let local_z = local_zscore(&abs_arr, w, h, wy, wx);

    let grad = vertical_derivative(&work, w, h, m_per_px);
    let edge_z = local_zscore(&grad, w, h, wy, wx);

    let mut mask = vec![false; (w * h) as usize];
    for i in 0..mask.len() {
        mask[i] = local_z[i] >= levels.z_thresh && edge_z[i] >= levels.edge_z_thresh;
    }

    let mut labels = vec![0u32; mask.len()];
    let mut current = 0u32;
    let mut out = Vec::new();

    for row in 0..h {
        for col in 0..w {
            let idx = (row * w + col) as usize;
            if !mask[idx] || labels[idx] != 0 {
                continue;
            }
            current += 1;
            let mut stack = vec![(col, row)];
            let mut pixels = Vec::new();
            labels[idx] = current;
            while let Some((c, r)) = stack.pop() {
                pixels.push((c, r));
                for (dc, dr) in [(0i32, 1), (0, -1), (1, 0), (-1, 0)] {
                    let nc = c as i32 + dc;
                    let nr = r as i32 + dr;
                    if nc < 0 || nr < 0 || nc >= w as i32 || nr >= h as i32 {
                        continue;
                    }
                    let ni = (nr as u32 * w + nc as u32) as usize;
                    if mask[ni] && labels[ni] == 0 {
                        labels[ni] = current;
                        stack.push((nc as u32, nr as u32));
                    }
                }
            }
            let pix = pixels.len() as u32;
            if pix < levels.min_pixels || pix > levels.max_pixels {
                continue;
            }
            let mut best_i = 0usize;
            let mut best_abs = 0.0f32;
            let mut z_max = 0.0f32;
            let mut ez_max = 0.0f32;
            let mut cmin = w;
            let mut cmax = 0u32;
            let mut rmin = h;
            let mut rmax = 0u32;
            for (i, (c, r)) in pixels.iter().enumerate() {
                let ni = (*r * w + *c) as usize;
                if abs_arr[ni] > best_abs {
                    best_abs = abs_arr[ni];
                    best_i = i;
                }
                z_max = z_max.max(local_z[ni]);
                ez_max = ez_max.max(edge_z[ni]);
                cmin = cmin.min(*c);
                cmax = cmax.max(*c);
                rmin = rmin.min(*r);
                rmax = rmax.max(*r);
            }
            let (bc, br) = pixels[best_i];
            let (lon, lat) = meta.pixel_to_lon_lat(bc, br);
            let score = 0.45 * z_max + 0.55 * ez_max;
            out.push(AdaptiveCandidate {
                label_id: current,
                col: bc,
                row: br,
                center_lat: lat,
                center_lon: lon,
                pixel_count: pix,
                width_m: (cmax - cmin + 1) as f32 * m_per_px,
                height_m: (rmax - rmin + 1) as f32 * m_per_px,
                score,
                local_z_max: z_max,
                edge_z_max: ez_max,
                amplitude_peak_abs: best_abs,
            });
        }
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    out.truncate(levels.top_n);
    out
}
