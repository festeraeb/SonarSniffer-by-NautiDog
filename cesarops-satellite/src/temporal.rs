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

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

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
