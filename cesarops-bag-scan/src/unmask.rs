//! Uncertainty-guided unmask reconstruction + visual export.
//!
//! Rebuilds the elevation surface hidden inside a [`MaskedRegion`] and writes
//! georeferenced rasters a high-end viewer (QGIS / Fledermaus / GlobalMapper)
//! can open. This is the Rust rebuild of the lost "fast reconstruction" engine
//! that fed `bag_visualization_generator.py` (recovered) — that tool read a
//! `fast_reconstructed/` dir and diffed it against the original; the engine
//! that produced those rasters was corrupted in the laptop-disk crash.
//!
//! # Why uncertainty drives it
//!
//! The BAG spec makes the uncertainty band MANDATORY — a BAG with no
//! uncertainty layer is malformed. So even when a redactor flattens the
//! ELEVATION inside a region, the uncertainty band typically still carries the
//! footprint of where real soundings were collected (their per-node vertical
//! uncertainty). That residual structure is the fingerprint we reconstruct
//! against: where uncertainty shows genuine soundings existed under the mask,
//! we restore RELIEF there rather than smoothing the area flat.
//!
//! # Improvements over the original `restore_preview` logic
//!
//! 1. IDW (inverse-distance weighted, smooth) baseline fill instead of pure
//!    nearest-neighbour (which produced blocky Voronoi facets).
//! 2. Uncertainty-guided relief: modulate the fill with the (demeaned,
//!    normalised) interior uncertainty so a hidden hull re-emerges as a bump
//!    instead of being averaged away.
//! 3. Hillshade export (shaded relief) — a raking-light render where a hidden
//!    wreck pops far better than a flat depth colormap.

use crate::geo::GeoTransformer;
use crate::types::{BagInfo, Knobs, MaskedRegion, M_TO_FT};
use gdal::Dataset;
use ndarray::Array2;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Output of one region reconstruction.
pub struct Reconstruction {
    pub region_id: String,
    /// Local window (full reconstructed elevation, meters), row-major.
    pub recon: Array2<f64>,
    /// Original elevation in the same window (NaN where masked/nodata).
    pub original: Array2<f64>,
    /// Difference (recon - original) only where original was masked.
    pub difference: Array2<f64>,
    /// Hillshade of the reconstruction, 0..=255.
    pub hillshade: Array2<u8>,
    /// Window origin in the full grid (row0, col0).
    pub origin: (usize, usize),
    /// Reconstruction stats for the report.
    pub filled_cells: usize,
    pub relief_applied: bool,
    pub max_relief_ft: f64,
}

/// Reconstruct every masked region's hidden surface and (optionally) export
/// georeferenced rasters. Returns one [`Reconstruction`] per region.
///
/// `elevation` / `uncertainty` are the full grids from stage A. `out_dir`, when
/// `Some`, receives `<region_id>_recon.tif`, `_diff.tif`, `_hillshade.tif`.
pub fn unmask_regions(
    path: &str,
    elevation: &Array2<f64>,
    uncertainty: Option<&Array2<f64>>,
    regions: &[MaskedRegion],
    info: &BagInfo,
    _geo: &GeoTransformer,
    knobs: &Knobs,
    out_dir: Option<&Path>,
) -> Vec<Reconstruction> {
    let (rows, cols) = elevation.dim();
    // Pull the dataset geotransform + projection once for georeferenced export.
    let (full_gt, proj) = match Dataset::open(path) {
        Ok(ds) => (
            ds.geo_transform().ok(),
            ds.projection(),
        ),
        Err(_) => (None, String::new()),
    };

    let mut out = Vec::new();
    for reg in regions {
        // Skip regions too large to be a deliberate hide (coverage-edge /
        // whole-tile artifacts). A real redaction is structure-sized, not a
        // survey-footprint-sized blob. Cap by long-side feet.
        // Big areas that pass the probe still get flagged (see below).
        let oversized = reg.long_side_ft > knobs.unmask_max_region_ft;

        // ── PROBE: does the uncertainty interior show evidence of a hidden object?
        // Use the tpu_boundary_score and band2_ghost_score from fusion_rescore
        // (already computed before this is called). A high score means the
        // uncertainty inside the mask is anomalous vs the ring — something's there.
        let probe_score = (reg.tpu_boundary_score + reg.band2_ghost_score) / 2.0;
        let probe_passes = probe_score >= knobs.unmask_probe_threshold
            || reg.depth_anomaly_ft.abs() > knobs.unmask_anomaly_threshold_ft;

        // Decide: auto-rebuild, flag-only, or skip.
        if !probe_passes && !oversized {
            // Small + no object evidence → likely a seam or noise. Skip.
            continue;
        }
        if oversized && !probe_passes {
            // Big + inconclusive → flag for optional rebuild, don't auto-export.
            // (These show up in the report as unconfirmed candidates; user can
            // re-run with --knobs '{"unmask_force_all":true}' to rebuild them.)
            if !knobs.unmask_force_all {
                continue;
            }
        }

        // ── REBUILD: reconstruct this region's hidden surface.
        // Reconstruct in a window around the region bbox + margin.
        let margin = knobs.unmask_margin_px;
        let r0 = reg.bbox_row_min.saturating_sub(margin);
        let r1 = (reg.bbox_row_max + margin).min(rows - 1);
        let c0 = reg.bbox_col_min.saturating_sub(margin);
        let c1 = (reg.bbox_col_max + margin).min(cols - 1);
        let wh = r1 - r0 + 1;
        let ww = c1 - c0 + 1;

        // Masked pixel set (those we must fill).
        let masked: HashSet<(usize, usize)> = mask_pixels(elevation, reg, r0, r1, c0, c1);
        if masked.is_empty() {
            continue;
        }

        // Local copies.
        let mut original = Array2::<f64>::from_elem((wh, ww), f64::NAN);
        for r in r0..=r1 {
            for c in c0..=c1 {
                original[[r - r0, c - c0]] = elevation[[r, c]];
            }
        }

        // Donor points = valid, non-masked cells in the window.
        let mut donors: Vec<(f64, f64, f64)> = Vec::new(); // (row, col, depth)
        for r in r0..=r1 {
            for c in c0..=c1 {
                let v = elevation[[r, c]];
                if v.is_finite() && !masked.contains(&(r, c)) {
                    donors.push((r as f64, c as f64, v));
                }
            }
        }
        if donors.len() < 8 {
            continue;
        }

        // IDW baseline fill of the masked interior.
        let mut recon = original.clone();
        let donor_sample = subsample(&donors, knobs.unmask_max_donors);
        for &(mr, mc) in &masked {
            let v = idw(mr as f64, mc as f64, &donor_sample, knobs.unmask_idw_power);
            recon[[mr - r0, mc - c0]] = v;
        }
        // Fill any remaining (non-masked) nodata so the window is continuous
        // for hillshade, using the same IDW.
        for r in 0..wh {
            for c in 0..ww {
                if !recon[[r, c]].is_finite() {
                    let gr = (r + r0) as f64;
                    let gc = (c + c0) as f64;
                    recon[[r, c]] = idw(gr, gc, &donor_sample, knobs.unmask_idw_power);
                }
            }
        }

        // Uncertainty-guided relief: where interior uncertainty deviates from
        // the donor-ring mean, add proportional relief so a hidden feature
        // re-emerges rather than being smoothed flat.
        let mut relief_applied = false;
        let mut max_relief_ft = 0.0;
        if knobs.unmask_uncertainty_relief {
            if let Some(unc) = uncertainty {
                let (applied, mx) = apply_uncertainty_relief(
                    &mut recon, unc, &masked, &donors, r0, c0, knobs,
                );
                relief_applied = applied;
                max_relief_ft = mx * M_TO_FT;
            }
        }

        // Difference map (recon - original) only where masked.
        let mut difference = Array2::<f64>::from_elem((wh, ww), f64::NAN);
        for &(mr, mc) in &masked {
            let lr = mr - r0;
            let lc = mc - c0;
            difference[[lr, lc]] = recon[[lr, lc]] - reg.surrounding_depth_ft / M_TO_FT;
        }

        // Hillshade of the reconstruction.
        let cell_m = info.resolution_m.max(0.1);
        let hillshade = hillshade(&recon, cell_m, knobs.unmask_sun_az_deg, knobs.unmask_sun_alt_deg);

        // Export georeferenced rasters for this window.
        if let Some(dir) = out_dir {
            let _ = std::fs::create_dir_all(dir);
            let win_gt = window_geotransform(full_gt, r0, c0, info.read_step);
            let base = sanitize(&reg.id);
            let _ = write_geotiff_f64(&dir.join(format!("{base}_recon.tif")), &recon, &win_gt, &proj, info.nodata_value);
            let _ = write_geotiff_f64(&dir.join(format!("{base}_diff.tif")), &difference, &win_gt, &proj, info.nodata_value);
            let _ = write_geotiff_u8(&dir.join(format!("{base}_hillshade.tif")), &hillshade, &win_gt, &proj);
        }

        out.push(Reconstruction {
            region_id: reg.id.clone(),
            recon,
            original,
            difference,
            hillshade,
            origin: (r0, c0),
            filled_cells: masked.len(),
            relief_applied,
            max_relief_ft,
        });
    }
    out
}

/// Collect the masked pixels of a region within the window. A cell is "masked"
/// if it is NaN/nodata (the redacted hole) OR falls inside the region bbox and
/// is part of the flattened/low-texture footprint. We approximate with: NaN
/// cells in-window, plus in-bbox cells (the detector already bounded the bbox).
fn mask_pixels(
    elevation: &Array2<f64>,
    reg: &MaskedRegion,
    r0: usize,
    r1: usize,
    c0: usize,
    c1: usize,
) -> HashSet<(usize, usize)> {
    let mut set = HashSet::new();
    for r in r0..=r1 {
        for c in c0..=c1 {
            let in_bbox = r >= reg.bbox_row_min
                && r <= reg.bbox_row_max
                && c >= reg.bbox_col_min
                && c <= reg.bbox_col_max;
            let v = elevation[[r, c]];
            // nan_hole: NaN cells are the redacted void.
            // flattened/texture: bbox interior is the suspect footprint.
            if !v.is_finite() {
                set.insert((r, c));
            } else if in_bbox && reg.mask_type != "nan_hole" {
                set.insert((r, c));
            }
        }
    }
    set
}

/// Inverse-distance-weighted interpolation at (r,c) from donor (row,col,val).
fn idw(r: f64, c: f64, donors: &[(f64, f64, f64)], power: f64) -> f64 {
    let mut num = 0.0;
    let mut den = 0.0;
    for &(dr, dc, dv) in donors {
        let d2 = (dr - r) * (dr - r) + (dc - c) * (dc - c);
        if d2 < 1e-9 {
            return dv; // exactly on a donor
        }
        let w = 1.0 / d2.powf(power / 2.0);
        num += w * dv;
        den += w;
    }
    if den > 0.0 {
        num / den
    } else {
        f64::NAN
    }
}

/// Add uncertainty-guided relief to the IDW baseline. Returns (applied, max_abs_relief_m).
///
/// Idea: inside the mask, the uncertainty band's local deviation from the donor
/// ring mean indicates where real soundings hit something (lower uncertainty =
/// dense/closer returns = likely a feature; or anomalously high = manipulated).
/// We map the demeaned, scaled interior uncertainty to a relief offset on the
/// smooth IDW fill so structure re-emerges. Conservative: clamped to a fraction
/// of local surrounding depth variability.
fn apply_uncertainty_relief(
    recon: &mut Array2<f64>,
    uncertainty: &Array2<f64>,
    masked: &HashSet<(usize, usize)>,
    donors: &[(f64, f64, f64)],
    r0: usize,
    c0: usize,
    knobs: &Knobs,
) -> (bool, f64) {
    // Ring uncertainty stats from donor cells.
    let mut ring_unc: Vec<f64> = Vec::new();
    for &(dr, dc, _) in donors {
        let u = uncertainty[[dr as usize, dc as usize]];
        if u.is_finite() && u > 0.0 {
            ring_unc.push(u);
        }
    }
    if ring_unc.len() < 10 {
        return (false, 0.0);
    }
    let umean = ring_unc.iter().sum::<f64>() / ring_unc.len() as f64;
    let ustd = (ring_unc.iter().map(|u| (u - umean) * (u - umean)).sum::<f64>()
        / ring_unc.len() as f64)
        .sqrt();
    if ustd < 1e-6 {
        return (false, 0.0);
    }

    // Donor depth spread sets the relief budget.
    let depths: Vec<f64> = donors.iter().map(|d| d.2).collect();
    let dmean = depths.iter().sum::<f64>() / depths.len() as f64;
    let dstd = (depths.iter().map(|v| (v - dmean) * (v - dmean)).sum::<f64>()
        / depths.len() as f64)
        .sqrt();
    let budget = dstd * knobs.unmask_relief_gain; // meters of relief at 1 sigma

    let mut max_abs = 0.0;
    let mut any = false;
    for &(mr, mc) in masked {
        let u = uncertainty[[mr, mc]];
        if !(u.is_finite() && u > 0.0) {
            continue;
        }
        // Negative z (lower-than-ring uncertainty) -> a feature shoaling up
        // (shallower => add positive relief). Sign chosen so a real shoal/wreck
        // rises toward the surface.
        let z = (u - umean) / ustd;
        let relief = (-z).clamp(-3.0, 3.0) * (budget / 3.0);
        recon[[mr - r0, mc - c0]] += relief;
        if relief.abs() > max_abs {
            max_abs = relief.abs();
        }
        any = true;
    }
    (any, max_abs)
}

/// Standard hillshade (Horn) from an elevation window. Azimuth/altitude in deg.
fn hillshade(z: &Array2<f64>, cell_m: f64, az_deg: f64, alt_deg: f64) -> Array2<u8> {
    let (h, w) = z.dim();
    let mut out = Array2::<u8>::zeros((h, w));
    let az = (360.0 - az_deg + 90.0).to_radians();
    let zen = (90.0 - alt_deg).to_radians();
    let cos_zen = zen.cos();
    let sin_zen = zen.sin();
    for r in 0..h {
        for c in 0..w {
            // 3x3 neighbourhood with edge clamping.
            let rm = r.saturating_sub(1);
            let rp = (r + 1).min(h - 1);
            let cm = c.saturating_sub(1);
            let cp = (c + 1).min(w - 1);
            let a = z[[rm, cm]];
            let b = z[[rm, c]];
            let cc = z[[rm, cp]];
            let d = z[[r, cm]];
            let f = z[[r, cp]];
            let g = z[[rp, cm]];
            let hh = z[[rp, c]];
            let i = z[[rp, cp]];
            if ![a, b, cc, d, f, g, hh, i].iter().all(|v| v.is_finite()) {
                out[[r, c]] = 0;
                continue;
            }
            let dzdx = ((cc + 2.0 * f + i) - (a + 2.0 * d + g)) / (8.0 * cell_m);
            let dzdy = ((g + 2.0 * hh + i) - (a + 2.0 * b + cc)) / (8.0 * cell_m);
            let slope = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();
            let aspect = dzdy.atan2(-dzdx);
            let hs = cos_zen * slope.cos() + sin_zen * slope.sin() * (az - aspect).cos();
            out[[r, c]] = (hs.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    out
}

fn subsample(v: &[(f64, f64, f64)], cap: usize) -> Vec<(f64, f64, f64)> {
    if v.len() <= cap {
        return v.to_vec();
    }
    let stride = (v.len() / cap).max(1);
    v.iter().step_by(stride).copied().collect()
}

/// Build a geotransform for the window, accounting for read-time decimation.
fn window_geotransform(full_gt: Option<[f64; 6]>, r0: usize, c0: usize, read_step: usize) -> [f64; 6] {
    let step = read_step.max(1) as f64;
    match full_gt {
        Some(gt) => {
            // Full-res pixel of window origin.
            let fc = c0 as f64 * step;
            let fr = r0 as f64 * step;
            let ox = gt[0] + fc * gt[1] + fr * gt[2];
            let oy = gt[3] + fc * gt[4] + fr * gt[5];
            // Pixel size scaled by decimation.
            [ox, gt[1] * step, gt[2] * step, oy, gt[4] * step, gt[5] * step]
        }
        None => [0.0, 1.0, 0.0, 0.0, 0.0, -1.0],
    }
}

fn write_geotiff_f64(
    path: &Path,
    arr: &Array2<f64>,
    gt: &[f64; 6],
    proj: &str,
    nodata: f64,
) -> gdal::errors::Result<()> {
    use gdal::DriverManager;
    let (h, w) = arr.dim();
    let driver = DriverManager::get_driver_by_name("GTiff")?;
    let mut ds = driver.create_with_band_type::<f64, _>(path, w, h, 1)?;
    ds.set_geo_transform(gt)?;
    let _ = ds.set_projection(proj);
    let mut band = ds.rasterband(1)?;
    let _ = band.set_no_data_value(Some(nodata));
    // Flatten row-major, replacing NaN with nodata.
    let mut buf = Vec::with_capacity(h * w);
    for v in arr.iter() {
        buf.push(if v.is_finite() { *v } else { nodata });
    }
    let mut bd = gdal::raster::Buffer::new((w, h), buf);
    band.write((0, 0), (w, h), &mut bd)?;
    Ok(())
}

fn write_geotiff_u8(
    path: &Path,
    arr: &Array2<u8>,
    gt: &[f64; 6],
    proj: &str,
) -> gdal::errors::Result<()> {
    use gdal::DriverManager;
    let (h, w) = arr.dim();
    let driver = DriverManager::get_driver_by_name("GTiff")?;
    let mut ds = driver.create_with_band_type::<u8, _>(path, w, h, 1)?;
    ds.set_geo_transform(gt)?;
    let _ = ds.set_projection(proj);
    let mut band = ds.rasterband(1)?;
    let buf: Vec<u8> = arr.iter().copied().collect();
    let mut bd = gdal::raster::Buffer::new((w, h), buf);
    band.write((0, 0), (w, h), &mut bd)?;
    Ok(())
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

/// Helper for callers wanting just the output dir paths of a region.
pub fn region_output_paths(out_dir: &Path, region_id: &str) -> (PathBuf, PathBuf, PathBuf) {
    let b = sanitize(region_id);
    (
        out_dir.join(format!("{b}_recon.tif")),
        out_dir.join(format!("{b}_diff.tif")),
        out_dir.join(format!("{b}_hillshade.tif")),
    )
}
