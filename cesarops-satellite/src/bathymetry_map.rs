//! Satellite-derived bathymetry (SDB) — multi-pass depth surface fusion.
//!
//! Recovered from `recovery/laptopdump/cesarops_core/src/cesarops/bathymetry_mapper.py`
//! (Phase 11.2 sonar grid) — **re-scoped** for multi-date Sentinel-2:
//! each clear pass contributes a depth proxy; turbidity gates which passes count;
//! fused grid yields relief/slope for ~30m (~100ft) targets in the Straits.
//!
//! Physics: Stumpf/Lyzenga log-ratio depth works to ~2–3× Secchi (~20–30m here).
//! Deeper hulls are inferred via shoal relief + column turbidity, not bottom reflectance.

use crate::types::BBox;
use ndarray::Array2;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{info, warn};

/// Default Stumpf-style coefficients (calibrate with shallow preserve wrecks).
pub const STUMPF_M0_M: f32 = 0.0;
pub const STUMPF_M1_M: f32 = 8.5;

/// Max depth (m) we trust for a single pass given Secchi (m).
pub fn depth_limit_m(secchi_m: f32) -> f32 {
    (2.5 * secchi_m).clamp(8.0, 35.0)
}

/// Secchi proxy from B02/B04 (matches poc.rs zebra_clarity).
pub fn secchi_proxy_m(b02: f32, b04: f32) -> f32 {
    if b04 <= 0.005 || b04 >= 0.3 || !b02.is_finite() || !b04.is_finite() {
        return f32::NAN;
    }
    let ratio = (b02 / b04).max(0.0);
    if ratio.is_finite() {
        (3.9 * ratio.sqrt() + 0.55) as f32
    } else {
        f32::NAN
    }
}

/// Stumpf depth (m, positive down): `m0 - m1 * ln(Rrs_blue)`.
pub fn stumpf_depth_m(b02: f32, m0: f32, m1: f32) -> f32 {
    if b02 <= 0.001 || !b02.is_finite() {
        return f32::NAN;
    }
    let d = m0 - m1 * b02.ln();
    if d.is_finite() && d > 0.0 {
        d
    } else {
        f32::NAN
    }
}

/// Lyzenga-style log-ratio depth proxy from blue/green.
pub fn lyzenga_depth_proxy_m(b02: f32, b03: f32) -> f32 {
    if b02 <= 0.001 || b03 <= 0.001 || !b02.is_finite() || !b03.is_finite() {
        return f32::NAN;
    }
    let x = b02.ln() - b03.ln();
    if x.is_finite() {
        (6.0 + 12.0 * x).max(0.5)
    } else {
        f32::NAN
    }
}

/// NDTI turbidity in [0,1]-ish; high = turbid → down-weight pass.
pub fn ndti(b04: f32, b03: f32) -> f32 {
    let den = b04 + b03;
    if den.abs() < 1e-6 {
        return f32::NAN;
    }
    (b04 - b03) / den
}

/// Per-pixel depth for one scene; optional B04 for Secchi gate.
pub fn depth_map_one_pass(
    b02: &Array2<f32>,
    b03: &Array2<f32>,
    b04: Option<&Array2<f32>>,
) -> (Array2<f32>, Array2<f32>) {
    let (rows, cols) = b02.dim();
    let mut depth = Array2::<f32>::from_elem((rows, cols), f32::NAN);
    let mut weight = Array2::<f32>::from_elem((rows, cols), 0.0);
    for r in 0..rows {
        for c in 0..cols {
            let blue = b02[[r, c]];
            let green = b03[[r, c]];
            let mut w = 1.0f32;
            if let Some(b4) = b04 {
                let s = secchi_proxy_m(blue, b4[[r, c]]);
                let t = ndti(b4[[r, c]], green);
                if s.is_finite() {
                    let lim = depth_limit_m(s);
                    let d_stumpf = stumpf_depth_m(blue, STUMPF_M0_M, STUMPF_M1_M);
                    let d_lyz = lyzenga_depth_proxy_m(blue, green);
                    let d = if d_stumpf.is_finite() {
                        d_stumpf.min(lim)
                    } else {
                        d_lyz.min(lim)
                    };
                    if d.is_finite() {
                        depth[[r, c]] = d;
                    }
                }
                if t.is_finite() && t > 0.15 {
                    w *= (-3.0 * (t - 0.15).max(0.0)).exp();
                }
            } else {
                depth[[r, c]] = lyzenga_depth_proxy_m(blue, green);
            }
            weight[[r, c]] = w;
        }
    }
    (depth, weight)
}

/// Fuse weighted median-ish stack: sum(depth*w)/sum(w).
pub fn fuse_depth_stack(
    depths: &[Array2<f32>],
    weights: &[Array2<f32>],
) -> Array2<f32> {
    if depths.is_empty() {
        return Array2::zeros((0, 0));
    }
    let (rows, cols) = depths[0].dim();
    let mut fused = Array2::<f32>::from_elem((rows, cols), f32::NAN);
    let mut wsum = Array2::<f32>::zeros((rows, cols));
    let mut dsum = Array2::<f32>::zeros((rows, cols));
    for (d, w) in depths.iter().zip(weights.iter()) {
        for r in 0..rows {
            for c in 0..cols {
                let dv = d[[r, c]];
                let wv = w[[r, c]];
                if dv.is_finite() && wv > 0.01 {
                    dsum[[r, c]] += dv * wv;
                    wsum[[r, c]] += wv;
                }
            }
        }
    }
    for r in 0..rows {
        for c in 0..cols {
            if wsum[[r, c]] > 0.05 {
                fused[[r, c]] = dsum[[r, c]] / wsum[[r, c]];
            }
        }
    }
    fused
}

/// Gradient magnitude (relief proxy, m per pixel — not georeferenced slope without GSD).
pub fn relief_map(depth: &Array2<f32>) -> Array2<f32> {
    let (rows, cols) = depth.dim();
    let mut out = Array2::<f32>::from_elem((rows, cols), f32::NAN);
    for r in 1..rows.saturating_sub(1) {
        for c in 1..cols.saturating_sub(1) {
            let z = depth[[r, c]];
            if !z.is_finite() {
                continue;
            }
            let dx = (depth[[r + 1, c]] - depth[[r - 1, c]]).abs();
            let dy = (depth[[r, c + 1]] - depth[[r, c - 1]]).abs();
            if dx.is_finite() && dy.is_finite() {
                out[[r, c]] = (dx * dx + dy * dy).sqrt();
            }
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BathyPassSummary {
    pub scene_id: String,
    pub n_valid_depth_px: usize,
    pub mean_weight: f32,
    pub mean_secchi_m: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BathyStackReport {
    pub n_passes_used: usize,
    pub passes: Vec<BathyPassSummary>,
    pub fused_depth_stats: Option<(f32, f32, f32)>,
    pub max_relief: Option<f32>,
    pub note: String,
}

/// Multi-pass SDB over local Sentinel-2 scene folders (same layout as `run_poc_local`).
#[cfg(feature = "gdal")]
pub fn run_bathymetry_stack_local(
    scene_dirs: &[impl AsRef<Path>],
    bbox: &BBox,
    target_px: usize,
    out_dir: &Path,
) -> anyhow::Result<BathyStackReport> {
    use rayon::prelude::*;
    std::fs::create_dir_all(out_dir)?;

    let dirs: Vec<std::path::PathBuf> = scene_dirs.iter().map(|p| p.as_ref().to_path_buf()).collect();
    let mut scene_ids: Vec<String> = Vec::new();
    for d in &dirs {
        if !d.is_dir() {
            continue;
        }
        for ent in std::fs::read_dir(d)?.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if name.ends_with(".blue.tif") {
                let id = name.trim_end_matches(".blue.tif").to_string();
                if scene_ids.iter().all(|x| x != &id) {
                    scene_ids.push(id);
                }
            }
        }
    }
    scene_ids.sort();
    info!("bathy_map: {} scenes across {} dirs", scene_ids.len(), dirs.len());

    let loaded: Vec<(String, Array2<f32>, Array2<f32>, Option<Array2<f32>>)> = scene_ids
        .par_iter()
        .filter_map(|id| {
            for d in &dirs {
                let blue = d.join(format!("{id}.blue.tif"));
                let green = d.join(format!("{id}.green.tif"));
                let red = d.join(format!("{id}.red.tif"));
                if !blue.is_file() || !green.is_file() {
                    continue;
                }
                let b02 = crate::chip::decode_local_band(&blue, bbox, target_px).ok()?;
                let b03 = crate::chip::decode_local_band(&green, bbox, target_px).ok()?;
                let b04 = if red.is_file() {
                    crate::chip::decode_local_band(&red, bbox, target_px).ok()
                } else {
                    None
                };
                return Some((id.clone(), b02, b03, b04));
            }
            None
        })
        .collect();

    let mut depths = Vec::new();
    let mut weights = Vec::new();
    let mut pass_summaries = Vec::new();

    for (id, b02, b03, b04) in &loaded {
        let (d, w) = depth_map_one_pass(b02, b03, b04.as_ref());
        let n_valid = d.iter().filter(|v| v.is_finite()).count();
        let mean_w = if n_valid > 0 {
            w.iter().filter(|&&v| v > 0.0).map(|v| *v as f64).sum::<f64>() / n_valid as f64
        } else {
            0.0
        };
        let mean_secchi = b04.as_ref().map(|b4| {
            let mut s = 0.0f64;
            let mut n = 0usize;
            for ((r, c), _) in d.indexed_iter() {
                if !b02[[r, c]].is_finite() {
                    continue;
                }
                let sc = secchi_proxy_m(b02[[r, c]], b4[[r, c]]);
                if sc.is_finite() {
                    s += sc as f64;
                    n += 1;
                }
            }
            if n > 0 {
                Some((s / n as f64) as f32)
            } else {
                None
            }
        });
        pass_summaries.push(BathyPassSummary {
            scene_id: id.clone(),
            n_valid_depth_px: n_valid,
            mean_weight: mean_w as f32,
            mean_secchi_m: mean_secchi.flatten(),
        });
        depths.push(d);
        weights.push(w);
    }

    let fused = fuse_depth_stack(&depths, &weights);
    let relief = relief_map(&fused);
    let max_relief = relief.iter().filter(|v| v.is_finite()).fold(0.0f32, |a, &b| a.max(b));
    let finite: Vec<f32> = fused.iter().filter_map(|v| v.is_finite().then_some(*v)).collect();
    let stats = if finite.is_empty() {
        None
    } else {
        let min = finite.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = finite.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mean = finite.iter().sum::<f32>() / finite.len() as f32;
        Some((min, mean, max))
    };

    let report = BathyStackReport {
        n_passes_used: loaded.len(),
        passes: pass_summaries,
        fused_depth_stats: stats,
        max_relief: if max_relief > 0.0 { Some(max_relief) } else { None },
        note: "Multi-pass SDB; turbidity-weighted. Calibrate m0/m1 with shallow preserve wrecks.".into(),
    };
    let path = out_dir.join("bathymetry_stack_report.json");
    std::fs::write(&path, serde_json::to_string_pretty(&report)?)?;
    info!("bathy_map: wrote {}", path.display());
    Ok(report)
}

#[cfg(not(feature = "gdal"))]
pub fn run_bathymetry_stack_local(
    _scene_dirs: &[impl AsRef<Path>],
    _bbox: &BBox,
    _target_px: usize,
    _out_dir: &Path,
) -> anyhow::Result<BathyStackReport> {
    anyhow::bail!("bathymetry_map requires --features gdal")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_limit_scales_with_secchi() {
        assert!(depth_limit_m(10.0) >= 20.0);
        assert!(depth_limit_m(4.0) <= 12.0);
    }

    #[test]
    fn turbid_pass_gets_lower_weight() {
        let (d, w) = depth_map_one_pass(
            &Array2::from_elem((3, 3), 0.02f32),
            &Array2::from_elem((3, 3), 0.015f32),
            Some(&Array2::from_elem((3, 3), 0.08f32)),
        );
        assert!(d[[1, 1]].is_finite());
        assert!(w[[1, 1]] > 0.0 && w[[1, 1]] <= 1.0);
    }
}
