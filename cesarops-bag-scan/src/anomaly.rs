//! Physical wreck (depth-anomaly) detection.
//!
//! Ported primarily from `bag_wreck_detector.py::AnomalyDetector` with the
//! local-baseline height idea from `standalone_bag_scanner.py::detect_anomalies_with_height`.
//!
//! Detection flow (matching `AnomalyDetector.detect`):
//!   1. Compute a smoothed "background" seafloor (Gaussian blur of NaN-filled
//!      elevation, sigma ~ 100m in cells, capped) — `_compute_background`.
//!   2. Find cells significantly SHALLOWER than background by `min_height_m`
//!      (`height_above = elevation - background`) — `_find_anomalies`.
//!   3. Cluster adjacent anomaly cells (8-connected) — `_cluster_anomalies`,
//!      applying the `MAX_ASPECT_RATIO` stitching-seam filter.
//!   4. Convert each cluster to a detection: centroid -> UTM -> lat/lon,
//!      Great-Lakes bbox gate, 36x10 ft min-size gate, confidence, ObjectType
//!      — `_cluster_to_detection`.
//!
//! All grid helpers (connected components, PCA) come from [`crate::grid`],
//! lifted from `bag_mesh.rs`.

use crate::geo::GeoTransformer;
use crate::grid::{component_pixels, connected_components};
use crate::types::{BagInfo, Knobs, ObjectType, WreckCandidate, M_TO_FT, SQM_TO_SQFT};
use ndarray::Array2;

/// Run physical-anomaly detection over the elevation grid.
///
/// Returns intermediate [`WreckCandidate`]s (orientation/dedup/contract
/// conversion happen in later stages).
pub fn detect(
    elevation: &Array2<f64>,
    info: &BagInfo,
    geo: &GeoTransformer,
    knobs: &Knobs,
) -> Vec<WreckCandidate> {
    detect_with_pixels(elevation, info, geo, knobs)
        .into_iter()
        .map(|(c, _)| c)
        .collect()
}

/// Like [`detect`], but also returns each candidate's (row, col) pixel list so
/// later stages (orientation) can run PCA on the exact cluster geometry.
pub fn detect_with_pixels(
    elevation: &Array2<f64>,
    info: &BagInfo,
    geo: &GeoTransformer,
    knobs: &Knobs,
) -> Vec<(WreckCandidate, Vec<(usize, usize)>)> {
    if info.valid_cell_count < 100 {
        return Vec::new();
    }

    let res = info.resolution_m;
    let _ = res;

    // 1. Background seafloor.
    let background = compute_background(elevation, info.resolution_m);

    // 2. Height-above-floor anomaly mask.
    let anomaly_mask = find_anomalies(elevation, &background, knobs.min_height_m);

    // 3. Cluster.
    let labels = connected_components(&anomaly_mask);
    let clusters = component_pixels(&labels);

    let mut out = Vec::new();
    for pixels in clusters.into_iter() {
        if pixels.len() < knobs.min_cluster_cells {
            continue;
        }
        if let Some(cand) =
            cluster_to_candidate(&pixels, elevation, &background, info, geo, knobs)
        {
            // min_confidence gate (advanced_bag_scanner._filter_and_rank_candidates).
            if cand.confidence >= knobs.min_confidence {
                out.push((cand, pixels));
            }
        }
    }
    out
}

/// Compute a smoothed background seafloor.
/// Ported from `AnomalyDetector._compute_background` (Gaussian path):
///   * sigma_cells = clamp(100m / resolution, 2, 30)
///   * fill NaN with the global median, blur, then restore NaN positions.
fn compute_background(elevation: &Array2<f64>, resolution_m: f64) -> Array2<f64> {
    let (rows, cols) = elevation.dim();
    let sigma = (100.0 / resolution_m.max(1e-6)).clamp(2.0, 30.0);

    // Global median for NaN fill.
    let mut vals: Vec<f64> = elevation.iter().copied().filter(|v| v.is_finite()).collect();
    if vals.is_empty() {
        return Array2::<f64>::from_elem((rows, cols), f64::NAN);
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = vals[vals.len() / 2];

    let mut filled = elevation.clone();
    for v in filled.iter_mut() {
        if !v.is_finite() {
            *v = median;
        }
    }

    let blurred = gaussian_blur(&filled, sigma);

    // Restore NaN positions (background[nan_mask] = nan).
    let mut bg = blurred;
    for ((r, c), &e) in elevation.indexed_iter() {
        if !e.is_finite() {
            bg[[r, c]] = f64::NAN;
        }
    }
    bg
}

/// Separable Gaussian blur with a truncated kernel (radius = 3*sigma).
/// Approximates `scipy.ndimage.gaussian_filter` used by the Python reference.
fn gaussian_blur(src: &Array2<f64>, sigma: f64) -> Array2<f64> {
    let (rows, cols) = src.dim();
    let radius = (sigma * 3.0).ceil() as i32;
    if radius < 1 {
        return src.clone();
    }
    // 1D kernel.
    let mut kernel = Vec::with_capacity((2 * radius + 1) as usize);
    let mut ksum = 0.0;
    for d in -radius..=radius {
        let w = (-(d as f64 * d as f64) / (2.0 * sigma * sigma)).exp();
        kernel.push(w);
        ksum += w;
    }
    for k in kernel.iter_mut() {
        *k /= ksum;
    }

    // Horizontal pass.
    let mut tmp = Array2::<f64>::zeros((rows, cols));
    for r in 0..rows {
        for c in 0..cols {
            let mut acc = 0.0;
            for (ki, d) in (-radius..=radius).enumerate() {
                let cc = (c as i32 + d).clamp(0, cols as i32 - 1) as usize;
                acc += src[[r, cc]] * kernel[ki];
            }
            tmp[[r, c]] = acc;
        }
    }
    // Vertical pass.
    let mut out = Array2::<f64>::zeros((rows, cols));
    for r in 0..rows {
        for c in 0..cols {
            let mut acc = 0.0;
            for (ki, d) in (-radius..=radius).enumerate() {
                let rr = (r as i32 + d).clamp(0, rows as i32 - 1) as usize;
                acc += tmp[[rr, c]] * kernel[ki];
            }
            out[[r, c]] = acc;
        }
    }
    out
}

/// Find cells shallower than background by at least `min_height_m`.
/// Ported from `AnomalyDetector._find_anomalies`.
fn find_anomalies(
    elevation: &Array2<f64>,
    background: &Array2<f64>,
    min_height_m: f64,
) -> Array2<bool> {
    let (rows, cols) = elevation.dim();
    let mut mask = Array2::<bool>::from_elem((rows, cols), false);
    for r in 0..rows {
        for c in 0..cols {
            let e = elevation[[r, c]];
            let b = background[[r, c]];
            if e.is_finite() && b.is_finite() {
                // Wreck = shallower (less negative): height_above = e - b >= min.
                if e - b >= min_height_m {
                    mask[[r, c]] = true;
                }
            }
        }
    }
    mask
}

/// Convert a cluster (pixel list) into a [`WreckCandidate`], applying the
/// aspect-ratio, Great-Lakes bbox, and min-size gates.
/// Ported from `_cluster_anomalies` (aspect filter) + `_cluster_to_detection`.
#[allow(clippy::too_many_arguments)]
fn cluster_to_candidate(
    pixels: &[(usize, usize)],
    elevation: &Array2<f64>,
    background: &Array2<f64>,
    info: &BagInfo,
    geo: &GeoTransformer,
    knobs: &Knobs,
) -> Option<WreckCandidate> {
    let res = info.resolution_m;

    let r_min = pixels.iter().map(|p| p.0).min()?;
    let r_max = pixels.iter().map(|p| p.0).max()?;
    let c_min = pixels.iter().map(|p| p.1).min()?;
    let c_max = pixels.iter().map(|p| p.1).max()?;

    // ── Stitching-seam aspect filter (from _cluster_anomalies) ──
    let row_span = (r_max - r_min + 1) as f64 * res;
    let col_span = (c_max - c_min + 1) as f64 * res;
    let long_side = row_span.max(col_span);
    let short_side = row_span.min(col_span).max(0.01);
    if long_side / short_side > knobs.max_aspect_ratio {
        return None;
    }

    // Cluster height stats.
    let mut heights = Vec::new();
    for &(r, c) in pixels {
        let e = elevation[[r, c]];
        let b = background[[r, c]];
        if e.is_finite() && b.is_finite() {
            heights.push(e - b);
        }
    }
    if heights.is_empty() {
        return None;
    }
    let max_height = heights.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    // Centroid.
    let center_row = pixels.iter().map(|p| p.0).sum::<usize>() as f64 / pixels.len() as f64;
    let center_col = pixels.iter().map(|p| p.1).sum::<usize>() as f64 / pixels.len() as f64;
    let cr = center_row.round() as usize;
    let cc = center_col.round() as usize;

    // Coordinates: grid -> UTM -> lat/lon.
    let (easting, northing) = geo.grid_to_projected(center_row, center_col);
    let (lat, lon) = geo.projected_to_latlon(easting, northing);

    // Great Lakes bbox sanity check (only when enabled, e.g. lake domain).
    if knobs.enforce_great_lakes_bbox
        && !(knobs.gl_lat_min <= lat
            && lat <= knobs.gl_lat_max
            && knobs.gl_lon_min <= lon
            && lon <= knobs.gl_lon_max)
    {
        return None;
    }

    // Depth at center (fall back to cluster mean).
    let depth = {
        let d = elevation[[cr.min(info.shape.0 - 1), cc.min(info.shape.1 - 1)]];
        if d.is_finite() {
            d
        } else {
            let mut s = 0.0;
            let mut n = 0;
            for &(r, c) in pixels {
                let e = elevation[[r, c]];
                if e.is_finite() {
                    s += e;
                    n += 1;
                }
            }
            if n > 0 {
                s / n as f64
            } else {
                0.0
            }
        }
    };

    // Size estimate (extent in meters), feet, min-size gate.
    let size_m = row_span.max(col_span);
    let size_ft = size_m * M_TO_FT;
    let long_side_ft = long_side * M_TO_FT;
    let short_side_ft = short_side * M_TO_FT;
    if long_side_ft < knobs.min_long_side_ft || short_side_ft < knobs.min_short_side_ft {
        return None;
    }

    // Footprint area (cells * cell area). Used for the size_sq_feet contract.
    let cell_area_m2 = res * res;
    let size_sq_meters = pixels.len() as f64 * cell_area_m2;
    let size_sq_feet = size_sq_meters * SQM_TO_SQFT;

    // Apply the advanced-scanner size gates on square footage.
    if size_sq_feet < knobs.min_wreck_size_sq_ft || size_sq_feet > knobs.max_wreck_size_sq_ft {
        return None;
    }

    // Confidence (from _cluster_to_detection):
    // min(0.99, 0.5 + height*0.05 + (cell_count/100)*0.2)
    let confidence =
        (0.5 + max_height * 0.05 + (pixels.len() as f64 / 100.0) * 0.2).min(0.99);

    let object_type = ObjectType::from_size_feet(size_ft);

    Some(WreckCandidate {
        center_row: cr,
        center_col: cc,
        easting,
        northing,
        latitude: lat,
        longitude: lon,
        depth_meters: depth.abs(),
        height_above_floor_m: max_height,
        size_meters: size_m,
        size_sq_meters,
        size_sq_feet,
        long_side_ft,
        short_side_ft,
        length_m: long_side,
        width_m: short_side,
        aspect_ratio: long_side / short_side,
        cell_count: pixels.len(),
        confidence,
        object_type,
        heading_deg: 0.0,
        heading_alt_deg: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_with_bump(rows: usize, cols: usize, depth: f64) -> Array2<f64> {
        Array2::<f64>::from_elem((rows, cols), depth)
    }

    #[test]
    fn find_anomalies_detects_shallow_bump() {
        // Flat bottom at -30, a 3x3 bump at -20 (10m shallower).
        let mut elev = flat_with_bump(20, 20, -30.0);
        for r in 8..11 {
            for c in 8..11 {
                elev[[r, c]] = -20.0;
            }
        }
        let bg = flat_with_bump(20, 20, -30.0);
        let mask = find_anomalies(&elev, &bg, 1.8);
        assert!(mask[[9, 9]]);
        assert!(!mask[[0, 0]]);
    }

    #[test]
    fn clustering_groups_adjacent_anomaly_cells() {
        // Build an elevation grid with one compact bump that should cluster.
        let res = 1.0;
        let mut elev = Array2::<f64>::from_elem((60, 60), -30.0);
        // A ~15x12 cell bump, 5m proud. (15m x 12m ~ 49ft x 39ft -> passes 36x10ft)
        for r in 20..35 {
            for c in 20..32 {
                elev[[r, c]] = -25.0;
            }
        }
        let bg = compute_background(&elev, res);
        let mask = find_anomalies(&elev, &bg, 1.8);
        let labels = connected_components(&mask);
        let clusters = component_pixels(&labels);
        // There should be at least one cluster of meaningful size.
        let big = clusters.iter().filter(|p| p.len() >= 8).count();
        assert!(big >= 1, "expected a clustered anomaly, got {big}");
    }

    #[test]
    fn detect_end_to_end_yields_candidate_with_latlon() {
        // Synthetic UTM16N grid in the Great Lakes; 1m cells.
        // SW easting/northing chosen to land near lat ~45, lon ~ -84.7.
        let res = 1.0;
        let rows = 80;
        let cols = 80;
        let mut elev = Array2::<f64>::from_elem((rows, cols), -40.0);
        // 20x14 cell bump ~6m proud -> wreck-sized.
        for r in 30..50 {
            for c in 30..44 {
                elev[[r, c]] = -34.0;
            }
        }

        let info = BagInfo {
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
            depth_max: -34.0,
            has_uncertainty: false,
            read_step: 1,
        };
        // Use SW-corner formula (no affine) for deterministic mapping.
        let geo = GeoTransformer::from_epsg(32616, info.sw_easting, info.sw_northing, res);

        let mut knobs = Knobs::default();
        // The synthetic point sits in the Great Lakes box, keep the gate on.
        knobs.min_confidence = 0.0;
        let cands = detect(&elev, &info, &geo, &knobs);
        assert!(!cands.is_empty(), "expected at least one candidate");
        let c = &cands[0];
        assert!(c.latitude > 41.3 && c.latitude < 49.0, "lat={}", c.latitude);
        assert!(c.longitude > -92.2 && c.longitude < -76.0, "lon={}", c.longitude);
        assert!(c.size_sq_feet > 0.0);
        assert!(c.height_above_floor_m > 1.8);
    }
}
