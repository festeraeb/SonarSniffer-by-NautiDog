//! Along-track KMZ / viewer overlay alignment.
//!
//! Satellite-style registration on overlap bands between consecutive strips:
//! normalized cross-correlation for sub-pixel row shift, tone matching, and
//! cross-fade at segment boundaries. Dropout rows are masked transparent.

use image::{Rgb, RgbImage};

/// Tunables for strip-chain alignment (see `align_overlay_strip_chain`).
#[derive(Debug, Clone, Copy)]
pub struct AlignConfig {
    /// Rows from bottom of strip N and top of strip N+1 used for correlation.
    pub overlap_rows: u32,
    /// Maximum along-track shift searched (pixels / ping rows).
    pub max_shift_rows: i32,
    /// Minimum NCC peak to apply a shift (avoids junk fits on uniform bottom).
    pub min_correlation: f64,
    /// Rows to RGB-blend at the seam (keep small — wide crossfade looks blurry in GE).
    pub crossfade_rows: u32,
}

impl Default for AlignConfig {
    fn default() -> Self {
        Self {
            overlap_rows: 48,
            max_shift_rows: 40,
            min_correlation: 0.22,
            crossfade_rows: 10,
        }
    }
}

fn luminance(rgb: &Rgb<u8>) -> f32 {
    rgb[0] as f32 * 0.299 + rgb[1] as f32 * 0.587 + rgb[2] as f32 * 0.114
}

fn extract_band_lum(img: &RgbImage, y0: u32, h: u32) -> Vec<f32> {
    let w = img.width();
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in y0..y0.saturating_add(h).min(img.height()) {
        for x in 0..w {
            out.push(luminance(img.get_pixel(x, y)));
        }
    }
    out
}

/// Normalized cross-correlation; returns (best_shift, peak_score).
/// Positive shift moves `candidate` **down** relative to `master` (candidate samples from higher y).
fn estimate_vertical_shift_ncc(
    master: &[f32],
    candidate: &[f32],
    w: usize,
    h: usize,
    max_shift: i32,
) -> (f32, f64) {
    if w == 0 || h == 0 || master.len() != candidate.len() || master.len() != w * h {
        return (0.0, 0.0);
    }

    let mut best_shift = 0i32;
    let mut best_score = f64::NEG_INFINITY;

    for dy in -max_shift..=max_shift {
        let mut sum_m = 0.0f64;
        let mut sum_c = 0.0f64;
        let mut sum_m2 = 0.0f64;
        let mut sum_c2 = 0.0f64;
        let mut sum_mc = 0.0f64;
        let mut n = 0u64;

        for y in 0..h {
            let cy = y as i32 + dy;
            if cy < 0 || cy >= h as i32 {
                continue;
            }
            for x in 0..w {
                let mi = y * w + x;
                let ci = cy as usize * w + x;
                let m = master[mi] as f64;
                let c = candidate[ci] as f64;
                sum_m += m;
                sum_c += c;
                sum_m2 += m * m;
                sum_c2 += c * c;
                sum_mc += m * c;
                n += 1;
            }
        }

        if n < 64 {
            continue;
        }
        let nf = n as f64;
        let var_m = sum_m2 - sum_m * sum_m / nf;
        let var_c = sum_c2 - sum_c * sum_c / nf;
        if var_m < 1e-6 || var_c < 1e-6 {
            continue;
        }
        let cov = sum_mc - sum_m * sum_c / nf;
        let score = cov / (var_m.sqrt() * var_c.sqrt());
        if score > best_score {
            best_score = score;
            best_shift = dy;
        }
    }

    // Sub-pixel parabolic refinement on scores at dy-1, dy, dy+1
    let mut refine = best_shift as f32;
    if best_shift > -max_shift && best_shift < max_shift {
        let s0 = ncc_at_shift(master, candidate, w, h, best_shift - 1);
        let s1 = best_score;
        let s2 = ncc_at_shift(master, candidate, w, h, best_shift + 1);
        let denom = s0 - 2.0 * s1 + s2;
        if denom.abs() > 1e-9 {
            let sub = (0.5 * (s0 - s2) / denom) as f32;
            refine = best_shift as f32 + sub.clamp(-0.5, 0.5);
        }
    }

    (refine, best_score)
}

fn ncc_at_shift(master: &[f32], candidate: &[f32], w: usize, h: usize, dy: i32) -> f64 {
    let mut sum_m = 0.0f64;
    let mut sum_c = 0.0f64;
    let mut sum_m2 = 0.0f64;
    let mut sum_c2 = 0.0f64;
    let mut sum_mc = 0.0f64;
    let mut n = 0u64;
    for y in 0..h {
        let cy = y as i32 + dy;
        if cy < 0 || cy >= h as i32 {
            continue;
        }
        for x in 0..w {
            let mi = y * w + x;
            let ci = cy as usize * w + x;
            let m = master[mi] as f64;
            let c = candidate[ci] as f64;
            sum_m += m;
            sum_c += c;
            sum_m2 += m * m;
            sum_c2 += c * c;
            sum_mc += m * c;
            n += 1;
        }
    }
    if n < 64 {
        return 0.0;
    }
    let nf = n as f64;
    let var_m = sum_m2 - sum_m * sum_m / nf;
    let var_c = sum_c2 - sum_c * sum_c / nf;
    if var_m < 1e-6 || var_c < 1e-6 {
        return 0.0;
    }
    let cov = sum_mc - sum_m * sum_c / nf;
    cov / (var_m.sqrt() * var_c.sqrt())
}

/// Resample `img` with vertical shift `dy` (positive = content moves down).
fn shift_rgb_vertical(img: &RgbImage, dy: f32) -> RgbImage {
    let (w, h) = img.dimensions();
    let mut out = RgbImage::new(w, h);
    let bg = Rgb([5u8, 10, 20]);
    for y in 0..h {
        for x in 0..w {
            let sy = y as f32 - dy;
            if sy < 0.0 || sy > (h - 1) as f32 {
                out.put_pixel(x, y, bg);
                continue;
            }
            let y0 = sy.floor() as u32;
            let y1 = (y0 + 1).min(h - 1);
            let fy = sy - y0 as f32;
            let p0 = img.get_pixel(x, y0);
            let p1 = img.get_pixel(x, y1);
            let blend = |a: u8, b: u8| -> u8 {
                (a as f32 * (1.0 - fy) + b as f32 * fy).round().clamp(0.0, 255.0) as u8
            };
            out.put_pixel(
                x,
                y,
                Rgb([blend(p0[0], p1[0]), blend(p0[1], p1[1]), blend(p0[2], p1[2])]),
            );
        }
    }
    out
}

fn tone_match_overlap(prev: &RgbImage, next: &mut RgbImage, overlap: u32) {
    let oh = overlap.min(prev.height()).min(next.height());
    if oh < 4 {
        return;
    }
    let w = prev.width();
    let prev_y0 = prev.height().saturating_sub(oh);
    let mut sum_p = 0.0f64;
    let mut sum_n = 0.0f64;
    let mut cnt = 0u64;
    for y in 0..oh {
        for x in 0..w {
            let lp = luminance(prev.get_pixel(x, prev_y0 + y));
            let ln = luminance(next.get_pixel(x, y));
            if lp > 8.0 && ln > 8.0 {
                sum_p += lp as f64;
                sum_n += ln as f64;
                cnt += 1;
            }
        }
    }
    if cnt < 32 || sum_n < 1.0 {
        return;
    }
    let scale = (sum_p / sum_n) as f32;
    let scale = scale.clamp(0.75, 1.35);
    for y in 0..oh.min(next.height()) {
        for x in 0..w {
            let p = next.get_pixel(x, y);
            let adj = Rgb([
                (p[0] as f32 * scale).round().clamp(0.0, 255.0) as u8,
                (p[1] as f32 * scale).round().clamp(0.0, 255.0) as u8,
                (p[2] as f32 * scale).round().clamp(0.0, 255.0) as u8,
            ]);
            next.put_pixel(x, y, adj);
        }
    }
}

fn crossfade_overlap(prev: &RgbImage, next: &mut RgbImage, overlap: u32) {
    let oh = overlap.min(prev.height()).min(next.height());
    if oh < 2 {
        return;
    }
    let w = prev.width();
    let prev_y0 = prev.height().saturating_sub(oh);
    for y in 0..oh {
        let t = (y as f32 + 0.5) / oh as f32;
        let w_prev = 1.0 - t;
        let w_next = t;
        for x in 0..w {
            let pp = prev.get_pixel(x, prev_y0 + y);
            let pn = next.get_pixel(x, y);
            let blend = |a: u8, b: u8| -> u8 {
                (a as f32 * w_prev + b as f32 * w_next).round().clamp(0.0, 255.0) as u8
            };
            next.put_pixel(
                x,
                y,
                Rgb([blend(pp[0], pn[0]), blend(pp[1], pn[1]), blend(pp[2], pn[2])]),
            );
        }
    }
}

/// Align `next` to `prev` using overlap-band NCC (along-track).
pub fn align_strip_pair(prev: &RgbImage, next: &mut RgbImage, cfg: &AlignConfig) {
    let oh = cfg
        .overlap_rows
        .min(prev.height() / 2)
        .min(next.height() / 2)
        .max(8);
    let w = prev.width() as usize;
    let h = oh as usize;
    if w == 0 || h == 0 || prev.width() != next.width() {
        return;
    }

    let master = extract_band_lum(prev, prev.height().saturating_sub(oh), oh);
    let candidate = extract_band_lum(next, 0, oh);
    let max_shift = cfg
        .max_shift_rows
        .min((oh / 2) as i32)
        .max(4);

    let (dy, peak) = estimate_vertical_shift_ncc(&master, &candidate, w, h, max_shift);
    if peak >= cfg.min_correlation && dy.abs() > 0.05 {
        *next = shift_rgb_vertical(next, dy);
    }
    tone_match_overlap(prev, next, oh);
    let fade = cfg.crossfade_rows.min(oh);
    crossfade_overlap(prev, next, fade);
}

/// Register consecutive overlay strips for KMZ / viewer compositing.
pub fn align_overlay_strip_chain(strips: &mut [RgbImage], cfg: &AlignConfig) {
    if strips.len() < 2 {
        return;
    }
    for i in 1..strips.len() {
        let (left, right) = strips.split_at_mut(i);
        let prev = &left[i - 1];
        let next = &mut right[0];
        align_strip_pair(prev, next, cfg);
    }
}

/// Mark ping-dropout rows (near-black) so feathering makes them transparent in KMZ.
pub fn mask_dropout_rows(rgb: &RgbImage) -> RgbImage {
    let (w, h) = rgb.dimensions();
    let mut out = rgb.clone();
    for y in 0..h {
        let mut dark = 0u32;
        for x in 0..w {
            if luminance(out.get_pixel(x, y)) < 5.0 {
                dark += 1;
            }
        }
        if dark > w * 3 / 10 {
            for x in 0..w {
                out.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ncc_finds_known_shift() {
        let w = 32usize;
        let h = 24usize;
        let mut master = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                master[y * w + x] = ((x + y) as f32).sin() * 50.0 + 80.0;
            }
        }
        let shift = 3i32;
        let mut candidate = vec![0.0f32; w * h];
        for y in 0..h {
            let sy = y as i32 - shift;
            if sy < 0 || sy >= h as i32 {
                continue;
            }
            for x in 0..w {
                candidate[y * w + x] = master[sy as usize * w + x];
            }
        }
        let (dy, peak) = estimate_vertical_shift_ncc(&master, &candidate, w, h, 8);
        assert!(peak > 0.5, "peak={peak}");
        assert!((dy - shift as f32).abs() < 1.5, "dy={dy}");
    }
}
