//! CPU dipole analysis — ports `pipelines/mag/dipole_analysis.py`.
//!
//! Given an aeromagnetic nT grid and a candidate centre pixel, computes background
//! statistics, dipole morphology, gradient contrast, and a man-made likelihood score.

/// Inputs for one candidate anomaly analysis.
pub struct DipoleAnalysisInput<'a> {
    /// Aeromagnetic nT values, row-major (row * cols + col).
    pub grid: &'a [f32],
    pub rows: usize,
    pub cols: usize,
    /// Horizontal pixel resolution in metres.
    pub pixel_x_m: f64,
    /// Vertical pixel resolution in metres.
    pub pixel_y_m: f64,
    pub center_row: usize,
    pub center_col: usize,
    /// Inner radius of the core anomaly region in pixels (~2000 yards).
    pub inner_radius_px: usize,
    /// Outer radius of the background annulus in pixels (~5000 yards).
    pub outer_radius_px: usize,
}

/// Results from one candidate dipole analysis.
pub struct DipoleAnalysisResult {
    pub bg_mean: f64,
    pub bg_std: f64,
    pub peak_pos: f64,
    pub peak_neg: f64,
    pub peak_abs: f64,
    pub is_dipolar: bool,
    pub lobe_ratio: Option<f64>,
    pub dipole_separation_m: Option<f64>,
    pub dipole_azimuth_deg: Option<f64>,
    pub flip_dist_min_m: Option<f64>,
    pub flip_dist_mean_m: Option<f64>,
    pub grad_peak: Option<f64>,
    pub grad_contrast: Option<f64>,
    pub aspect_ratio: Option<f64>,
    /// Long-axis azimuth (deg, 0–180) of the significant inner pixels from PCA.
    /// Ports `elongation_azimuth` from dipole_analysis.py.
    pub elongation_azimuth_deg: Option<f64>,
    pub score_manmade: f64,
    pub classification: &'static str,
}

/// Classify a 0–100 man-made score into one of four verdict bands.
///
/// Ports the classification heuristic at the end of
/// `pipelines/mag/dipole_analysis.py`:
///   >= 60 → "LIKELY MAN-MADE (strong)"
///   >= 40 → "POSSIBLY MAN-MADE (moderate)"
///   >= 20 → "AMBIGUOUS"
///   else  → "LIKELY GEOLOGICAL"
pub fn classify_manmade(score: f64) -> &'static str {
    if score >= 60.0 {
        "LIKELY MAN-MADE (strong)"
    } else if score >= 40.0 {
        "POSSIBLY MAN-MADE (moderate)"
    } else if score >= 20.0 {
        "AMBIGUOUS"
    } else {
        "LIKELY GEOLOGICAL"
    }
}

/// Analyse a single anomaly candidate in an aeromagnetic grid.
///
/// Returns `None` if there are fewer than 3 annulus cells or fewer than 3 inner
/// cells, which indicates the candidate is too close to the grid edge or the radii
/// are larger than the grid.
pub fn analyze_candidate(input: &DipoleAnalysisInput) -> Option<DipoleAnalysisResult> {
    let rows = input.rows;
    let cols = input.cols;
    let cr = input.center_row as isize;
    let cc = input.center_col as isize;
    let r_inner = input.inner_radius_px as f64;
    let r_outer = input.outer_radius_px as f64;
    let px_x = input.pixel_x_m;
    let px_y = input.pixel_y_m;
    let grid = input.grid;

    // Safe grid index; returns None if (r, c) is out of bounds.
    let idx = |r: isize, c: isize| -> Option<usize> {
        if r >= 0 && c >= 0 && (r as usize) < rows && (c as usize) < cols {
            Some((r as usize) * cols + (c as usize))
        } else {
            None
        }
    };

    // Axis-aligned bounding box that covers both the inner region and the annulus.
    let row_min = (cr - r_outer as isize).max(0);
    let row_max = (cr + r_outer as isize + 1).min(rows as isize);
    let col_min = (cc - r_outer as isize).max(0);
    let col_max = (cc + r_outer as isize + 1).min(cols as isize);

    // Separate cells into the inner circle and the background annulus.
    let mut annulus_vals: Vec<f64> = Vec::new();
    let mut inner_cells: Vec<(isize, isize)> = Vec::new();

    for r in row_min..row_max {
        for c in col_min..col_max {
            let dr = (r - cr) as f64;
            let dc = (c - cc) as f64;
            let dist = (dr * dr + dc * dc).sqrt();
            if let Some(i) = idx(r, c) {
                if dist <= r_inner {
                    inner_cells.push((r, c));
                } else if dist <= r_outer {
                    annulus_vals.push(grid[i] as f64);
                }
            }
        }
    }

    if annulus_vals.len() < 3 || inner_cells.len() < 3 {
        return None;
    }

    // ── Background statistics ────────────────────────────────────────────────
    let n_ann = annulus_vals.len() as f64;
    let bg_mean = annulus_vals.iter().sum::<f64>() / n_ann;
    let bg_var = annulus_vals
        .iter()
        .map(|v| (v - bg_mean).powi(2))
        .sum::<f64>()
        / n_ann;
    let bg_std = bg_var.sqrt();

    // ── Detrended inner values ───────────────────────────────────────────────
    // idx(r, c) is always Some here because inner_cells were collected after the
    // bounds check above.
    let inner_detrended: Vec<(isize, isize, f64)> = inner_cells
        .iter()
        .map(|&(r, c)| {
            let v = grid[idx(r, c).unwrap()] as f64;
            (r, c, v - bg_mean)
        })
        .collect();

    // ── Peak statistics ──────────────────────────────────────────────────────
    let peak_pos = inner_detrended
        .iter()
        .map(|t| t.2)
        .fold(f64::NEG_INFINITY, f64::max);
    let peak_neg = inner_detrended
        .iter()
        .map(|t| t.2)
        .fold(f64::INFINITY, f64::min);
    let peak_abs = peak_pos.abs().max(peak_neg.abs());

    // Early exit: effectively uniform — no meaningful anomaly to analyse.
    if peak_abs < 1e-12 {
        return Some(DipoleAnalysisResult {
            bg_mean,
            bg_std,
            peak_pos,
            peak_neg,
            peak_abs,
            is_dipolar: false,
            lobe_ratio: None,
            dipole_separation_m: None,
            dipole_azimuth_deg: None,
            flip_dist_min_m: None,
            flip_dist_mean_m: None,
            grad_peak: None,
            grad_contrast: None,
            aspect_ratio: None,
            elongation_azimuth_deg: None,
            score_manmade: 0.0,
            classification: "LIKELY GEOLOGICAL",
        });
    }

    // ── Dipole lobes ─────────────────────────────────────────────────────────
    let lobe_threshold = 0.15 * peak_abs;

    let mut pos_rows: Vec<f64> = Vec::new();
    let mut pos_cols: Vec<f64> = Vec::new();
    let mut neg_rows: Vec<f64> = Vec::new();
    let mut neg_cols: Vec<f64> = Vec::new();

    for &(r, c, v) in &inner_detrended {
        if v > lobe_threshold {
            pos_rows.push(r as f64);
            pos_cols.push(c as f64);
        } else if v < -lobe_threshold {
            neg_rows.push(r as f64);
            neg_cols.push(c as f64);
        }
    }

    let is_dipolar = !pos_rows.is_empty() && !neg_rows.is_empty();

    // ── Lobe ratio ───────────────────────────────────────────────────────────
    let lobe_ratio = if is_dipolar {
        let pp = peak_pos.abs();
        let pn = peak_neg.abs();
        if pp.max(pn) > 1e-12 {
            Some((pp.min(pn) / pp.max(pn)).clamp(0.0, 1.0))
        } else {
            None
        }
    } else {
        None
    };

    // ── Dipole separation and azimuth ────────────────────────────────────────
    let (dipole_separation_m, dipole_azimuth_deg) = if is_dipolar {
        let pr = pos_rows.iter().sum::<f64>() / pos_rows.len() as f64;
        let pc = pos_cols.iter().sum::<f64>() / pos_cols.len() as f64;
        let nr = neg_rows.iter().sum::<f64>() / neg_rows.len() as f64;
        let nc = neg_cols.iter().sum::<f64>() / neg_cols.len() as f64;

        let dr = pr - nr;
        let dc = pc - nc;
        let avg_px_m = (px_x + px_y) / 2.0;
        let sep = (dr * dr + dc * dc).sqrt() * avg_px_m;
        // Azimuth: 0° = north, increasing clockwise.
        let az = (dc * px_x).atan2(-dr * px_y).to_degrees().rem_euclid(360.0);
        (Some(sep), Some(az))
    } else {
        (None, None)
    };

    // ── Polarity flip distance (8 directions from peak cell) ─────────────────
    // Find the inner cell with the largest absolute detrended value.
    let &(peak_r, peak_c, peak_v) = inner_detrended
        .iter()
        .max_by(|a, b| {
            a.2.abs()
                .partial_cmp(&b.2.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap(); // safe: inner_detrended.len() >= 3

    let peak_sign: f64 = if peak_v >= 0.0 { 1.0 } else { -1.0 };
    let flip_threshold = 0.05 * peak_abs;
    let max_walk = r_outer as isize + 5;

    const DIRS: [(isize, isize); 8] = [
        (0, 1),
        (0, -1),
        (1, 0),
        (-1, 0),
        (1, 1),
        (1, -1),
        (-1, 1),
        (-1, -1),
    ];

    let mut flip_distances: Vec<f64> = Vec::new();

    for &(dr, dc) in &DIRS {
        let dir_len = ((dr as f64).powi(2) + (dc as f64).powi(2)).sqrt();
        let mut step: isize = 1;
        loop {
            let r = peak_r + dr * step;
            let c = peak_c + dc * step;
            if let Some(i) = idx(r, c) {
                let v = grid[i] as f64 - bg_mean;
                if peak_sign * v < 0.0 && v.abs() > flip_threshold {
                    let dist_m = (step as f64) * dir_len * (px_x + px_y) / 2.0;
                    flip_distances.push(dist_m);
                    break;
                }
                step += 1;
                if step > max_walk {
                    break;
                }
            } else {
                break;
            }
        }
    }

    let flip_dist_min_m = if flip_distances.is_empty() {
        None
    } else {
        Some(
            flip_distances
                .iter()
                .cloned()
                .fold(f64::INFINITY, f64::min),
        )
    };
    let flip_dist_mean_m = if flip_distances.is_empty() {
        None
    } else {
        Some(flip_distances.iter().sum::<f64>() / flip_distances.len() as f64)
    };

    // ── Gradient contrast (central-difference Sobel) ─────────────────────────
    let mut inner_grad: Vec<f64> = Vec::new();
    let mut annulus_grad: Vec<f64> = Vec::new();

    for r in row_min..row_max {
        for c in col_min..col_max {
            // Central differences require all four orthogonal neighbours.
            if let (Some(il), Some(ir), Some(iu), Some(id)) = (
                idx(r, c - 1),
                idx(r, c + 1),
                idx(r - 1, c),
                idx(r + 1, c),
            ) {
                let gx = (grid[ir] as f64 - grid[il] as f64) / (2.0 * px_x);
                let gy = (grid[id] as f64 - grid[iu] as f64) / (2.0 * px_y);
                let gm = (gx * gx + gy * gy).sqrt();

                let dr = (r - cr) as f64;
                let dc = (c - cc) as f64;
                let dist = (dr * dr + dc * dc).sqrt();

                if dist <= r_inner {
                    inner_grad.push(gm);
                } else if dist <= r_outer {
                    annulus_grad.push(gm);
                }
            }
        }
    }

    let grad_peak = if inner_grad.is_empty() {
        None
    } else {
        let v = inner_grad.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        if v.is_finite() { Some(v) } else { None }
    };

    let bg_grad_mean = if annulus_grad.is_empty() {
        None
    } else {
        Some(annulus_grad.iter().sum::<f64>() / annulus_grad.len() as f64)
    };

    let grad_contrast = match (grad_peak, bg_grad_mean) {
        (Some(gp), Some(bgm)) if bgm > 1e-12 => Some((gp / bgm).clamp(0.0, 1_000.0)),
        _ => None,
    };

    // ── Aspect ratio via 2×2 PCA on significant inner pixels ─────────────────
    let sig_threshold = 0.25 * peak_abs;
    let sig_pts: Vec<(f64, f64)> = inner_detrended
        .iter()
        .filter(|&&(_, _, v)| v.abs() > sig_threshold)
        .map(|&(r, c, _)| (r as f64 * px_y, c as f64 * px_x))
        .collect();

    // aspect_ratio + elongation_azimuth_deg from PCA (ports the `aspect_ratio`
    // / `elongation_azimuth` block in dipole_analysis.py). Python stores points
    // as (row*my, col*mx); the long-axis azimuth is atan2(x_comp, y_comp) % 180.
    let (aspect_ratio, elongation_azimuth_deg) = if sig_pts.len() >= 3 {
        let n = sig_pts.len() as f64;
        let mean_y = sig_pts.iter().map(|p| p.0).sum::<f64>() / n;
        let mean_x = sig_pts.iter().map(|p| p.1).sum::<f64>() / n;

        let (mut cyy, mut cxx, mut cyx) = (0.0f64, 0.0f64, 0.0f64);
        for &(y, x) in &sig_pts {
            let dy = y - mean_y;
            let dx = x - mean_x;
            cyy += dy * dy;
            cxx += dx * dx;
            cyx += dy * dx;
        }
        cyy /= n;
        cxx /= n;
        cyx /= n;

        // Eigenvalues of 2×2 symmetric matrix via quadratic formula:
        //   λ² - trace·λ + det = 0
        let trace = cyy + cxx;
        let det = cyy * cxx - cyx * cyx;
        let discriminant = (trace * trace - 4.0 * det).max(0.0);
        let sqrt_d = discriminant.sqrt();
        let l1 = ((trace + sqrt_d) / 2.0).max(1e-12);
        let l2 = ((trace - sqrt_d) / 2.0).max(1e-12);
        let l_max = l1.max(l2);
        let l_min = l1.min(l2);
        let ar = (l_max / l_min).sqrt().clamp(1.0, 100.0);

        // Major eigenvector of [[cyy, cyx],[cyx, cxx]] for l_max is
        // (vy, vx) = (l_max - cxx, cyx). Fall back to axis-aligned when the
        // matrix is (near-)diagonal so atan2 stays well-defined.
        let (vy, vx) = if cyx.abs() > 1e-12 {
            (l_max - cxx, cyx)
        } else if cyy >= cxx {
            (1.0, 0.0) // major axis along row/y → azimuth 0
        } else {
            (0.0, 1.0) // major axis along col/x → azimuth 90
        };
        let az = vx.atan2(vy).to_degrees().rem_euclid(180.0);
        (Some(ar), Some(az))
    } else {
        (None, None)
    };

    // ── Score and classify ───────────────────────────────────────────────────
    let mut score = 0.0f64;

    if is_dipolar {
        score += 25.0;
    }
    if let Some(lr) = lobe_ratio {
        if lr > 0.3 {
            score += 15.0 * lr;
        }
    }
    if let Some(sep) = dipole_separation_m {
        if sep < 3_000.0 {
            score += 20.0;
        }
    }
    if let Some(fd) = flip_dist_min_m {
        if fd < 2_000.0 {
            let flip_km = fd / 1_000.0;
            score += 20.0 * (1.0 - flip_km / 2.0);
        }
    }
    if let Some(gc) = grad_contrast {
        if gc > 3.0 {
            score += 20.0;
        } else if gc > 1.5 {
            score += 10.0;
        }
    }
    if let Some(ar) = aspect_ratio {
        if ar < 3.0 {
            score += 10.0;
        } else if ar > 6.0 {
            score -= 10.0;
        }
    }

    // Man-made score bands — ports dipole_analysis.py classification heuristic.
    // Four bands: >=60 strong, >=40 possible, >=20 ambiguous, else geological.
    let classification = classify_manmade(score);

    Some(DipoleAnalysisResult {
        bg_mean,
        bg_std,
        peak_pos,
        peak_neg,
        peak_abs,
        is_dipolar,
        lobe_ratio,
        dipole_separation_m,
        dipole_azimuth_deg,
        flip_dist_min_m,
        flip_dist_mean_m,
        grad_peak,
        grad_contrast,
        aspect_ratio,
        elongation_azimuth_deg,
        score_manmade: score,
        classification,
    })
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A uniform grid has no anomaly: bg_mean equals the constant value and
    /// peak_abs is effectively zero, so the result is not dipolar.
    #[test]
    fn test_uniform_grid_not_dipolar() {
        let rows = 100usize;
        let cols = 100usize;
        let grid = vec![5.0f32; rows * cols];

        let input = DipoleAnalysisInput {
            grid: &grid,
            rows,
            cols,
            pixel_x_m: 100.0,
            pixel_y_m: 100.0,
            center_row: 50,
            center_col: 50,
            inner_radius_px: 10,
            outer_radius_px: 20,
        };

        let result = analyze_candidate(&input).expect("uniform grid should return Some");
        assert!(
            (result.bg_mean - 5.0).abs() < 1e-9,
            "bg_mean should equal the constant grid value"
        );
        assert!(
            result.peak_abs < 1e-6,
            "peak_abs should be near zero for a uniform grid"
        );
        assert!(!result.is_dipolar, "uniform grid must not be classified as dipolar");
    }

    /// A synthetic dipole: one positive 11×9 block above centre and one negative
    /// 11×9 block below centre, both within the inner radius.  The algorithm must
    /// detect both lobes and report is_dipolar = true.
    #[test]
    fn test_synthetic_dipole_is_dipolar() {
        let rows = 200usize;
        let cols = 200usize;
        let mut grid = vec![0.0f32; rows * cols];

        // Positive lobe: rows 82–92, cols 96–104
        // Max distance from centre (100,100) ≈ sqrt(18²+4²) ≈ 18.4 px — within r=25.
        for r in 82..93usize {
            for c in 96..105usize {
                grid[r * cols + c] = 50.0;
            }
        }
        // Negative lobe: rows 108–118, cols 96–104 (mirror)
        for r in 108..119usize {
            for c in 96..105usize {
                grid[r * cols + c] = -50.0;
            }
        }

        let input = DipoleAnalysisInput {
            grid: &grid,
            rows,
            cols,
            pixel_x_m: 100.0,
            pixel_y_m: 100.0,
            center_row: 100,
            center_col: 100,
            inner_radius_px: 25,
            outer_radius_px: 45,
        };

        let result = analyze_candidate(&input).expect("dipole grid should return Some");
        assert!(result.is_dipolar, "synthetic dipole should be detected");
        assert!(
            result.dipole_separation_m.is_some(),
            "separation must be computed for a dipolar signal"
        );
        // Positive and negative peaks are symmetric → lobe_ratio should be 1.0.
        assert!(
            result.lobe_ratio.map(|r| (r - 1.0).abs() < 1e-6).unwrap_or(false),
            "symmetric dipole lobe_ratio should be 1.0"
        );
    }

    /// When the inner radius is larger than the grid the annulus is empty, so the
    /// function must return None rather than panic.
    #[test]
    fn test_too_small_grid_returns_none() {
        let rows = 5usize;
        let cols = 5usize;
        let grid = vec![1.0f32; rows * cols];

        let input = DipoleAnalysisInput {
            grid: &grid,
            rows,
            cols,
            pixel_x_m: 100.0,
            pixel_y_m: 100.0,
            center_row: 2,
            center_col: 2,
            inner_radius_px: 10,
            outer_radius_px: 20,
        };

        assert!(
            analyze_candidate(&input).is_none(),
            "grid too small to have an annulus → must return None"
        );
    }

    /// The man-made score must map to the four Python verdict bands, including
    /// the AMBIGUOUS band (20–40) that Wave 1 was missing.
    #[test]
    fn test_four_classification_bands() {
        assert_eq!(classify_manmade(75.0), "LIKELY MAN-MADE (strong)");
        assert_eq!(classify_manmade(60.0), "LIKELY MAN-MADE (strong)");
        assert_eq!(classify_manmade(50.0), "POSSIBLY MAN-MADE (moderate)");
        assert_eq!(classify_manmade(40.0), "POSSIBLY MAN-MADE (moderate)");
        // AMBIGUOUS band (the new fourth band).
        assert_eq!(classify_manmade(30.0), "AMBIGUOUS");
        assert_eq!(classify_manmade(20.0), "AMBIGUOUS");
        assert_eq!(classify_manmade(19.9), "LIKELY GEOLOGICAL");
        assert_eq!(classify_manmade(0.0), "LIKELY GEOLOGICAL");
    }

    /// A synthetic dipole should populate `elongation_azimuth_deg` (0–180) when
    /// the significant inner pixels form an elongated cluster. The dipole here
    /// is elongated along rows (north-south), so the long axis azimuth should be
    /// near 0° / 180°.
    #[test]
    fn test_elongation_azimuth_populated() {
        let rows = 200usize;
        let cols = 200usize;
        let mut grid = vec![0.0f32; rows * cols];
        // Vertically (row-direction) elongated positive blob + negative blob.
        for r in 82..93usize {
            for c in 98..103usize {
                grid[r * cols + c] = 50.0;
            }
        }
        for r in 108..119usize {
            for c in 98..103usize {
                grid[r * cols + c] = -50.0;
            }
        }
        let input = DipoleAnalysisInput {
            grid: &grid,
            rows,
            cols,
            pixel_x_m: 100.0,
            pixel_y_m: 100.0,
            center_row: 100,
            center_col: 100,
            inner_radius_px: 25,
            outer_radius_px: 45,
        };
        let result = analyze_candidate(&input).expect("dipole grid should return Some");
        let az = result
            .elongation_azimuth_deg
            .expect("elongation azimuth must be computed for an elongated cluster");
        assert!((0.0..=180.0).contains(&az), "azimuth must be in [0,180], got {az}");
        // Row-elongated → long axis near 0°/180°.
        let near_axis = az < 20.0 || az > 160.0;
        assert!(near_axis, "row-elongated cluster long axis should be ~0/180, got {az}");
    }
}
