//! Shared grid/morphology helpers.
//!
//! These are LIFTED (not rewritten) from the porting base
//! `pipelines/bag/wreckhunter2000/src/bag_mesh.rs`:
//!   * `binary_erode`, `binary_dilate`
//!   * `connected_components` (8-connected flood fill)
//!   * `gradient_magnitude` (central differences)
//!   * `pca_axis` (principal-axis heading/length/width)
//!
//! They underpin both the anomaly clustering and the redaction-region detection.

use ndarray::Array2;
use std::collections::VecDeque;

/// NoData threshold used across uncertainty math (from `bag_mesh.rs`).
pub const NODATA_THRESH: f32 = 999_000.0;

/// Binary erosion: shrink true regions by `iterations` pixels (4-connected).
/// Lifted from `bag_mesh.rs::binary_erode`.
pub fn binary_erode(mask: &Array2<bool>, iterations: usize) -> Array2<bool> {
    let mut current = mask.clone();
    let (rows, cols) = current.dim();

    for _ in 0..iterations {
        let prev = current.clone();
        for r in 0..rows {
            for c in 0..cols {
                if prev[[r, c]] {
                    let keep = (r > 0 && prev[[r - 1, c]])
                        && (r + 1 < rows && prev[[r + 1, c]])
                        && (c > 0 && prev[[r, c - 1]])
                        && (c + 1 < cols && prev[[r, c + 1]]);
                    current[[r, c]] = keep;
                }
            }
        }
    }
    current
}

/// Binary dilation: grow true regions by `iterations` pixels (4-connected).
/// Lifted from `bag_mesh.rs::binary_dilate`.
pub fn binary_dilate(mask: &Array2<bool>, iterations: usize) -> Array2<bool> {
    let mut current = mask.clone();
    let (rows, cols) = current.dim();

    for _ in 0..iterations {
        let prev = current.clone();
        for r in 0..rows {
            for c in 0..cols {
                if !prev[[r, c]] {
                    let grow = (r > 0 && prev[[r - 1, c]])
                        || (r + 1 < rows && prev[[r + 1, c]])
                        || (c > 0 && prev[[r, c - 1]])
                        || (c + 1 < cols && prev[[r, c + 1]]);
                    if grow {
                        current[[r, c]] = true;
                    }
                }
            }
        }
    }
    current
}

/// Pixel-bbox of a component: (r_min, r_max, c_min, c_max).
pub fn pixels_bbox(pixels: &[(usize, usize)]) -> (usize, usize, usize, usize) {
    let r_min = pixels.iter().map(|p| p.0).min().unwrap_or(0);
    let r_max = pixels.iter().map(|p| p.0).max().unwrap_or(0);
    let c_min = pixels.iter().map(|p| p.1).min().unwrap_or(0);
    let c_max = pixels.iter().map(|p| p.1).max().unwrap_or(0);
    (r_min, r_max, c_min, c_max)
}

/// Windowed 4-connected dilation of a single component.
///
/// Functionally identical to `binary_dilate` of a full-grid mask that is true
/// only on `pixels`, but the work is confined to the component bbox grown by
/// `iterations` (+1 guard) in each direction instead of the whole raster. The
/// returned set contains every (row, col) that the full-grid dilation would
/// mark true. This is the scalability fix for per-region morphology: cost is
/// O(window_area * iterations) rather than O(grid_area * iterations).
///
/// `rows`/`cols` are the full-grid bounds used to clamp the window.
pub fn dilate_component(
    pixels: &[(usize, usize)],
    iterations: usize,
    rows: usize,
    cols: usize,
) -> std::collections::HashSet<(usize, usize)> {
    use std::collections::HashSet;
    let set: HashSet<(usize, usize)> = pixels.iter().copied().collect();
    if iterations == 0 || set.is_empty() {
        return set;
    }
    let (r0, r1, c0, c1) = pixels_bbox(pixels);
    // Local window grown by the dilation reach, clamped to the grid.
    let wr0 = r0.saturating_sub(iterations + 1);
    let wr1 = (r1 + iterations + 1).min(rows.saturating_sub(1));
    let wc0 = c0.saturating_sub(iterations + 1);
    let wc1 = (c1 + iterations + 1).min(cols.saturating_sub(1));
    let lh = wr1 - wr0 + 1;
    let lw = wc1 - wc0 + 1;

    // Local boolean buffer.
    let idx = |r: usize, c: usize| (r - wr0) * lw + (c - wc0);
    let mut cur = vec![false; lh * lw];
    for &(r, c) in pixels {
        cur[idx(r, c)] = true;
    }
    let mut prev = cur.clone();
    for _ in 0..iterations {
        prev.copy_from_slice(&cur);
        for lr in 0..lh {
            for lc in 0..lw {
                let i = lr * lw + lc;
                if prev[i] {
                    continue;
                }
                let up = lr > 0 && prev[i - lw];
                let down = lr + 1 < lh && prev[i + lw];
                let left = lc > 0 && prev[i - 1];
                let right = lc + 1 < lw && prev[i + 1];
                if up || down || left || right {
                    cur[i] = true;
                }
            }
        }
    }

    let mut out = HashSet::new();
    for lr in 0..lh {
        for lc in 0..lw {
            if cur[lr * lw + lc] {
                out.insert((wr0 + lr, wc0 + lc));
            }
        }
    }
    out
}

/// Windowed erosion of a single component: returns the count of interior pixels
/// that survive `iterations` of 4-connected erosion. Equivalent to running
/// `binary_erode` on the component's own mask but confined to its bbox.
pub fn erode_component_interior_count(pixels: &[(usize, usize)], iterations: usize) -> usize {
    if pixels.is_empty() {
        return 0;
    }
    if iterations == 0 {
        return pixels.len();
    }
    let (r0, r1, c0, c1) = pixels_bbox(pixels);
    // Pad by 1 so the bbox border erodes correctly (outside = background).
    let wr0 = r0.saturating_sub(1);
    let wc0 = c0.saturating_sub(1);
    let lh = (r1 - r0) + 3;
    let lw = (c1 - c0) + 3;
    let idx = |r: usize, c: usize| (r - wr0) * lw + (c - wc0);
    let mut cur = vec![false; lh * lw];
    for &(r, c) in pixels {
        cur[idx(r, c)] = true;
    }
    let mut prev = cur.clone();
    for _ in 0..iterations {
        prev.copy_from_slice(&cur);
        for lr in 0..lh {
            for lc in 0..lw {
                let i = lr * lw + lc;
                if !prev[i] {
                    continue;
                }
                let keep = lr > 0
                    && prev[i - lw]
                    && lr + 1 < lh
                    && prev[i + lw]
                    && lc > 0
                    && prev[i - 1]
                    && lc + 1 < lw
                    && prev[i + 1];
                cur[i] = keep;
            }
        }
    }
    cur.iter().filter(|&&v| v).count()
}

/// Connected component labelling via 8-connected BFS flood fill.
/// Lifted from `bag_mesh.rs::connected_components`.
///
/// Returns a label grid (0 = background, 1.. = component id).
pub fn connected_components(mask: &Array2<bool>) -> Array2<u32> {
    let (rows, cols) = mask.dim();
    let mut labels = Array2::<u32>::zeros((rows, cols));
    let mut current_label = 0u32;

    for r in 0..rows {
        for c in 0..cols {
            if mask[[r, c]] && labels[[r, c]] == 0 {
                current_label += 1;
                let mut queue = VecDeque::new();
                queue.push_back((r, c));
                labels[[r, c]] = current_label;

                while let Some((qr, qc)) = queue.pop_front() {
                    for (dr, dc) in &[
                        (-1i32, 0),
                        (1, 0),
                        (0, -1i32),
                        (0, 1),
                        (-1, -1),
                        (-1, 1),
                        (1, -1),
                        (1, 1),
                    ] {
                        let nr = qr as i32 + dr;
                        let nc = qc as i32 + dc;
                        if nr >= 0 && nr < rows as i32 && nc >= 0 && nc < cols as i32 {
                            let nr = nr as usize;
                            let nc = nc as usize;
                            if mask[[nr, nc]] && labels[[nr, nc]] == 0 {
                                labels[[nr, nc]] = current_label;
                                queue.push_back((nr, nc));
                            }
                        }
                    }
                }
            }
        }
    }
    labels
}

/// Collect the (row, col) pixels of every labelled component.
/// `out[k]` holds the pixels of label `k+1`.
pub fn component_pixels(labels: &Array2<u32>) -> Vec<Vec<(usize, usize)>> {
    let max_label = labels.iter().copied().max().unwrap_or(0);
    let mut out: Vec<Vec<(usize, usize)>> = vec![Vec::new(); max_label as usize];
    let (rows, cols) = labels.dim();
    for r in 0..rows {
        for c in 0..cols {
            let l = labels[[r, c]];
            if l > 0 {
                out[(l - 1) as usize].push((r, c));
            }
        }
    }
    out
}

/// Gradient magnitude via central differences.
/// Lifted from `bag_mesh.rs::gradient_magnitude`.
pub fn gradient_magnitude(grid: &Array2<f32>) -> Array2<f32> {
    let (rows, cols) = grid.dim();
    let mut grad = Array2::<f32>::zeros((rows, cols));

    if rows < 3 || cols < 3 {
        return grad;
    }

    for r in 1..rows - 1 {
        for c in 1..cols - 1 {
            let gy = grid[[r + 1, c]] - grid[[r - 1, c]];
            let gx = grid[[r, c + 1]] - grid[[r, c - 1]];
            if gy.is_finite() && gx.is_finite() {
                grad[[r, c]] = (gy * gy + gx * gx).sqrt();
            }
        }
    }
    grad
}

/// PCA-based principal axis of a boolean mask.
/// Returns (heading_deg from north CW, length_m, width_m).
/// Lifted from `bag_mesh.rs::pca_axis`.
pub fn pca_axis(mask: &Array2<bool>, cell_size: f64) -> (f64, f64, f64) {
    let coords: Vec<(f64, f64)> = mask
        .indexed_iter()
        .filter(|(_, &v)| v)
        .map(|((r, c), _)| (r as f64, c as f64))
        .collect();

    pca_axis_from_coords(&coords, cell_size)
}

/// PCA principal axis from an explicit list of (row, col) coordinates.
/// Same math as `bag_mesh.rs::pca_axis`, factored so anomaly clusters
/// (stored as coordinate lists) can reuse it without building a full grid.
pub fn pca_axis_from_coords(coords: &[(f64, f64)], cell_size: f64) -> (f64, f64, f64) {
    if coords.len() < 3 {
        return (0.0, 0.0, 0.0);
    }

    let n = coords.len() as f64;
    let mean_r: f64 = coords.iter().map(|c| c.0).sum::<f64>() / n;
    let mean_c: f64 = coords.iter().map(|c| c.1).sum::<f64>() / n;

    let mut cov_rr = 0.0;
    let mut cov_rc = 0.0;
    let mut cov_cc = 0.0;
    for &(r, c) in coords {
        let dr = r - mean_r;
        let dc = c - mean_c;
        cov_rr += dr * dr;
        cov_rc += dr * dc;
        cov_cc += dc * dc;
    }
    cov_rr /= n;
    cov_rc /= n;
    cov_cc /= n;

    let trace = cov_rr + cov_cc;
    let det = cov_rr * cov_cc - cov_rc * cov_rc;
    let disc = (trace * trace / 4.0 - det).max(0.0).sqrt();
    let lambda1 = trace / 2.0 + disc;

    let (er, ec) = if cov_rc.abs() > 1e-10 {
        (lambda1 - cov_cc, cov_rc)
    } else if cov_rr >= cov_cc {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    };

    let heading = ec.atan2(er).to_degrees().rem_euclid(360.0);

    let norm = (er * er + ec * ec).sqrt().max(1e-10);
    let (pr, pc) = (er / norm, ec / norm);
    let (mr, mc) = (-pc, pr);

    let mut min_p = f64::INFINITY;
    let mut max_p = f64::NEG_INFINITY;
    let mut min_m = f64::INFINITY;
    let mut max_m = f64::NEG_INFINITY;

    for &(r, c) in coords {
        let dr = r - mean_r;
        let dc = c - mean_c;
        let proj_p = dr * pr + dc * pc;
        let proj_m = dr * mr + dc * mc;
        min_p = min_p.min(proj_p);
        max_p = max_p.max(proj_p);
        min_m = min_m.min(proj_m);
        max_m = max_m.max(proj_m);
    }

    let length = (max_p - min_p) * cell_size;
    let width = (max_m - min_m) * cell_size;

    (heading, length, width)
}

/// Percentile (linear, lower-index) of valid values — mirrors the
/// `sorted[len * pct / 100]` style indexing used in `detect_masked_regions`.
pub fn percentile(sorted_ascending: &[f32], pct: f64) -> f32 {
    if sorted_ascending.is_empty() {
        return f32::NAN;
    }
    let idx = ((sorted_ascending.len() as f64) * pct / 100.0) as usize;
    let idx = idx.min(sorted_ascending.len() - 1);
    sorted_ascending[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_erode() {
        let mut mask = Array2::<bool>::from_elem((7, 7), false);
        for r in 1..6 {
            for c in 1..6 {
                mask[[r, c]] = true;
            }
        }
        let eroded = binary_erode(&mask, 1);
        assert!(!eroded[[1, 1]]);
        assert!(eroded[[3, 3]]);
    }

    #[test]
    fn test_binary_dilate() {
        let mut mask = Array2::<bool>::from_elem((7, 7), false);
        mask[[3, 3]] = true;
        let dilated = binary_dilate(&mask, 1);
        assert!(dilated[[2, 3]]);
        assert!(dilated[[4, 3]]);
        assert!(!dilated[[0, 0]]);
    }

    #[test]
    fn test_connected_components_two_clusters() {
        let mut mask = Array2::<bool>::from_elem((10, 10), false);
        mask[[1, 1]] = true;
        mask[[1, 2]] = true;
        mask[[8, 8]] = true;
        mask[[8, 9]] = true;
        let labels = connected_components(&mask);
        assert!(labels[[1, 1]] > 0);
        assert_eq!(labels[[1, 1]], labels[[1, 2]]);
        assert_ne!(labels[[1, 1]], labels[[8, 8]]);

        let pix = component_pixels(&labels);
        assert_eq!(pix.len(), 2);
    }

    #[test]
    fn test_gradient_magnitude_unit_slope() {
        let mut grid = Array2::<f32>::zeros((5, 5));
        for r in 0..5 {
            for c in 0..5 {
                grid[[r, c]] = c as f32;
            }
        }
        let grad = gradient_magnitude(&grid);
        assert!((grad[[2, 2]] - 2.0).abs() < 0.1);
    }

    #[test]
    fn test_pca_axis_horizontal_bar() {
        let mut mask = Array2::<bool>::from_elem((5, 25), false);
        for c in 2..23 {
            mask[[2, c]] = true;
        }
        let (heading, length, width) = pca_axis(&mask, 0.5);
        assert!(heading > 45.0 && heading < 135.0, "heading={heading}");
        assert!(length > width, "length={length} width={width}");
    }

    #[test]
    fn test_percentile() {
        let v: Vec<f32> = (0..100).map(|x| x as f32).collect();
        assert_eq!(percentile(&v, 5.0), 5.0);
        assert_eq!(percentile(&v, 50.0), 50.0);
    }
}
