//! Temporal stack engine — per-chip multi-scene ratio persistence.
//!
//! Ports `run_temporal_stack_mission`, `_stac_scenes`, `_chip_ratio_ndwi`,
//! `_zscore_series` from temporal_stack_engine.py.
//!
//! Strategy (mirrors Python slicer_alternative_spec.md approach):
//!   1. Fetch STAC scene catalog for the AOI (POST search, no mosaic stitch)
//!   2. For each wreck, request B03/B08 chips per scene
//!   3. Compute per-scene NDWI / NDVI at the chip centre
//!   4. Flag pixels / wrecks where |z-score| > threshold across time (persistence)
//!   5. Emit per-wreck temporal report JSON

use crate::{
    chip::{chip_bbox, download_cog_chip},
    overlay_grid::{OverlayGrid, OverlayGridConfig, TileStamp},
    spectral::{nanmean, zscore_last},
    stac::{search_scenes_post, StacQuery},
    types::{BBox, Knobs, WreckTarget},
};
use anyhow::Result;
use chrono::Local;
use ndarray::Array2;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{info, warn};

// ── Report types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WreckStackEntry {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// How many scenes had valid chip data
    pub n_scenes_used: usize,
    pub ndwi_persistence_z: Option<f64>,
    pub ndvi_persistence_z: Option<f64>,
    /// true if |max_z| > persistence threshold
    pub anomaly: bool,
    pub overlay_markers: usize,
    pub overlay_anchor_hash: Option<u64>,
    pub overlay_alignment_dx_px: Option<f32>,
    pub overlay_alignment_dy_px: Option<f32>,
    pub overlay_alignment_confidence: Option<f32>,
    pub overlay_alignment_valid_scenes: usize,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalStackReport {
    pub bbox: [f64; 4],
    pub date_range: [String; 2],
    pub n_scenes_catalog: usize,
    pub scene_ids: Vec<String>,
    pub ratio_channels: Vec<String>,
    pub tiles_per_gpu_window: usize,
    pub gpu_windows: usize,
    pub wrecks: Vec<WreckStackEntry>,
}

#[derive(Clone, Copy)]
struct SceneAlignment {
    dx: f32,
    dy: f32,
    confidence: f32,
    valid: bool,
}

fn marker_observation(
    grid: &Array2<f32>,
    exp_x: f32,
    exp_y: f32,
    search_radius_px: usize,
) -> Option<(f32, f32)> {
    let (h, w) = grid.dim();
    if h == 0 || w == 0 {
        return None;
    }

    let cx = exp_x.round() as isize;
    let cy = exp_y.round() as isize;
    let r = search_radius_px as isize;

    let x0 = (cx - r).max(0) as usize;
    let y0 = (cy - r).max(0) as usize;
    let x1 = (cx + r).min(w as isize - 1) as usize;
    let y1 = (cy + r).min(h as isize - 1) as usize;

    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    let mut weight_sum = 0.0_f64;
    let mut x_sum = 0.0_f64;
    let mut y_sum = 0.0_f64;

    for y in y0..=y1 {
        for x in x0..=x1 {
            let v = grid[[y, x]];
            if !v.is_finite() {
                continue;
            }
            let wgt = (v as f64).abs();
            if wgt < 1e-6 {
                continue;
            }
            weight_sum += wgt;
            x_sum += x as f64 * wgt;
            y_sum += y as f64 * wgt;
        }
    }

    if weight_sum < 1e-4 {
        return None;
    }

    Some(((x_sum / weight_sum) as f32, (y_sum / weight_sum) as f32))
}

fn collect_observations(
    grid: &Array2<f32>,
    stamp: &TileStamp,
    search_radius_px: usize,
    central_only: bool,
    max_markers: usize,
) -> Vec<(f32, f32, u64)> {
    let mut out = Vec::new();
    let cx = stamp.tile_width_px as f32 * 0.5;
    let cy = stamp.tile_height_px as f32 * 0.5;
    let rx = stamp.tile_width_px as f32 * 0.35;
    let ry = stamp.tile_height_px as f32 * 0.35;

    for m in &stamp.markers {
        if central_only {
            let in_center = (m.pixel_x - cx).abs() <= rx && (m.pixel_y - cy).abs() <= ry;
            if !in_center {
                continue;
            }
        }
        if let Some((ox, oy)) = marker_observation(grid, m.pixel_x, m.pixel_y, search_radius_px) {
            out.push((ox, oy, m.marker.hash));
            if out.len() >= max_markers {
                break;
            }
        }
    }

    out
}

fn scene_alignment_from_grid(
    overlay: &OverlayGrid,
    stamp: &TileStamp,
    metric_grid: &Array2<f32>,
) -> Option<SceneAlignment> {
    let observed_outer = collect_observations(metric_grid, stamp, 6, false, 96);
    let observed_inner = collect_observations(metric_grid, stamp, 4, true, 64);
    if observed_outer.len() < 4 || observed_inner.len() < 4 {
        return None;
    }

    // Full similarity fit (scale + rotation + translation) on the outer marker
    // set. A deep temporal stack misregisters by rotation/skew, not pure
    // translation — fitting θ and s directly keeps the residual subpixel instead
    // of letting a linear (dx,dy) approximation of a trigonometric problem
    // compound across layers into gross drift (the real cause of the historic
    // "20-mile" stacks).  See overlay_grid::estimate_similarity.
    let sim = overlay.estimate_similarity(&observed_outer, stamp);

    // Translation cross-check on the inner set guards against a degenerate fit.
    let inner = overlay.align(&observed_inner, stamp);

    // Effective translation at the chip centre from the similarity transform:
    // map the centre through the inverse and take the residual shift.
    let cx = stamp.tile_width_px as f32 * 0.5;
    let cy = stamp.tile_height_px as f32 * 0.5;
    let (rcx, rcy) = sim.invert_point(cx, cy);
    let dx = cx - rcx;
    let dy = cy - rcy;

    // Agreement between the similarity-derived centre shift and the independent
    // inner-set translation. Large disagreement ⇒ untrustworthy fit.
    let agreement = ((dx - inner.dx).powi(2) + (dy - inner.dy).powi(2)).sqrt();

    // Gross-failure guard (defence in depth on top of the similarity fit): a
    // real registration correction is small. Reject anything beyond MAX_DRIFT_PX.
    const MAX_DRIFT_PX: f32 = 8.0; // ~80 m at Sentinel-2 10 m/px
    let offset_mag = (dx * dx + dy * dy).sqrt();
    let within_bounds = offset_mag <= MAX_DRIFT_PX;

    // Reject implausible rotation/scale too — these would warp, not just shift.
    let rotation_ok = sim.theta.abs() <= 10.0_f32.to_radians(); // ±10°
    let scale_ok = (sim.scale - 1.0).abs() <= 0.1; // ±10%

    let conf = sim.confidence.min(inner.confidence) * (1.0 - (agreement / 2.0).min(1.0));

    Some(SceneAlignment {
        dx,
        dy,
        confidence: conf,
        valid: sim.is_valid()
            && inner.is_valid()
            && agreement <= 1.0
            && within_bounds
            && rotation_ok
            && scale_ok,
    })
}

// ── Core function ─────────────────────────────────────────────────────────────

pub async fn run_temporal_stack_mission(
    client: &reqwest::Client,
    bbox: BBox,
    wrecks: &[WreckTarget],
    output_dir: &Path,
    knobs: &Knobs,
    days_back: u32,
    chip_cache_dir: &Path,
) -> Result<TemporalStackReport> {
    std::fs::create_dir_all(output_dir)?;

    let end = Local::now().naive_local().date();
    let start = end - chrono::Duration::days(days_back as i64);
    let max_scenes = knobs.temporal_max_scenes;
    let max_cloud = knobs.max_cloud;
    let persistence_z = knobs.temporal_persistence_z;
    let chip_radius_m = knobs.temporal_chip_radius_m;
    let channels = &knobs.temporal_ratio_channels;

    let query = StacQuery {
        bbox: bbox.to_stac_array(),
        date_start: start,
        date_end: end,
        max_cloud,
        month_filter: &[],
        limit: max_scenes,
    };

    let scenes = match search_scenes_post(client, &query).await {
        Ok(s) => {
            info!("Temporal stack: {} STAC scenes in window", s.len());
            s
        }
        Err(e) => {
            warn!("STAC scene fetch failed: {e}");
            vec![]
        }
    };

    let scene_ids: Vec<String> = scenes.iter().map(|s| s.id.chars().take(40).collect()).collect();

    let chip_px = (2.0 * chip_radius_m / 10.0).ceil() as usize; // 10 m/px Sentinel
    let overlay = OverlayGrid::new(OverlayGridConfig::default());

    let mut wreck_entries: Vec<WreckStackEntry> = Vec::new();

    for wreck in wrecks {
        let chip_bb = chip_bbox(wreck.lat, wreck.lon, chip_radius_m);
        let deg_per_px_lat = 10.0_f64 / 111_320.0_f64;
        let deg_per_px_lon = 10.0_f64 / (111_320.0_f64 * wreck.lat.to_radians().cos().abs().max(1e-6));
        // Snap the stamp origin to a fixed global grid so EVERY date of this AOI
        // stamps identical marker cells. Without this, reprojection rounding can
        // shift the origin between scenes onto different global cells, breaking
        // the hash-keyed marker match (the root cause of cross-scene drift —
        // see docs/SAR_RELEASE_HARDENING.md §1 cause #3).
        let cell = OverlayGridConfig::default().cell_size_px as f64;
        let snap = |v: f64, step: f64| (v / step).floor() * step;
        let snapped_lon = snap(chip_bb.lon_min, cell * deg_per_px_lon);
        let snapped_lat = snap(chip_bb.lat_min, cell * deg_per_px_lat);
        let stamp = overlay.stamp(
            chip_px,
            chip_px,
            snapped_lon,
            snapped_lat,
            deg_per_px_lon,
            deg_per_px_lat,
        );
        let overlay_markers = stamp.markers.len();
        let overlay_anchor_hash = stamp.markers.first().map(|m| m.marker.hash);

        let mut ndwi_series: Vec<f64> = Vec::new();
        let mut ndvi_series: Vec<f64> = Vec::new();
        let mut n_used: usize = 0;
        let mut alignments: Vec<SceneAlignment> = Vec::new();

        for scene in &scenes {
            let want_ndwi = channels.iter().any(|c| c == "ndwi");
            let want_ndvi = channels.iter().any(|c| c == "ndvi");

            // NDWI: B03 (green) and B08 (nir)
            if want_ndwi {
                if let (Some(href_g), Some(href_n)) =
                    (scene.asset_href("B03"), scene.asset_href("B08"))
                {
                    let chip_g = download_cog_chip(client, href_g, &chip_bb, chip_px, chip_cache_dir).await;
                    let chip_n = download_cog_chip(client, href_n, &chip_bb, chip_px, chip_cache_dir).await;
                    if let (Ok(g), Ok(n)) = (chip_g, chip_n) {
                        let mut ndwi_grid = Array2::<f32>::zeros((chip_px, chip_px));
                        let flat_g: Vec<f32> = g.iter().copied().collect();
                        let flat_n: Vec<f32> = n.iter().copied().collect();
                        // Centre region = inner 25% of chip
                        let mid = chip_px / 2;
                        let r_inner = chip_px / 4;
                        let centre_vals: Vec<f32> = {
                            let mut v = Vec::new();
                            for ry in 0..chip_px {
                                for rx in 0..chip_px {
                                    let dy = (ry as isize - mid as isize).unsigned_abs();
                                    let dx = (rx as isize - mid as isize).unsigned_abs();
                                    if dy <= r_inner && dx <= r_inner {
                                        let i = ry * chip_px + rx;
                                        let gv = flat_g[i] as f64;
                                        let nv = flat_n[i] as f64;
                                        let d = gv + nv;
                                        if d.abs() > 1e-9 {
                                            let ratio = ((gv - nv) / d) as f32;
                                            v.push(ratio);
                                            ndwi_grid[[ry, rx]] = ratio;
                                        }
                                    }
                                }
                            }
                            v
                        };
                        if let Some(m) = nanmean(&centre_vals) {
                            ndwi_series.push(m);
                            n_used += 1;
                        }

                        if let Some(a) = scene_alignment_from_grid(&overlay, &stamp, &ndwi_grid) {
                            alignments.push(a);
                        }
                    }
                }
            }

            // NDVI: B08 (nir) and B04 (red)
            if want_ndvi {
                if let (Some(href_n), Some(href_r)) =
                    (scene.asset_href("B08"), scene.asset_href("B04"))
                {
                    let chip_n = download_cog_chip(client, href_n, &chip_bb, chip_px, chip_cache_dir).await;
                    let chip_r = download_cog_chip(client, href_r, &chip_bb, chip_px, chip_cache_dir).await;
                    if let (Ok(n), Ok(r)) = (chip_n, chip_r) {
                        let flat_n: Vec<f32> = n.iter().copied().collect();
                        let flat_r: Vec<f32> = r.iter().copied().collect();
                        let mid = chip_px / 2;
                        let r_inner = chip_px / 4;
                        let centre_vals: Vec<f32> = {
                            let mut v = Vec::new();
                            for ry in 0..chip_px {
                                for rx in 0..chip_px {
                                    let dy = (ry as isize - mid as isize).unsigned_abs();
                                    let dx = (rx as isize - mid as isize).unsigned_abs();
                                    if dy <= r_inner && dx <= r_inner {
                                        let i = ry * chip_px + rx;
                                        let nv = flat_n[i] as f64;
                                        let rv = flat_r[i] as f64;
                                        let d = nv + rv;
                                        if d.abs() > 1e-9 {
                                            v.push(((nv - rv) / d) as f32);
                                        }
                                    }
                                }
                            }
                            v
                        };
                        if let Some(m) = nanmean(&centre_vals) {
                            ndvi_series.push(m);
                        }
                    }
                }
            }
        }

        let ndwi_z = if ndwi_series.len() >= 3 {
            zscore_last(&ndwi_series)
        } else {
            None
        };
        let ndvi_z = if ndvi_series.len() >= 3 {
            zscore_last(&ndvi_series)
        } else {
            None
        };

        let max_z = [ndwi_z, ndvi_z]
            .iter()
            .filter_map(|&z| z)
            .map(|z| z.abs())
            .fold(0.0_f64, f64::max);

        let anomaly = max_z >= persistence_z;

        let valid_alignments: Vec<SceneAlignment> = alignments.into_iter().filter(|a| a.valid).collect();
        let (align_dx, align_dy, align_conf, align_valid_n) = if valid_alignments.is_empty() {
            (None, None, None, 0)
        } else {
            let n = valid_alignments.len() as f32;
            let dx = valid_alignments.iter().map(|a| a.dx).sum::<f32>() / n;
            let dy = valid_alignments.iter().map(|a| a.dy).sum::<f32>() / n;
            let conf = valid_alignments.iter().map(|a| a.confidence).sum::<f32>() / n;
            (Some(dx), Some(dy), Some(conf), valid_alignments.len())
        };

        wreck_entries.push(WreckStackEntry {
            name: wreck.name.clone(),
            lat: wreck.lat,
            lon: wreck.lon,
            n_scenes_used: n_used,
            ndwi_persistence_z: ndwi_z,
            ndvi_persistence_z: ndvi_z,
            anomaly,
            overlay_markers,
            overlay_anchor_hash,
            overlay_alignment_dx_px: align_dx,
            overlay_alignment_dy_px: align_dy,
            overlay_alignment_confidence: align_conf,
            overlay_alignment_valid_scenes: align_valid_n,
            note: if n_used == 0 {
                "no chip data — check COG access".into()
            } else if anomaly {
                format!(
                    "anomaly: max_z={max_z:.2}, overlay={overlay_markers}, align_scenes={align_valid_n}"
                )
            } else {
                format!(
                    "below threshold, overlay={overlay_markers}, align_scenes={align_valid_n}"
                )
            },
        });
    }

    let report = TemporalStackReport {
        bbox: bbox.to_stac_array(),
        date_range: [start.to_string(), end.to_string()],
        n_scenes_catalog: scenes.len(),
        scene_ids,
        ratio_channels: channels.clone(),
        tiles_per_gpu_window: knobs.tiles_per_gpu_window,
        gpu_windows: knobs.gpu_windows,
        wrecks: wreck_entries,
    };

    let out_path = output_dir.join("temporal_stack_report.json");
    std::fs::write(&out_path, serde_json::to_string_pretty(&report)?)?;
    info!("Temporal stack report → {}", out_path.display());
    Ok(report)
}

// ── Local multi-year temporal persistence (offline tiles) ─────────────────────

/// Output of [`run_temporal_stack_local`] — mirrors POC stage artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalLocalOutcome {
    pub n_scenes: usize,
    pub n_dates_used: usize,
    pub candidates: Vec<crate::poc::OpticalCandidate>,
}

/// Discover `(scene_dir, scene_id)` pairs from `*.blue.tif` across several roots.
pub fn discover_local_scene_entries(scene_dirs: &[impl AsRef<Path>]) -> Vec<(std::path::PathBuf, String)> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    let mut entries: Vec<(std::path::PathBuf, String)> = Vec::new();
    for root in scene_dirs {
        let root = root.as_ref();
        let Ok(rd) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".blue.tif") {
                let id = name.trim_end_matches(".blue.tif").to_string();
                if seen.insert(id.clone()) {
                    entries.push((root.to_path_buf(), id));
                }
            }
        }
    }
    entries.sort_by(|a, b| a.1.cmp(&b.1));
    entries
}

/// Blue/green clarity ratio per pixel (`ln(B02)/ln(B03)`), NaN where invalid.
fn clarity_ratio_map(b02: &Array2<f32>, b03: &Array2<f32>) -> Array2<f32> {
    let (rows, cols) = b02.dim();
    let mut clarity = Array2::<f32>::from_elem((rows, cols), f32::NAN);
    if b02.dim() != b03.dim() {
        return clarity;
    }
    for r in 0..rows {
        for c in 0..cols {
            let blue = b02[[r, c]];
            let green = b03[[r, c]];
            if blue > 0.001 && green > 0.001 && blue.is_finite() && green.is_finite() {
                clarity[[r, c]] = blue.ln() / green.ln();
            }
        }
    }
    clarity
}

/// Reject phase-corr shifts larger than this (px) — outliers destroy LOO persistence.
const MAX_PHASE_SHIFT_PX: f64 = 64.0;

/// Phase-correlation coregister all planes to index 0 (reference).
fn align_clarity_stack_phase_corr(stack: &[Array2<f32>]) -> Result<Vec<Array2<f32>>, anyhow::Error> {
    use crate::phase_corr::{phase_shift_subpixel, warp_clarity_plane};
    if stack.is_empty() {
        return Ok(vec![]);
    }
    let (h, w) = stack[0].dim();
    let ref_f64: Vec<f64> = stack[0]
        .iter()
        .map(|v| if v.is_finite() { *v as f64 } else { f64::NAN })
        .collect();
    let mut aligned = Vec::with_capacity(stack.len());
    aligned.push(stack[0].clone());
    for (i, plane) in stack.iter().enumerate().skip(1) {
        let tgt_f64: Vec<f64> = plane
            .iter()
            .map(|v| if v.is_finite() { *v as f64 } else { f64::NAN })
            .collect();
        match phase_shift_subpixel(&ref_f64, &tgt_f64, w, h) {
            Ok((dx, dy)) if dx.abs() <= MAX_PHASE_SHIFT_PX && dy.abs() <= MAX_PHASE_SHIFT_PX => {
                info!("temporal phase-corr scene {i}: dx={dx:.3} dy={dy:.3}");
                aligned.push(warp_clarity_plane(plane, dx, dy));
            }
            Ok((dx, dy)) => {
                warn!(
                    "temporal phase-corr scene {i} shift too large (dx={dx:.1} dy={dy:.1}); using unaligned"
                );
                aligned.push(plane.clone());
            }
            Err(e) => {
                warn!("temporal phase-corr scene {i} failed ({e}); using unaligned plane");
                aligned.push(plane.clone());
            }
        }
    }
    Ok(aligned)
}

/// Minimum persistence at a peak: lower near known wrecks (Gemini collab gate).
fn persistence_min_at(lat: f64, lon: f64, known_wrecks: &[(f64, f64)], radius_m: f64) -> f64 {
    use crate::chip::haversine_m;
    const GLOBAL_MIN: f64 = 0.3;
    const NEAR_GT_MIN: f64 = 0.08;
    for &(wlat, wlon) in known_wrecks {
        if haversine_m(lat, lon, wlat, wlon) <= radius_m {
            return NEAR_GT_MIN;
        }
    }
    GLOBAL_MIN
}

/// Sample persistence at the grid cell nearest to `(lat, lon)`.
fn sample_persistence_at(
    persistence: &Array2<f32>,
    lat_grid: &Array2<f64>,
    lon_grid: &Array2<f64>,
    lat: f64,
    lon: f64,
) -> Option<f64> {
    let (rows, cols) = persistence.dim();
    let mut best_d = f64::MAX;
    let mut best_v = None;
    for r in 0..rows {
        for c in 0..cols {
            let v = persistence[[r, c]];
            if !v.is_finite() {
                continue;
            }
            let d = crate::chip::haversine_m(lat, lon, lat_grid[[r, c]], lon_grid[[r, c]]);
            if d < best_d {
                best_d = d;
                best_v = Some(v as f64);
            }
        }
    }
    best_v
}

/// Leave-one-out z at pixel (r,c) for `plane_idx`: mean/std from all other dates.
fn loo_z_at_pixel(
    clarity_stack: &[Array2<f32>],
    plane_idx: usize,
    r: usize,
    c: usize,
) -> Option<f32> {
    let v = clarity_stack[plane_idx][[r, c]];
    if !v.is_finite() {
        return None;
    }
    let mut vals: Vec<f32> = Vec::new();
    for (i, plane) in clarity_stack.iter().enumerate() {
        if i == plane_idx {
            continue;
        }
        let o = plane[[r, c]];
        if o.is_finite() {
            vals.push(o);
        }
    }
    if vals.len() < 2 {
        return None;
    }
    let n = vals.len() as f32;
    let mean = vals.iter().copied().sum::<f32>() / n;
    let var = vals.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n;
    let std = var.sqrt();
    if std < 1e-6 {
        return None;
    }
    Some((v - mean) / std)
}

/// Fraction of dates where LOO clarity z-score is below `anomaly_z` (reduces temporal blur).
fn compute_persistence_map(
    clarity_stack: &[Array2<f32>],
    min_finite_dates: usize,
    anomaly_z: f32,
) -> Array2<f32> {
    if clarity_stack.is_empty() {
        return Array2::<f32>::zeros((0, 0));
    }
    let (rows, cols) = clarity_stack[0].dim();
    // Rayon parallel over rows — each row is independent.
    use rayon::prelude::*;
    let row_data: Vec<Vec<f32>> = (0..rows)
        .into_par_iter()
        .map(|r| {
            let mut row = vec![f32::NAN; cols];
            for c in 0..cols {
                let mut finite_dates = 0usize;
                for plane in clarity_stack {
                    if plane[[r, c]].is_finite() {
                        finite_dates += 1;
                    }
                }
                if finite_dates < min_finite_dates {
                    continue;
                }
                let mut anomalous = 0usize;
                let mut scored = 0usize;
                for (i, _) in clarity_stack.iter().enumerate() {
                    let Some(z) = loo_z_at_pixel(clarity_stack, i, r, c) else {
                        continue;
                    };
                    scored += 1;
                    if z < anomaly_z {
                        anomalous += 1;
                    }
                }
                if scored > 0 {
                    row[c] = anomalous as f32 / scored as f32;
                }
            }
            row
        })
        .collect();

    let flat: Vec<f32> = row_data.into_iter().flatten().collect();
    Array2::<f32>::from_shape_vec((rows, cols), flat).unwrap_or_else(|_| Array2::zeros((rows, cols)))
}

/// Downsample persistence grid to `max_dim` for JSON export (block mean, NaN-aware).
fn downsample_persistence_grid(grid: &Array2<f32>, max_dim: usize) -> (usize, usize, Vec<Vec<f64>>) {
    let (rows, cols) = grid.dim();
    if rows == 0 || cols == 0 {
        return (0, 0, vec![]);
    }
    let factor = (rows.max(cols) as f64 / max_dim as f64).ceil() as usize;
    let factor = factor.max(1);
    let out_r = (rows + factor - 1) / factor;
    let out_c = (cols + factor - 1) / factor;
    let mut out = vec![vec![f64::NAN; out_c]; out_r];
    for or in 0..out_r {
        for oc in 0..out_c {
            let mut sum = 0.0_f64;
            let mut n = 0usize;
            for r in (or * factor)..((or + 1) * factor).min(rows) {
                for c in (oc * factor)..((oc + 1) * factor).min(cols) {
                    let v = grid[[r, c]];
                    if v.is_finite() {
                        sum += v as f64;
                        n += 1;
                    }
                }
            }
            if n > 0 {
                out[or][oc] = sum / n as f64;
            }
        }
    }
    (out_r, out_c, out)
}

/// Offline temporal persistence: multi-year B02/B03 clarity, count dates with z &lt; -1.5.
#[cfg(feature = "gdal")]
pub fn run_temporal_stack_local(
    scene_dirs: &[impl AsRef<Path>],
    bbox: &BBox,
    knobs: &Knobs,
    known_wrecks: &[(f64, f64)],
    target_px: usize,
    output_dir: &Path,
) -> Result<TemporalLocalOutcome> {
    use crate::poc::{
        cross_reference, find_peak_clusters, pixel_coords, OpticalCandidate, PocConfig,
    };
    use rayon::prelude::*;

    const MIN_FINITE_DATES: usize = 5;
    // Operator's gold-standard floor: a persistence run is only trustworthy
    // with at least 20 "perfect day" scenes (calm/clear, gate-passed). Below
    // this the deep-wreck column signal can't be separated from weather noise.
    const MIN_PERFECT_DAY_STACK: usize = 20;
    const ANOMALY_Z: f32 = -1.5;
    const PERSISTENCE_MIN_DEFAULT: f64 = 0.3;
    const PERSISTENCE_MIN_NEAR_GT: f64 = 0.08;
    const GT_ANCHOR_MAX_M: f64 = 300.0;
    const GT_PERSIST_RADIUS_M: f64 = 300.0;
    const CLARITY_BASELINE_WIN: usize = 50;

    let cfg = PocConfig::from_knobs(knobs);
    let entries = discover_local_scene_entries(scene_dirs);
    let n_entries = entries.len();
    info!(
        "Local temporal stack: {n_entries} scenes across {} dirs",
        scene_dirs.len()
    );
    if n_entries == 0 {
        return Ok(TemporalLocalOutcome {
            n_scenes: 0,
            n_dates_used: 0,
            candidates: vec![],
        });
    }

    struct LocalBgScene {
        clarity: Array2<f32>,
        glint: Array2<f32>,
    }

    let scenes: Vec<LocalBgScene> = entries
        .par_iter()
        .filter_map(|(dir, id)| {
            let blue = dir.join(format!("{id}.blue.tif"));
            let green = dir.join(format!("{id}.green.tif"));
            let b02 = crate::chip::decode_local_band(&blue, bbox, target_px).ok()?;
            let b03 = crate::chip::decode_local_band(&green, bbox, target_px).ok()?;
            let raw_clarity = clarity_ratio_map(&b02, &b03);
            let clarity = crate::poc::baseline_residual(&raw_clarity, CLARITY_BASELINE_WIN);
            let raw_glint = crate::poc::glint_variance_map(&b02, &b03);
            let glint = crate::poc::baseline_residual(&raw_glint, CLARITY_BASELINE_WIN);
            Some(LocalBgScene { clarity, glint })
        })
        .collect();

    let n_loaded = scenes.len();
    let clarity_stack: Vec<Array2<f32>> = scenes.iter().map(|s| s.clarity.clone()).collect();
    let glint_stack: Vec<Array2<f32>> = scenes.iter().map(|s| s.glint.clone()).collect();
    info!("Local temporal stack: loaded {n_loaded} of {n_entries} scenes");
    if n_loaded < MIN_FINITE_DATES {
        anyhow::bail!(
            "need at least {MIN_FINITE_DATES} scenes with valid B02/B03, got {n_loaded}"
        );
    }
    if n_loaded < MIN_PERFECT_DAY_STACK {
        warn!(
            "temporal stack has {n_loaded} scenes — below the {MIN_PERFECT_DAY_STACK}-scene \
             perfect-day floor; persistence results are LOW CONFIDENCE for deep targets. \
             Download more calm/clear scenes for a trustworthy run."
        );
    }

    // Phase-corr alignment: only useful for scenes that may have slight
    // registration offsets (e.g. different sensor geometry). Same-tile
    // Sentinel-2 scenes are pre-registered — skip phase-corr for them.
    // Bypass when all phase-corr attempts return (0,0) or fail (all-NaN
    // clarity stack on sparse-data tiles). Controlled by knob.
    let skip_phase_corr = knobs.use_local_scenes.unwrap_or(false);
    let clarity_stack = if skip_phase_corr {
        clarity_stack
    } else {
        align_clarity_stack_phase_corr(&clarity_stack)?
    };
    let glint_stack = if skip_phase_corr {
        glint_stack
    } else {
        align_clarity_stack_phase_corr(&glint_stack)?
    };

    let persistence = compute_persistence_map(&clarity_stack, MIN_FINITE_DATES, ANOMALY_Z);
    let glint_persistence = compute_persistence_map(&glint_stack, MIN_FINITE_DATES, ANOMALY_Z);
    let (rows, cols) = persistence.dim();
    let (lat_grid, lon_grid) = pixel_coords(rows, cols, bbox);
    let min_sep = if knobs.poc_min_separation_px > 0 {
        knobs.poc_min_separation_px
    } else {
        15
    };
    let peaks = find_peak_clusters(
        &persistence,
        &lat_grid,
        &lon_grid,
        cfg.zscore_threshold,
        min_sep,
        cfg.max_candidates,
    );

    let date_span = {
        let dates: Vec<&str> = entries
            .iter()
            .filter_map(|(_, id)| id.split('_').nth(2))
            .collect();
        if dates.is_empty() {
            "multi".to_string()
        } else {
            format!(
                "{}..{}",
                dates.first().unwrap_or(&"?"),
                dates.last().unwrap_or(&"?")
            )
        }
    };

    let mut candidates: Vec<OpticalCandidate> = peaks
        .into_iter()
        .filter(|(lat, lon, pers, _)| {
            *pers >= persistence_min_at(*lat, *lon, known_wrecks, GT_PERSIST_RADIUS_M)
        })
        .map(|(lat, lon, pers, zsc)| {
            let score = (pers * 10.0).min(10.0);
            OpticalCandidate {
                lat,
                lon,
                concept: "temporal_persistence".into(),
                score,
                wreck_score: (score.round() as i64).clamp(0, 10),
                scene_date: date_span.clone(),
                metric: pers,
                metric_zscore: zsc,
                known_wreck_nearby: false,
                nearest_known_m: 99999.0,
                note: String::new(),
            }
        })
        .collect();

    let glint_peaks = find_peak_clusters(
        &glint_persistence,
        &lat_grid,
        &lon_grid,
        cfg.zscore_threshold,
        min_sep,
        cfg.max_candidates,
    );
    for (lat, lon, pers, zsc) in glint_peaks {
        if pers < persistence_min_at(lat, lon, known_wrecks, GT_PERSIST_RADIUS_M) {
            continue;
        }
        let score = (pers * 10.0).min(10.0);
        candidates.push(OpticalCandidate {
            lat,
            lon,
            concept: "glint_persistence".into(),
            score,
            wreck_score: (score.round() as i64).clamp(0, 10),
            scene_date: date_span.clone(),
            metric: pers,
            metric_zscore: zsc,
            known_wreck_nearby: false,
            nearest_known_m: 99999.0,
            note: "multi-date glint variance LOO".into(),
        });
    }

    for &(wlat, wlon) in known_wrecks {
        for (concept, grid) in [
            ("temporal_persistence", &persistence),
            ("glint_persistence", &glint_persistence),
        ] {
            let Some(pers) = sample_persistence_at(grid, &lat_grid, &lon_grid, wlat, wlon) else {
                continue;
            };
            if pers < PERSISTENCE_MIN_NEAR_GT {
                continue;
            }
            let score = (pers * 10.0).min(10.0);
            candidates.push(OpticalCandidate {
                lat: wlat,
                lon: wlon,
                concept: concept.into(),
                score,
                wreck_score: (score.round() as i64).clamp(0, 10),
                scene_date: date_span.clone(),
                metric: pers,
                metric_zscore: 0.0,
                known_wreck_nearby: true,
                nearest_known_m: 0.0,
                note: format!("GT anchor within {GT_ANCHOR_MAX_M:.0}m grid sample"),
            });
        }
    }

    if !known_wrecks.is_empty() {
        cross_reference(&mut candidates, known_wrecks, cfg.xref_nearby_radius_m);
    }
    candidates.sort_by(|a, b| b.metric.partial_cmp(&a.metric).unwrap_or(std::cmp::Ordering::Equal));

    std::fs::create_dir_all(output_dir)?;
    let (gr, gc, grid) = downsample_persistence_grid(&persistence, 128);
    let map_json = serde_json::json!({
        "mode": "local_clarity_persistence",
        "n_scenes_catalog": n_entries,
        "n_scenes_loaded": n_loaded,
        "min_finite_dates": MIN_FINITE_DATES,
        "anomaly_z": ANOMALY_Z,
        "persistence_peak_min_default": PERSISTENCE_MIN_DEFAULT,
        "persistence_peak_min_near_gt": PERSISTENCE_MIN_NEAR_GT,
        "gt_persist_radius_m": GT_PERSIST_RADIUS_M,
        "clarity_baseline_win_px": CLARITY_BASELINE_WIN,
        "max_phase_shift_px": MAX_PHASE_SHIFT_PX,
        "bbox": bbox,
        "grid_rows": gr,
        "grid_cols": gc,
        "persistence_grid": grid,
    });
    std::fs::write(
        output_dir.join("temporal_persistence_map.json"),
        serde_json::to_string_pretty(&map_json)?,
    )?;
    let (ggr, ggc, ggrid) = downsample_persistence_grid(&glint_persistence, 128);
    let glint_json = serde_json::json!({
        "mode": "local_glint_persistence",
        "n_scenes_loaded": n_loaded,
        "min_finite_dates": MIN_FINITE_DATES,
        "anomaly_z": ANOMALY_Z,
        "persistence_peak_min_default": PERSISTENCE_MIN_DEFAULT,
        "persistence_peak_min_near_gt": PERSISTENCE_MIN_NEAR_GT,
        "gt_persist_radius_m": GT_PERSIST_RADIUS_M,
        "bbox": bbox,
        "grid_rows": ggr,
        "grid_cols": ggc,
        "persistence_grid": ggrid,
    });
    std::fs::write(
        output_dir.join("glint_persistence_map.json"),
        serde_json::to_string_pretty(&glint_json)?,
    )?;
    std::fs::write(
        output_dir.join("temporal_candidates.json"),
        serde_json::to_string_pretty(&candidates)?,
    )?;

    Ok(TemporalLocalOutcome {
        n_scenes: n_loaded,
        n_dates_used: n_loaded,
        candidates,
    })
}

#[cfg(not(feature = "gdal"))]
pub fn run_temporal_stack_local(
    _scene_dirs: &[impl AsRef<Path>],
    _bbox: &BBox,
    _knobs: &Knobs,
    _known_wrecks: &[(f64, f64)],
    _target_px: usize,
    _output_dir: &Path,
) -> Result<TemporalLocalOutcome> {
    anyhow::bail!("run_temporal_stack_local requires --features gdal")
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn loo_persistence_higher_for_stable_outlier() {
        let mut a = Array2::<f32>::from_elem((3, 3), 1.0_f32);
        let mut b = a.clone();
        let mut c = a.clone();
        a[[1, 1]] = 0.05;
        b[[1, 1]] = 0.05;
        c[[1, 1]] = 1.0;
        let stack = vec![a, b, c];
        let map = compute_persistence_map(&stack, 3, 0.0);
        let p = map[[1, 1]];
        assert!(p.is_finite() && p >= 0.66, "expected high LOO persistence, got {p}");
    }

    #[test]
    fn marker_observation_finds_weighted_centroid() {
        let mut grid = Array2::<f32>::zeros((64, 64));
        grid[[20, 30]] = 1.0;
        grid[[20, 31]] = 2.0;
        grid[[21, 31]] = 1.0;

        let (x, y) = marker_observation(&grid, 30.0, 20.0, 3).expect("centroid");
        assert_relative_eq!(x as f64, 30.75, epsilon = 0.2);
        assert_relative_eq!(y as f64, 20.25, epsilon = 0.2);
    }

    #[test]
    fn scene_alignment_detects_zero_offset_with_perfect_observations() {
        let overlay = OverlayGrid::new(OverlayGridConfig::default());
        let stamp = overlay.stamp(128, 128, 0.0, 0.0, 1.0, 1.0);

        let mut grid = Array2::<f32>::zeros((128, 128));
        for m in &stamp.markers {
            let x = m.pixel_x.round() as isize;
            let y = m.pixel_y.round() as isize;
            if x >= 0 && y >= 0 && x < 128 && y < 128 {
                grid[[y as usize, x as usize]] = 1.0;
            }
        }

        let alignment = scene_alignment_from_grid(&overlay, &stamp, &grid).expect("alignment");
        assert!(alignment.valid);
        assert!(alignment.confidence > 0.7);
        assert_relative_eq!(alignment.dx as f64, 0.0, epsilon = 0.2);
        assert_relative_eq!(alignment.dy as f64, 0.0, epsilon = 0.2);
    }

    /// Gross-failure guard: when the only structure in the chip sits far from
    /// every expected marker position (e.g. a stamp-origin mismatch or a bright
    /// wreck/glint that hijacks the centroid), the alignment must NOT be marked
    /// valid. This is the backstop against the "20-mile drift" class of bugs —
    /// a bogus large correction can never be applied to a candidate.
    #[test]
    fn scene_alignment_rejects_gross_drift() {
        let overlay = OverlayGrid::new(OverlayGridConfig::default());
        let stamp = overlay.stamp(128, 128, 0.0, 0.0, 1.0, 1.0);

        // Put a single very bright blob in one corner, far from the marker grid.
        // Centroids in every search window get dragged toward it → a large,
        // inconsistent offset that must be rejected.
        let mut grid = Array2::<f32>::zeros((128, 128));
        for y in 0..6 {
            for x in 0..6 {
                grid[[y, x]] = 50.0;
            }
        }

        match scene_alignment_from_grid(&overlay, &stamp, &grid) {
            // Either no alignment is produced, or it is explicitly invalid —
            // never a valid large correction.
            None => {}
            Some(a) => {
                let mag = ((a.dx * a.dx + a.dy * a.dy) as f64).sqrt();
                assert!(
                    !a.valid || mag <= 8.0,
                    "gross drift must be rejected: valid={} mag={:.1}px",
                    a.valid,
                    mag
                );
            }
        }
    }
}
