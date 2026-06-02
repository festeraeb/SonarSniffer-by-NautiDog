//! Redaction / masking detection — the marquee IP.
//!
//! This module ports three complementary detection families:
//!
//! 1. ELEVATION SIGNATURE DETECTORS (from `advanced_bag_scanner.py`):
//!    `_detect_smoothing_signatures`, `_detect_removal_signatures`,
//!    `_detect_alteration_signatures`, `_detect_pattern_signatures`, and
//!    `_identify_redactors`. These emit [`RedactionSignature`]s describing HOW
//!    the data was tampered with. Off by default (`enable_redaction_signatures`).
//!
//! 2. MASKING SCANNER (from `masking_scanner.py::MaskingDetector`):
//!    `detect_nan_holes`, `detect_flattened_zones`, `detect_texture_breaks`,
//!    plus the `_score_confidence` heuristic and `UnmaskPreview.restore` depth
//!    anomaly. These emit [`MaskedRegion`]s — the wreck-sized redacted areas.
//!
//! 3. UNCERTAINTY-BASED MASK DETECTION (lifted from
//!    `bag_mesh.rs::detect_masked_regions`): connected clusters of uncertainty
//!    below the p5 percentile, closed + eroded. Used when band 2 is present.
//!
//! Every masked region is also surfaced as a contract-bearing detection with
//! `signature_type = "masked_redaction_flat"` (see [`crate::pipeline`]).

use crate::geo::GeoTransformer;
use crate::grid::{
    binary_dilate, binary_erode, component_pixels, connected_components, dilate_component,
    gradient_magnitude, percentile, NODATA_THRESH,
};
use crate::types::{BagInfo, Knobs, MaskedRegion, RedactionSignature, M_TO_FT};
use ndarray::Array2;
use serde_json::json;

/// Aspect-ratio cap for masking regions (`masking_scanner.py` uses 8.0).
const MASK_MAX_ASPECT: f64 = 8.0;

// ============================================================================
// MASKING SCANNER (masking_scanner.py::MaskingDetector)
// ============================================================================

/// Raw masking hit before geo/scoring (mirrors the dict the Python detectors
/// return: type, pixel rows/cols, spans, area).
struct MaskHit {
    mask_type: &'static str,
    pixels: Vec<(usize, usize)>,
    long_ft: f64,
    short_ft: f64,
    area_sq_ft: f64,
}

/// Run the full masking scan over an elevation grid, producing [`MaskedRegion`]s.
///
/// Ported from `masking_scanner.py::scan_bag_for_masking` (detector +
/// restore + score loop).
pub fn detect_masking(
    elevation: &Array2<f64>,
    info: &BagInfo,
    geo: &GeoTransformer,
    knobs: &Knobs,
) -> Vec<MaskedRegion> {
    let (rows, cols) = elevation.dim();
    let res_m = info.resolution_m;
    let res_ft = res_m * M_TO_FT;

    let mut hits = Vec::new();
    hits.extend(detect_nan_holes(elevation, res_ft, knobs));
    hits.extend(detect_flattened_zones(elevation, res_m, res_ft, knobs));
    hits.extend(detect_texture_breaks(elevation, res_m, res_ft, knobs));

    let mut regions = Vec::new();
    for (i, hit) in hits.into_iter().enumerate() {
        let restore = restore_preview(elevation, &hit.pixels);
        let confidence = score_confidence(&hit, &restore);

        // Centroid + bbox in grid space.
        let r_min = hit.pixels.iter().map(|p| p.0).min().unwrap();
        let r_max = hit.pixels.iter().map(|p| p.0).max().unwrap();
        let c_min = hit.pixels.iter().map(|p| p.1).min().unwrap();
        let c_max = hit.pixels.iter().map(|p| p.1).max().unwrap();
        let cr = (r_min + r_max) as f64 / 2.0;
        let cc = (c_min + c_max) as f64 / 2.0;

        let (center_lat, center_lon) = geo.grid_to_latlon(cr, cc);
        let (sw_lat, sw_lon) = geo.grid_to_latlon(r_max as f64, c_min as f64);
        let (ne_lat, ne_lon) = geo.grid_to_latlon(r_min as f64, c_max as f64);

        // Great Lakes gate (mirrors MaskingDetector GL bbox use downstream).
        if knobs.enforce_great_lakes_bbox
            && !(knobs.gl_lat_min <= center_lat
                && center_lat <= knobs.gl_lat_max
                && knobs.gl_lon_min <= center_lon
                && center_lon <= knobs.gl_lon_max)
        {
            continue;
        }

        regions.push(MaskedRegion {
            id: format!("{}_mask{:03}", info.survey_id, i),
            bag_file: basename(&info.filepath),
            survey_id: info.survey_id.clone(),
            mask_type: hit.mask_type.to_string(),
            center_lat,
            center_lon,
            center_row: cr.round().clamp(0.0, (rows - 1) as f64) as usize,
            center_col: cc.round().clamp(0.0, (cols - 1) as f64) as usize,
            bbox_sw_lat: sw_lat,
            bbox_sw_lon: sw_lon,
            bbox_ne_lat: ne_lat,
            bbox_ne_lon: ne_lon,
            bbox_row_min: r_min,
            bbox_row_max: r_max,
            bbox_col_min: c_min,
            bbox_col_max: c_max,
            long_side_ft: hit.long_ft,
            short_side_ft: hit.short_ft,
            area_sq_ft: hit.area_sq_ft,
            cell_count: hit.pixels.len(),
            surrounding_depth_ft: restore.surrounding_depth_m * M_TO_FT,
            depth_variance_ft: restore.surrounding_std_m * M_TO_FT,
            restored_depth_ft: restore.restored_depth_m * M_TO_FT,
            depth_anomaly_ft: restore.depth_anomaly_m * M_TO_FT,
            confidence,
            tpu_boundary_score: 0.0,
            curvelet_proxy_score: 0.0,
            band2_ghost_score: 0.0,
            resolution_ft: res_ft,
            epsg: info.epsg_code,
        });
    }
    regions
}

/// `MaskingDetector.detect_nan_holes`: connected NaN regions that are
/// wreck-sized (>=4 cells, passes the 36x10ft + aspect<=8 gates).
fn detect_nan_holes(elevation: &Array2<f64>, res_ft: f64, _knobs: &Knobs) -> Vec<MaskHit> {
    let (rows, cols) = elevation.dim();
    let mut nan_mask = Array2::<bool>::from_elem((rows, cols), false);
    let mut any = false;
    for ((r, c), &v) in elevation.indexed_iter() {
        if !v.is_finite() {
            nan_mask[[r, c]] = true;
            any = true;
        }
    }
    if !any {
        return Vec::new();
    }

    let labels = connected_components(&nan_mask);
    let comps = component_pixels(&labels);

    let mut out = Vec::new();
    for pixels in comps {
        if pixels.len() < 4 {
            continue;
        }
        let (long_ft, short_ft) = spans_ft(&pixels, res_ft);
        if long_ft < 36.0 || short_ft < 10.0 {
            continue;
        }
        if long_ft / short_ft.max(0.01) > MASK_MAX_ASPECT {
            continue;
        }
        let area = pixels.len() as f64 * res_ft * res_ft;
        out.push(MaskHit {
            mask_type: "nan_hole",
            pixels,
            long_ft,
            short_ft,
            area_sq_ft: area,
        });
    }
    out
}

/// `MaskingDetector.detect_flattened_zones`: unnaturally flat zones where the
/// local std is < 5% of the global std, surrounded by normal-variance bottom.
fn detect_flattened_zones(
    elevation: &Array2<f64>,
    res_m: f64,
    res_ft: f64,
    _knobs: &Knobs,
) -> Vec<MaskHit> {
    let (rows, cols) = elevation.dim();
    let valid_count = elevation.iter().filter(|v| v.is_finite()).count();
    if valid_count < 1000 {
        return Vec::new();
    }

    // Global std + median fill.
    let mut vals: Vec<f64> = elevation.iter().copied().filter(|v| v.is_finite()).collect();
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let global_med = vals[vals.len() / 2];
    let mean: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
    let global_std =
        (vals.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / vals.len() as f64).sqrt();
    if global_std < 0.01 {
        return Vec::new();
    }

    let mut filled = elevation.clone();
    for v in filled.iter_mut() {
        if !v.is_finite() {
            *v = global_med;
        }
    }

    // Local std in a ~50m window via box mean of x and x^2.
    let mut window_px = (50.0 / res_m).max(3.0) as usize;
    if window_px % 2 == 0 {
        window_px += 1;
    }
    let local_mean = box_mean(&filled, window_px);
    let sq: Array2<f64> = filled.mapv(|x| x * x);
    let local_sq = box_mean(&sq, window_px);

    let flat_threshold = global_std * 0.05;
    let mut flat_mask = Array2::<bool>::from_elem((rows, cols), false);
    for r in 0..rows {
        for c in 0..cols {
            if elevation[[r, c]].is_finite() {
                let var = (local_sq[[r, c]] - local_mean[[r, c]] * local_mean[[r, c]]).max(0.0);
                if var.sqrt() < flat_threshold {
                    flat_mask[[r, c]] = true;
                }
            }
        }
    }

    let labels = connected_components(&flat_mask);
    let comps = component_pixels(&labels);

    let mut out = Vec::new();
    for pixels in comps {
        if pixels.len() < 8 {
            continue;
        }
        let (long_ft, short_ft) = spans_ft(&pixels, res_ft);
        if long_ft < 36.0 || short_ft < 10.0 {
            continue;
        }
        if long_ft / short_ft.max(0.01) > MASK_MAX_ASPECT {
            continue;
        }

        // Ring check: surroundings must have NORMAL variance (>=15% of global),
        // else it's just open flat bottom, not masking.
        // Windowed: dilate only this component (bbox+reach), not the full grid.
        let dilate_iter = window_px.max(3);
        let comp_set: std::collections::HashSet<(usize, usize)> =
            pixels.iter().copied().collect();
        let dilated = dilate_component(&pixels, dilate_iter, rows, cols);
        let mut ring_vals = Vec::new();
        for &(r, c) in &dilated {
            if !comp_set.contains(&(r, c)) && elevation[[r, c]].is_finite() {
                ring_vals.push(elevation[[r, c]]);
            }
        }
        if ring_vals.len() < 20 {
            continue;
        }
        let rmean: f64 = ring_vals.iter().sum::<f64>() / ring_vals.len() as f64;
        let ring_std = (ring_vals.iter().map(|v| (v - rmean) * (v - rmean)).sum::<f64>()
            / ring_vals.len() as f64)
            .sqrt();
        if ring_std < global_std * 0.15 {
            continue;
        }

        let area = pixels.len() as f64 * res_ft * res_ft;
        out.push(MaskHit {
            mask_type: "flattened",
            pixels,
            long_ft,
            short_ft,
            area_sq_ft: area,
        });
    }
    out
}

/// `MaskingDetector.detect_texture_breaks`: sharp gradient discontinuities at
/// region boundaries (top 2% gradient, dilated, wreck-sized connected patches).
fn detect_texture_breaks(
    elevation: &Array2<f64>,
    res_m: f64,
    res_ft: f64,
    _knobs: &Knobs,
) -> Vec<MaskHit> {
    let (rows, cols) = elevation.dim();
    let valid_count = elevation.iter().filter(|v| v.is_finite()).count();
    if valid_count < 1000 {
        return Vec::new();
    }

    let mut vals: Vec<f64> = elevation.iter().copied().filter(|v| v.is_finite()).collect();
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let global_med = vals[vals.len() / 2];

    let mut filled = elevation.mapv(|v| if v.is_finite() { v as f32 } else { global_med as f32 });
    let _ = &mut filled;
    // Gradient magnitude (central differences proxy for Sobel). Reuses the
    // bag_mesh.rs helper via crate::grid.
    let gradient = gradient_magnitude(&filled);

    // p98 of gradient over valid cells.
    let mut grad_vals: Vec<f32> = Vec::new();
    for ((r, c), &v) in elevation.indexed_iter() {
        if v.is_finite() {
            grad_vals.push(gradient[[r, c]]);
        }
    }
    if grad_vals.is_empty() {
        return Vec::new();
    }
    grad_vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let grad_p95 = percentile(&grad_vals, 95.0);
    if grad_p95 < 0.001 {
        return Vec::new();
    }
    let break_threshold = percentile(&grad_vals, 98.0);

    let mut break_mask = Array2::<bool>::from_elem((rows, cols), false);
    for ((r, c), &v) in elevation.indexed_iter() {
        if v.is_finite() && gradient[[r, c]] > break_threshold {
            break_mask[[r, c]] = true;
        }
    }

    // Dilate edges to merge nearby ones (iterations=3 in Python).
    let dilated = binary_dilate(&break_mask, 3);
    let labels = connected_components(&dilated);
    let comps = component_pixels(&labels);

    let _ = res_m;
    let mut out = Vec::new();
    for pixels in comps {
        if pixels.len() < 12 {
            continue;
        }
        let (long_ft, short_ft) = spans_ft(&pixels, res_ft);
        if long_ft < 36.0 || short_ft < 10.0 {
            continue;
        }
        if long_ft / short_ft.max(0.01) > MASK_MAX_ASPECT {
            continue;
        }
        let area = pixels.len() as f64 * res_ft * res_ft;
        out.push(MaskHit {
            mask_type: "texture_break",
            pixels,
            long_ft,
            short_ft,
            area_sq_ft: area,
        });
    }
    out
}

/// Result of `UnmaskPreview.restore`.
struct RestorePreview {
    restored_depth_m: f64,
    surrounding_depth_m: f64,
    depth_anomaly_m: f64,
    surrounding_std_m: f64,
}

/// `UnmaskPreview.restore`: estimate the depth hidden under a mask by sampling
/// the surrounding ring and a nearest-neighbour interpolation of the interior.
fn restore_preview(elevation: &Array2<f64>, pixels: &[(usize, usize)]) -> RestorePreview {
    let nan = RestorePreview {
        restored_depth_m: f64::NAN,
        surrounding_depth_m: f64::NAN,
        depth_anomaly_m: 0.0,
        surrounding_std_m: 0.0,
    };
    let (rows, cols) = elevation.dim();
    let r_min = pixels.iter().map(|p| p.0).min().unwrap();
    let r_max = pixels.iter().map(|p| p.0).max().unwrap();
    let c_min = pixels.iter().map(|p| p.1).min().unwrap();
    let c_max = pixels.iter().map(|p| p.1).max().unwrap();

    let margin = 30usize;
    let lr0 = r_min.saturating_sub(margin);
    let lr1 = (r_max + margin).min(rows - 1);
    let lc0 = c_min.saturating_sub(margin);
    let lc1 = (c_max + margin).min(cols - 1);

    // Mask within local window.
    let mut mask_set = std::collections::HashSet::new();
    for &(r, c) in pixels {
        mask_set.insert((r, c));
    }

    // Ring = dilation(mask, 5) minus mask, intersect valid.
    // Windowed: dilate only this component, not the full grid.
    let ring_dilated = crate::grid::dilate_component(pixels, 5, rows, cols);

    let mut ring_vals = Vec::new();
    let mut surround_pts: Vec<(f64, f64, f64)> = Vec::new(); // (r, c, val) valid & !mask
    for r in lr0..=lr1 {
        for c in lc0..=lc1 {
            let v = elevation[[r, c]];
            if !v.is_finite() {
                continue;
            }
            if mask_set.contains(&(r, c)) {
                continue;
            }
            surround_pts.push((r as f64, c as f64, v));
            if ring_dilated.contains(&(r, c)) {
                ring_vals.push(v);
            }
        }
    }

    if surround_pts.len() < 10 {
        return nan;
    }
    if ring_vals.len() < 5 {
        // Fall back to all valid non-mask cells in window.
        ring_vals = surround_pts.iter().map(|p| p.2).collect();
    }
    if ring_vals.len() < 5 {
        return nan;
    }

    let surr_depth = median(&mut ring_vals.clone());
    let rmean: f64 = ring_vals.iter().sum::<f64>() / ring_vals.len() as f64;
    let surr_std = (ring_vals.iter().map(|v| (v - rmean) * (v - rmean)).sum::<f64>()
        / ring_vals.len() as f64)
        .sqrt();

    if surround_pts.len() < 3 {
        return RestorePreview {
            restored_depth_m: surr_depth,
            surrounding_depth_m: surr_depth,
            depth_anomaly_m: 0.0,
            surrounding_std_m: surr_std,
        };
    }

    // Nearest-neighbour interpolation of masked interior, then median.
    //
    // We only need the MEDIAN of the restored values (it feeds depth_anomaly),
    // so an exact per-pixel NN over the full interior is wasteful: a large flat
    // mask gives O(mask_px * surround_px) ~ billions of ops. Subsample both
    // sides to a bounded cap — the median is statistically unchanged.
    const MAX_SAMPLE: usize = 256;
    let mask_sample: Vec<(usize, usize)> = if pixels.len() > MAX_SAMPLE {
        let stride = pixels.len() / MAX_SAMPLE;
        pixels.iter().step_by(stride.max(1)).copied().collect()
    } else {
        pixels.to_vec()
    };
    let surr_sample: Vec<(f64, f64, f64)> = if surround_pts.len() > MAX_SAMPLE {
        let stride = surround_pts.len() / MAX_SAMPLE;
        surround_pts.iter().step_by(stride.max(1)).copied().collect()
    } else {
        surround_pts.clone()
    };
    let mut restored_vals = Vec::with_capacity(mask_sample.len());
    for &(mr, mc) in &mask_sample {
        let mut best = f64::INFINITY;
        let mut best_val = surr_depth;
        for &(pr, pc, pv) in &surr_sample {
            let d = (pr - mr as f64) * (pr - mr as f64) + (pc - mc as f64) * (pc - mc as f64);
            if d < best {
                best = d;
                best_val = pv;
            }
        }
        restored_vals.push(best_val);
    }
    let restored_depth = if restored_vals.is_empty() {
        surr_depth
    } else {
        median(&mut restored_vals)
    };
    let anomaly = restored_depth - surr_depth; // positive = shallower = bump

    RestorePreview {
        restored_depth_m: restored_depth,
        surrounding_depth_m: surr_depth,
        depth_anomaly_m: anomaly,
        surrounding_std_m: surr_std,
    }
}

/// `_score_confidence`: base 0.3, plus per-type, size, and anomaly bonuses.
fn score_confidence(hit: &MaskHit, restore: &RestorePreview) -> f64 {
    let mut score = 0.3_f64;
    match hit.mask_type {
        "nan_hole" => score += 0.3,
        "flattened" => score += 0.2,
        "texture_break" => score += 0.1,
        _ => {}
    }
    let long_ft = hit.long_ft;
    if (50.0..=800.0).contains(&long_ft) {
        score += 0.2;
    } else if long_ft > 800.0 {
        score += 0.05;
    }
    let anomaly = restore.depth_anomaly_m.abs();
    if anomaly > 1.0 {
        score += 0.15;
    }
    score.min(0.99)
}

// ============================================================================
// UNCERTAINTY-BASED MASK DETECTION (bag_mesh.rs::detect_masked_regions)
// ============================================================================

/// Detect masked regions from the uncertainty band: connected clusters of
/// uncertainty below the p5 percentile, closed (dilate+erode) then eroded to
/// strip the boundary artifact ring.
///
/// Lifted/adapted from `bag_mesh.rs::detect_masked_regions`. Emits MaskedRegions
/// tagged `mask_type = "uncertainty_low"`.
pub fn detect_masked_regions_uncertainty(
    uncert: &Array2<f64>,
    info: &BagInfo,
    geo: &GeoTransformer,
    knobs: &Knobs,
) -> Vec<MaskedRegion> {
    let (rows, cols) = uncert.dim();
    let res_m = info.resolution_m;
    let res_ft = res_m * M_TO_FT;

    // Valid uncertainty values.
    let mut valid: Vec<f32> = uncert
        .iter()
        .copied()
        .filter(|&v| v > 0.0 && (v as f32) < NODATA_THRESH && v.is_finite())
        .map(|v| v as f32)
        .collect();
    if valid.len() < 100 {
        return Vec::new();
    }
    valid.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p_threshold = percentile(&valid, knobs.mask_uncertainty_pct) as f64;

    // Binary mask: below percentile.
    let mut mask = Array2::<bool>::from_elem((rows, cols), false);
    for ((r, c), &u) in uncert.indexed_iter() {
        if u > 0.0 && u < p_threshold && (u as f32) < NODATA_THRESH && u.is_finite() {
            mask[[r, c]] = true;
        }
    }

    // Binary closing (dilate then erode) to fill small gaps (bag_mesh: 3,3).
    mask = binary_dilate(&mask, 3);
    mask = binary_erode(&mask, 3);

    let labels = connected_components(&mask);
    let comps = component_pixels(&labels);

    let cell_area_m2 = res_m * res_m;
    let mut out = Vec::new();
    for (i, pixels) in comps.into_iter().enumerate() {
        let area_m2 = pixels.len() as f64 * cell_area_m2;
        let min_area = knobs.min_wreck_size_sq_ft / (M_TO_FT * M_TO_FT);
        let max_area = knobs.max_wreck_size_sq_ft / (M_TO_FT * M_TO_FT);
        if area_m2 < min_area || area_m2 > max_area {
            continue;
        }

        // Edge erosion to drop the boundary artifact ring.
        let r_min = pixels.iter().map(|p| p.0).min().unwrap();
        let r_max = pixels.iter().map(|p| p.0).max().unwrap();
        let c_min = pixels.iter().map(|p| p.1).min().unwrap();
        let c_max = pixels.iter().map(|p| p.1).max().unwrap();

        // Windowed erosion interior count (bbox-confined, not full grid).
        let interior_count = crate::grid::erode_component_interior_count(&pixels, knobs.mask_erosion_px);
        if interior_count < 5 {
            continue;
        }

        let (long_ft, short_ft) = spans_ft(&pixels, res_ft);
        let cr = (r_min + r_max) as f64 / 2.0;
        let cc = (c_min + c_max) as f64 / 2.0;
        let (center_lat, center_lon) = geo.grid_to_latlon(cr, cc);
        let (sw_lat, sw_lon) = geo.grid_to_latlon(r_max as f64, c_min as f64);
        let (ne_lat, ne_lon) = geo.grid_to_latlon(r_min as f64, c_max as f64);

        if knobs.enforce_great_lakes_bbox
            && !(knobs.gl_lat_min <= center_lat
                && center_lat <= knobs.gl_lat_max
                && knobs.gl_lon_min <= center_lon
                && center_lon <= knobs.gl_lon_max)
        {
            continue;
        }

        out.push(MaskedRegion {
            id: format!("{}_umask{:03}", info.survey_id, i),
            bag_file: basename(&info.filepath),
            survey_id: info.survey_id.clone(),
            mask_type: "uncertainty_low".to_string(),
            center_lat,
            center_lon,
            center_row: cr.round().clamp(0.0, (rows - 1) as f64) as usize,
            center_col: cc.round().clamp(0.0, (cols - 1) as f64) as usize,
            bbox_sw_lat: sw_lat,
            bbox_sw_lon: sw_lon,
            bbox_ne_lat: ne_lat,
            bbox_ne_lon: ne_lon,
            bbox_row_min: r_min,
            bbox_row_max: r_max,
            bbox_col_min: c_min,
            bbox_col_max: c_max,
            long_side_ft: long_ft,
            short_side_ft: short_ft,
            area_sq_ft: area_m2 * M_TO_FT * M_TO_FT,
            cell_count: pixels.len(),
            surrounding_depth_ft: 0.0,
            depth_variance_ft: 0.0,
            restored_depth_ft: 0.0,
            depth_anomaly_ft: 0.0,
            confidence: 0.6, // uncertainty-low clusters are a strong masking signal
            tpu_boundary_score: 0.0,
            curvelet_proxy_score: 0.0,
            band2_ghost_score: 0.0,
            resolution_ft: res_ft,
            epsg: info.epsg_code,
        });
    }
    out
}

/// Fused rescore pass for masked regions, implementing:
/// - Route 2: TPU boundary discontinuity around mask edges
/// - Route 1 (proxy): curvelet-like edge coherence from elevation gradients
/// - Route 3: raw Band2 ghost contrast (interior vs ring)
///
/// This is intentionally lightweight and windowed per-region so it scales on
/// large BAGs while preserving the strongest mask-candidate geometry.
pub fn fusion_rescore_regions(
    elevation: &Array2<f64>,
    uncertainty: Option<&Array2<f64>>,
    regions: &mut [MaskedRegion],
    knobs: &Knobs,
) {
    if regions.is_empty() || !knobs.enable_tpu_fusion {
        return;
    }
    let w_sum = (knobs.tpu_boundary_weight + knobs.curvelet_proxy_weight + knobs.band2_ghost_weight)
        .max(1e-6);
    for reg in regions.iter_mut() {
        let tpu = uncertainty
            .map(|u| tpu_boundary_step_score(u, reg, knobs.tpu_ring_px))
            .unwrap_or(0.0);
        let coh = curvelet_proxy_coherence_score(elevation, reg);
        let ghost = uncertainty
            .map(|u| band2_ghost_contrast_score(u, reg, knobs.tpu_ring_px))
            .unwrap_or(0.0);

        reg.tpu_boundary_score = tpu;
        reg.curvelet_proxy_score = coh;
        reg.band2_ghost_score = ghost;

        let fused = (knobs.tpu_boundary_weight * tpu
            + knobs.curvelet_proxy_weight * coh
            + knobs.band2_ghost_weight * ghost)
            / w_sum;

        // Keep prior detector confidence as anchor; fused score sharpens ranking.
        reg.confidence = (0.55 * reg.confidence + 0.45 * fused).clamp(0.0, 0.99);
    }
}

fn tpu_boundary_step_score(uncert: &Array2<f64>, reg: &MaskedRegion, ring_px: usize) -> f64 {
    let (rows, cols) = uncert.dim();
    let r0 = reg.bbox_row_min.saturating_sub(ring_px);
    let r1 = (reg.bbox_row_max + ring_px).min(rows.saturating_sub(1));
    let c0 = reg.bbox_col_min.saturating_sub(ring_px);
    let c1 = (reg.bbox_col_max + ring_px).min(cols.saturating_sub(1));
    if r0 >= r1 || c0 >= c1 {
        return 0.0;
    }

    let mut inside = Vec::new();
    let mut boundary = Vec::new();
    let mut ring = Vec::new();
    for r in r0..=r1 {
        for c in c0..=c1 {
            let v = uncert[[r, c]];
            if !(v.is_finite() && v > 0.0) {
                continue;
            }
            let in_box = r >= reg.bbox_row_min
                && r <= reg.bbox_row_max
                && c >= reg.bbox_col_min
                && c <= reg.bbox_col_max;
            let in_inner = r >= reg.bbox_row_min.saturating_add(1)
                && r + 1 <= reg.bbox_row_max
                && c >= reg.bbox_col_min.saturating_add(1)
                && c + 1 <= reg.bbox_col_max;
            if in_inner {
                inside.push(v);
            } else if in_box {
                boundary.push(v);
            } else {
                ring.push(v);
            }
        }
    }
    if inside.len() < 5 || ring.len() < 10 {
        return 0.0;
    }
    let i = mean(&inside);
    let b = mean(&boundary);
    let r = mean(&ring);
    let rs = stddev(&ring, r).max(1e-6);
    (((b - i).abs() + (b - r).abs()) / (2.0 * rs)).tanh()
}

fn band2_ghost_contrast_score(uncert: &Array2<f64>, reg: &MaskedRegion, ring_px: usize) -> f64 {
    let (rows, cols) = uncert.dim();
    let r0 = reg.bbox_row_min.saturating_sub(ring_px);
    let r1 = (reg.bbox_row_max + ring_px).min(rows.saturating_sub(1));
    let c0 = reg.bbox_col_min.saturating_sub(ring_px);
    let c1 = (reg.bbox_col_max + ring_px).min(cols.saturating_sub(1));
    if r0 >= r1 || c0 >= c1 {
        return 0.0;
    }
    let mut inside = Vec::new();
    let mut ring = Vec::new();
    for r in r0..=r1 {
        for c in c0..=c1 {
            let v = uncert[[r, c]];
            if !(v.is_finite() && v > 0.0) {
                continue;
            }
            if r >= reg.bbox_row_min
                && r <= reg.bbox_row_max
                && c >= reg.bbox_col_min
                && c <= reg.bbox_col_max
            {
                inside.push(v);
            } else {
                ring.push(v);
            }
        }
    }
    if inside.len() < 5 || ring.len() < 10 {
        return 0.0;
    }
    let mi = mean(&inside);
    let mr = mean(&ring);
    let sr = stddev(&ring, mr).max(1e-6);
    ((mr - mi) / sr).max(0.0).tanh()
}

fn curvelet_proxy_coherence_score(elevation: &Array2<f64>, reg: &MaskedRegion) -> f64 {
    let (rows, cols) = elevation.dim();
    if rows < 3 || cols < 3 {
        return 0.0;
    }
    let r0 = reg.bbox_row_min.saturating_sub(2).max(1);
    let r1 = (reg.bbox_row_max + 2).min(rows.saturating_sub(2));
    let c0 = reg.bbox_col_min.saturating_sub(2).max(1);
    let c1 = (reg.bbox_col_max + 2).min(cols.saturating_sub(2));
    if r0 >= r1 || c0 >= c1 {
        return 0.0;
    }

    let mut sxx = 0.0;
    let mut syy = 0.0;
    let mut sxy = 0.0;
    let mut n = 0.0;
    for r in r0..=r1 {
        for c in c0..=c1 {
            let z = elevation[[r, c]];
            if !z.is_finite() {
                continue;
            }
            let gx = (elevation[[r, c + 1]] - elevation[[r, c - 1]]) * 0.5;
            let gy = (elevation[[r + 1, c]] - elevation[[r - 1, c]]) * 0.5;
            if !(gx.is_finite() && gy.is_finite()) {
                continue;
            }
            sxx += gx * gx;
            syy += gy * gy;
            sxy += gx * gy;
            n += 1.0;
        }
    }
    if n < 10.0 {
        return 0.0;
    }
    sxx /= n;
    syy /= n;
    sxy /= n;
    let tr = sxx + syy;
    if tr <= 1e-9 {
        return 0.0;
    }
    let det = (sxx * syy - sxy * sxy).max(0.0);
    let disc = (tr * tr - 4.0 * det).max(0.0).sqrt();
    let l1 = 0.5 * (tr + disc);
    let l2 = 0.5 * (tr - disc);
    ((l1 - l2) / (l1 + l2 + 1e-9)).clamp(0.0, 1.0)
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f64>() / v.len() as f64
}

fn stddev(v: &[f64], m: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    (v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / v.len() as f64).sqrt()
}

// ============================================================================
// ELEVATION SIGNATURE DETECTORS (advanced_bag_scanner.py)
// ============================================================================

/// Run the four elevation/uncertainty signature detectors + redactor ID.
/// Ported from `advanced_bag_scanner.py::_analyze_redaction_signatures`.
///
/// Heavy on large rasters, hence gated by `enable_redaction_signatures`.
pub fn analyze_redaction_signatures(
    elevation: &Array2<f64>,
    uncertainty: Option<&Array2<f64>>,
    geo: &GeoTransformer,
    knobs: &Knobs,
) -> Vec<RedactionSignature> {
    let mut sigs = Vec::new();
    sigs.extend(detect_smoothing_signatures(elevation, geo, knobs));
    sigs.extend(detect_removal_signatures(elevation, geo, knobs));
    if let Some(u) = uncertainty {
        sigs.extend(detect_alteration_signatures(elevation, u, geo, knobs));
    }
    sigs.extend(detect_pattern_signatures(elevation, geo, knobs));
    identify_redactors(&mut sigs);
    sigs
}

/// `_detect_smoothing_signatures`: 20x20 windows with unnaturally low variance.
fn detect_smoothing_signatures(
    elevation: &Array2<f64>,
    geo: &GeoTransformer,
    knobs: &Knobs,
) -> Vec<RedactionSignature> {
    let (rows, cols) = elevation.dim();
    let kernel = 20usize;
    let mut sigs = Vec::new();
    if rows < kernel || cols < kernel {
        return sigs;
    }
    let mut i = 0;
    while i + kernel < rows {
        let mut j = 0;
        while j + kernel < cols {
            let mut window = Vec::with_capacity(kernel * kernel);
            for r in i..i + kernel {
                for c in j..j + kernel {
                    let v = elevation[[r, c]];
                    if v.is_finite() {
                        window.push(v);
                    }
                }
            }
            if window.len() as f64 >= (kernel * kernel) as f64 * 0.8 {
                let mean: f64 = window.iter().sum::<f64>() / window.len() as f64;
                let local_std = (window.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>()
                    / window.len() as f64)
                    .sqrt();
                let expected_std = (mean.abs() * 0.01).max(0.1);
                if local_std < expected_std * 0.5 {
                    let center_i = (i + kernel / 2) as f64;
                    let center_j = (j + kernel / 2) as f64;
                    let (lat, lon) = geo.grid_to_latlon(center_i, center_j);
                    let confidence = ((expected_std / (local_std + 1e-6) - 1.0) / 10.0).min(1.0);
                    if confidence > knobs.redaction_sensitivity {
                        let bbox_size = kernel as f64 * geo_resolution(geo);
                        sigs.push(RedactionSignature {
                            signature_type: "smoothing".into(),
                            confidence,
                            location: (lat, lon),
                            bounding_box: (
                                lon - bbox_size / 2.0,
                                lat - bbox_size / 2.0,
                                lon + bbox_size / 2.0,
                                lat + bbox_size / 2.0,
                            ),
                            size_pixels: kernel * kernel,
                            size_meters_sq: bbox_size * bbox_size,
                            redactor_id: None,
                            technique_used: "artificial_smoothing".into(),
                            evidence: json!({
                                "local_std": local_std,
                                "expected_std": expected_std,
                                "variance_ratio": local_std / expected_std,
                            }),
                        });
                    }
                }
            }
            j += kernel / 2;
        }
        i += kernel / 2;
    }
    sigs
}

/// `_detect_removal_signatures`: pixels too similar to neighbours in otherwise
/// variable areas (feature flattening). Sampled to keep cost bounded.
fn detect_removal_signatures(
    elevation: &Array2<f64>,
    geo: &GeoTransformer,
    knobs: &Knobs,
) -> Vec<RedactionSignature> {
    let (rows, cols) = elevation.dim();
    let mut sigs = Vec::new();
    if rows < 3 || cols < 3 {
        return sigs;
    }
    // The Python loops every pixel; we sample to keep this tractable on big
    // rasters while preserving the detector semantics.
    let step = 1usize;
    let mut i = 1;
    while i < rows - 1 {
        let mut j = 1;
        while j < cols - 1 {
            let center = elevation[[i, j]];
            if center.is_finite() {
                let mut neighbors = Vec::with_capacity(8);
                for di in -1i32..=1 {
                    for dj in -1i32..=1 {
                        if di == 0 && dj == 0 {
                            continue;
                        }
                        let ni = i as i32 + di;
                        let nj = j as i32 + dj;
                        if ni >= 0 && ni < rows as i32 && nj >= 0 && nj < cols as i32 {
                            let nv = elevation[[ni as usize, nj as usize]];
                            if nv.is_finite() {
                                neighbors.push(nv);
                            }
                        }
                    }
                }
                if neighbors.len() >= 4 {
                    let nmean: f64 = neighbors.iter().sum::<f64>() / neighbors.len() as f64;
                    let nstd = (neighbors.iter().map(|v| (v - nmean) * (v - nmean)).sum::<f64>()
                        / neighbors.len() as f64)
                        .sqrt();
                    if nstd > 0.1 {
                        let similarity = 1.0 - (center - nmean).abs() / (nstd * 2.0);
                        if similarity > 0.95 {
                            let (lat, lon) = geo.grid_to_latlon(i as f64, j as f64);
                            let confidence = similarity;
                            if confidence > knobs.redaction_sensitivity {
                                sigs.push(RedactionSignature {
                                    signature_type: "removal".into(),
                                    confidence,
                                    location: (lat, lon),
                                    bounding_box: (lon - 5.0, lat - 5.0, lon + 5.0, lat + 5.0),
                                    size_pixels: 1,
                                    size_meters_sq: 25.0,
                                    redactor_id: None,
                                    technique_used: "feature_flattening".into(),
                                    evidence: json!({
                                        "center_value": center,
                                        "neighbor_mean": nmean,
                                        "neighbor_std": nstd,
                                        "similarity": similarity,
                                    }),
                                });
                            }
                        }
                    }
                }
            }
            j += step;
        }
        i += step;
    }
    sigs
}

/// `_detect_alteration_signatures`: uncertainty much higher than expected in
/// shallow water (uncertainty manipulation). Samples every 10 px.
fn detect_alteration_signatures(
    elevation: &Array2<f64>,
    uncertainty: &Array2<f64>,
    geo: &GeoTransformer,
    knobs: &Knobs,
) -> Vec<RedactionSignature> {
    let (rows, cols) = elevation.dim();
    let mut sigs = Vec::new();
    let mut i = 0;
    while i < rows {
        let mut j = 0;
        while j < cols {
            let elev = elevation[[i, j]];
            let unc = uncertainty[[i, j]];
            if elev.is_finite() && unc.is_finite() && elev.abs() < 100.0 {
                let expected = (elev.abs() * 0.01).max(0.1);
                if unc > expected * 3.0 {
                    let (lat, lon) = geo.grid_to_latlon(i as f64, j as f64);
                    let confidence = (unc / (expected + 1e-6) / 5.0).min(1.0);
                    if confidence > knobs.redaction_sensitivity {
                        sigs.push(RedactionSignature {
                            signature_type: "alteration".into(),
                            confidence,
                            location: (lat, lon),
                            bounding_box: (lon - 10.0, lat - 10.0, lon + 10.0, lat + 10.0),
                            size_pixels: 100,
                            size_meters_sq: 100.0,
                            redactor_id: None,
                            technique_used: "uncertainty_manipulation".into(),
                            evidence: json!({
                                "elevation": elev,
                                "uncertainty": unc,
                                "expected_uncertainty": expected,
                                "ratio": unc / expected,
                            }),
                        });
                    }
                }
            }
            j += 10;
        }
        i += 10;
    }
    sigs
}

/// `_detect_pattern_signatures`: 2D autocorrelation periodicity.
///
/// STUBBED: the Python implementation depends on `scipy.signal.correlate2d`
/// over a sampled region. A full FFT/autocorrelation port is out of scope for
/// this pass; we leave a clearly-marked placeholder that returns no signatures.
/// TODO(pattern-autocorr): port `correlate2d` + secondary-peak periodicity.
fn detect_pattern_signatures(
    _elevation: &Array2<f64>,
    _geo: &GeoTransformer,
    _knobs: &Knobs,
) -> Vec<RedactionSignature> {
    Vec::new()
}

/// `_identify_redactors`: group signatures by technique; for techniques with
/// >=3 signatures, assign a redactor id to those within ~1km of the centroid.
fn identify_redactors(sigs: &mut [RedactionSignature]) {
    if sigs.len() < 2 {
        return;
    }
    // Collect technique -> indices.
    let mut groups: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (idx, s) in sigs.iter().enumerate() {
        groups.entry(s.technique_used.clone()).or_default().push(idx);
    }

    // Deterministic ordering of techniques.
    let mut techniques: Vec<String> = groups.keys().cloned().collect();
    techniques.sort();

    let mut redactor_id = 1;
    for technique in techniques {
        let idxs = &groups[&technique];
        if idxs.len() >= 3 {
            let clat: f64 = idxs.iter().map(|&k| sigs[k].location.0).sum::<f64>() / idxs.len() as f64;
            let clon: f64 = idxs.iter().map(|&k| sigs[k].location.1).sum::<f64>() / idxs.len() as f64;
            for &k in idxs {
                let d = ((sigs[k].location.0 - clat).powi(2) + (sigs[k].location.1 - clon).powi(2))
                    .sqrt();
                if d < 0.01 {
                    sigs[k].redactor_id = Some(format!("redactor_{redactor_id}"));
                }
            }
            redactor_id += 1;
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn spans_ft(pixels: &[(usize, usize)], res_ft: f64) -> (f64, f64) {
    let r_min = pixels.iter().map(|p| p.0).min().unwrap();
    let r_max = pixels.iter().map(|p| p.0).max().unwrap();
    let c_min = pixels.iter().map(|p| p.1).min().unwrap();
    let c_max = pixels.iter().map(|p| p.1).max().unwrap();
    let row_span_ft = (r_max - r_min + 1) as f64 * res_ft;
    let col_span_ft = (c_max - c_min + 1) as f64 * res_ft;
    (row_span_ft.max(col_span_ft), row_span_ft.min(col_span_ft))
}

/// Box-mean (uniform window) filter with reflected edges — approximates
/// `scipy.ndimage.convolve(x, ones/(w*w), mode="reflect")`.
fn box_mean(src: &Array2<f64>, window_px: usize) -> Array2<f64> {
    let (rows, cols) = src.dim();
    let r = (window_px / 2) as i32;
    let mut out = Array2::<f64>::zeros((rows, cols));
    let reflect = |x: i32, n: i32| -> usize {
        let mut v = x;
        if v < 0 {
            v = -v - 1;
        }
        if v >= n {
            v = 2 * n - v - 1;
        }
        v.clamp(0, n - 1) as usize
    };
    // Separable: horizontal then vertical box.
    let mut tmp = Array2::<f64>::zeros((rows, cols));
    let win = (2 * r + 1) as f64;
    for rr in 0..rows {
        for cc in 0..cols {
            let mut acc = 0.0;
            for d in -r..=r {
                let c = reflect(cc as i32 + d, cols as i32);
                acc += src[[rr, c]];
            }
            tmp[[rr, cc]] = acc / win;
        }
    }
    for rr in 0..rows {
        for cc in 0..cols {
            let mut acc = 0.0;
            for d in -r..=r {
                let rrr = reflect(rr as i32 + d, rows as i32);
                acc += tmp[[rrr, cc]];
            }
            out[[rr, cc]] = acc / win;
        }
    }
    out
}

fn median(vals: &mut [f64]) -> f64 {
    if vals.is_empty() {
        return f64::NAN;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = vals.len();
    if n % 2 == 1 {
        vals[n / 2]
    } else {
        (vals[n / 2 - 1] + vals[n / 2]) / 2.0
    }
}

fn basename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Best-effort cell resolution in meters from the transformer, for bbox sizing.
/// Falls back to 1.0 when unknown (only affects diagnostic bbox extent).
fn geo_resolution(_geo: &GeoTransformer) -> f64 {
    // The signature detectors only use this for a rough bbox; exact value is
    // not part of the validate_geo contract. Use 1.0 m as a neutral default.
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BagInfo;

    fn test_info(rows: usize, cols: usize, res: f64) -> BagInfo {
        BagInfo {
            filepath: "synthetic.bag".into(),
            survey_id: "synthetic".into(),
            shape: (rows, cols),
            sw_easting: 525_000.0,
            sw_northing: 5_070_000.0,
            ne_easting: 525_000.0 + cols as f64 * res,
            ne_northing: 5_070_000.0 + rows as f64 * res,
            resolution_m: res,
            crs_wkt: String::new(),
            epsg_code: 32616,
            vertical_datum: "Unknown".into(),
            nodata_value: 1_000_000.0,
            valid_cell_count: rows * cols,
            total_cell_count: rows * cols,
            depth_min: -40.0,
            depth_max: -30.0,
            has_uncertainty: false,
            read_step: 1,
        }
    }

    #[test]
    fn nan_hole_detected_as_masked_region() {
        // 80x80 grid at 2m cells, a 20x16 NaN hole (40m x 32m -> 131ft x 105ft).
        let res = 2.0;
        let rows = 80;
        let cols = 80;
        let mut elev = Array2::<f64>::from_elem((rows, cols), -35.0);
        for r in 30..50 {
            for c in 30..46 {
                elev[[r, c]] = f64::NAN;
            }
        }
        let info = test_info(rows, cols, res);
        let geo = GeoTransformer::from_epsg(32616, info.sw_easting, info.sw_northing, res);
        let regions = detect_masking(&elev, &info, &geo, &Knobs::default());
        let nan_holes: Vec<_> = regions.iter().filter(|r| r.mask_type == "nan_hole").collect();
        assert!(!nan_holes.is_empty(), "expected a nan_hole region");
        let reg = nan_holes[0];
        assert!(reg.confidence > 0.3, "confidence should exceed base");
        assert!(reg.area_sq_ft > 0.0);
        assert!(reg.long_side_ft >= 36.0 && reg.short_side_ft >= 10.0);
    }

    #[test]
    fn flattened_zone_detected() {
        // Variable bottom (textured) with a large flat rectangular patch.
        // Use 5 m cells so the ~50 m local-std window (10 px) is smaller than
        // the flat patch, giving a genuinely flat window interior.
        let res = 5.0;
        let rows = 100;
        let cols = 100;
        let mut elev = Array2::<f64>::zeros((rows, cols));
        for r in 0..rows {
            for c in 0..cols {
                // Rough textured bottom.
                let v = -30.0
                    + 3.0 * ((r as f64 * 0.7).sin() + (c as f64 * 0.9).cos())
                    + ((r * 13 + c * 7) % 5) as f64 * 0.5;
                elev[[r, c]] = v;
            }
        }
        // Flatten a large 35x30 patch (175m x 150m) to a constant depth.
        for r in 35..70 {
            for c in 35..65 {
                elev[[r, c]] = -31.0;
            }
        }
        let info = test_info(rows, cols, res);
        let geo = GeoTransformer::from_epsg(32616, info.sw_easting, info.sw_northing, res);
        let mut knobs = Knobs::default();
        knobs.enforce_great_lakes_bbox = false;
        let regions = detect_masking(&elev, &info, &geo, &knobs);
        assert!(
            regions.iter().any(|r| r.mask_type == "flattened"),
            "expected a flattened region; got {:?}",
            regions.iter().map(|r| &r.mask_type).collect::<Vec<_>>()
        );
    }

    #[test]
    fn smoothing_signature_detected_in_flat_block() {
        // A grid that is mostly textured but has a large unnaturally smooth area.
        let rows = 60;
        let cols = 60;
        let mut elev = Array2::<f64>::zeros((rows, cols));
        for r in 0..rows {
            for c in 0..cols {
                elev[[r, c]] = -50.0 + ((r * 7 + c * 11) % 17) as f64 * 0.8;
            }
        }
        // Smooth (constant) block 0..40 x 0..40 -> several 20x20 windows hit.
        for r in 0..40 {
            for c in 0..40 {
                elev[[r, c]] = -50.0;
            }
        }
        let info = test_info(rows, cols, 1.0);
        let geo = GeoTransformer::from_epsg(32616, info.sw_easting, info.sw_northing, 1.0);
        let mut knobs = Knobs::default();
        knobs.redaction_sensitivity = 0.2; // lower so the smooth block registers
        knobs.enforce_great_lakes_bbox = false;
        let sigs = analyze_redaction_signatures(&elev, None, &geo, &knobs);
        assert!(
            sigs.iter().any(|s| s.signature_type == "smoothing"),
            "expected a smoothing signature"
        );
    }

    #[test]
    fn identify_redactors_groups_by_technique() {
        // Three smoothing signatures near each other -> share a redactor id.
        let mut sigs = vec![];
        for k in 0..3 {
            sigs.push(RedactionSignature {
                signature_type: "smoothing".into(),
                confidence: 0.8,
                location: (45.0 + k as f64 * 0.0001, -84.0 + k as f64 * 0.0001),
                bounding_box: (0.0, 0.0, 0.0, 0.0),
                size_pixels: 400,
                size_meters_sq: 400.0,
                redactor_id: None,
                technique_used: "artificial_smoothing".into(),
                evidence: serde_json::Value::Null,
            });
        }
        identify_redactors(&mut sigs);
        assert!(
            sigs.iter().all(|s| s.redactor_id.is_some()),
            "all 3 close same-technique sigs should get a redactor id"
        );
    }
}
