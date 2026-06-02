//! Optical concept evaluators — the three detection methods from
//! `wh2k_sentinel_wreck_targeting.py`.
//!
//! Concepts
//! --------
//! shadow_roughness  — NIR roughness / current contrast (spring, March–April)
//! zebra_clarity     — zebra/quagga mussel water clarity anomaly (July–Oct)
//! sediment_plume    — post-storm NDTI turbidity anchor (April–Oct)
//!
//! Each concept has:
//!   - season window (months + archive range)
//!   - signal_direction (dark / bright / turbid)
//!   - per-scene z-score computed against an annular background chip
//!   - hit_rate = fraction of scenes with |z| > HIT_ZSCORE_MIN in the expected direction
//!   - composite score 0–10

use crate::{
    chip::{annular_masks, chip_bbox},
    spectral::{curvelet_energy_ratio, nanmean},
    stac::{search_scenes, StacQuery},
    types::{ConceptResult, Knobs, WreckTarget},
};
use anyhow::Result;
use chrono::NaiveDate;
use ndarray::{Array2, Zip};
use reqwest::Client;
use tracing::{debug, warn};

pub const HIT_ZSCORE_MIN: f64 = 1.5;

// ── Concept definitions ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Concept {
    ShadowRoughness,
    ZebraClarity,
    SedimentPlume,
}

impl Concept {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "shadow_roughness" => Some(Self::ShadowRoughness),
            "zebra_clarity" => Some(Self::ZebraClarity),
            "sediment_plume" => Some(Self::SedimentPlume),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::ShadowRoughness => "shadow_roughness",
            Self::ZebraClarity => "zebra_clarity",
            Self::SedimentPlume => "sediment_plume",
        }
    }

    pub fn months(self) -> &'static [u32] {
        match self {
            Self::ShadowRoughness => &[3, 4],
            Self::ZebraClarity => &[7, 8, 9, 10],
            Self::SedimentPlume => &[4, 5, 6, 7, 8, 9, 10],
        }
    }

    pub fn max_cloud(self) -> f64 {
        match self {
            Self::ShadowRoughness => 15.0,
            Self::ZebraClarity => 10.0,
            Self::SedimentPlume => 25.0,
        }
    }

    pub fn archive_start(self) -> NaiveDate {
        match self {
            Self::ShadowRoughness => NaiveDate::from_ymd_opt(2017, 3, 1).unwrap(),
            Self::ZebraClarity => NaiveDate::from_ymd_opt(2017, 7, 1).unwrap(),
            Self::SedimentPlume => NaiveDate::from_ymd_opt(2017, 4, 1).unwrap(),
        }
    }

    pub fn archive_end(self) -> NaiveDate {
        chrono::Local::now().naive_local().date()
    }

    /// Expected direction of the signal relative to background.
    /// "dark" = lower than background, "bright" = higher, "turbid" = higher NDTI.
    pub fn signal_direction(self) -> &'static str {
        match self {
            Self::ShadowRoughness => "dark",
            Self::ZebraClarity => "bright",
            Self::SedimentPlume => "turbid",
        }
    }
}

pub fn all_concepts() -> &'static [Concept] {
    &[Concept::ShadowRoughness, Concept::ZebraClarity, Concept::SedimentPlume]
}

pub fn resolve_concepts(knob: &str) -> Vec<Concept> {
    if knob == "all" || knob.is_empty() {
        return all_concepts().to_vec();
    }
    knob.split(',')
        .filter_map(|s| Concept::from_str(s.trim()))
        .collect()
}

// ── Concept geometry constants (mirrors Python) ───────────────────────────────
//
// These are the DEFAULT annulus/scene radii.  They are overridden per-mission by
// the `chip_signal_m` / `chip_bg_inner_m` / `chip_bg_outer_m` / `chip_scene_radius_m`
// knobs (wired in `score_wreck_concept`).

const CHIP_SIGNAL_M: f64 = 150.0; // inner radius: signal window
const CHIP_BG_INNER_M: f64 = 350.0; // background annulus inner
const CHIP_BG_OUTER_M: f64 = 1200.0; // background annulus outer
const CHIP_SCENE_RADIUS_M: f64 = 1500.0; // fetch window for STAC chip

// ── Per-scene z-score ─────────────────────────────────────────────────────────

/// Build the per-pixel concept metric grid for a chip.
///
/// Mirrors the Python concept metric extractors in
/// `wh2k_sentinel_wreck_targeting.py`:
///   shadow_roughness → `_extract_sar_metric`     (Sobel gradient magnitude of B08)
///   zebra_clarity    → `_extract_clarity_metric` (Secchi proxy 3.9*sqrt(B02/B04)+0.55)
///   sediment_plume   → `_extract_plume_metric`   (NDTI = (B04-B03)/(B04+B03))
///
/// Invalid / masked-out pixels are set to `NaN` so downstream `nanmean`/`nanstd`
/// ignore them (matching the numpy validity masks).
///
/// Band roles (must match `score_wreck_concept` request order):
///   shadow_roughness : band_a = B08            (band_b ignored)
///   zebra_clarity    : band_a = B02 (Blue), band_b = B04 (Red)
///   sediment_plume   : band_a = B04 (Red),  band_b = B03 (Green)
fn concept_metric_grid(
    concept: Concept,
    band_a: &Array2<f32>,
    band_b: &Array2<f32>,
) -> Array2<f32> {
    match concept {
        Concept::ShadowRoughness => {
            // Python `_extract_sar_metric`: NaN→0, Sobel Gx/Gy, hypot magnitude,
            // keep gradient only where the original B08 was finite.
            sobel_magnitude(band_a)
        }
        Concept::ZebraClarity => {
            // Python `_extract_clarity_metric`:
            //   valid = (b04>0.005) & (b04<0.3) & finite(b02) & finite(b04)
            //   secchi = 3.9 * sqrt(max(b02/b04, 0)) + 0.55
            let mut out = Array2::<f32>::from_elem(band_a.raw_dim(), f32::NAN);
            Zip::from(&mut out)
                .and(band_a) // B02 (Blue)
                .and(band_b) // B04 (Red)
                .for_each(|o, &b02, &b04| {
                    let b02 = b02 as f64;
                    let b04 = b04 as f64;
                    if b04 > 0.005 && b04 < 0.3 && b02.is_finite() && b04.is_finite() {
                        let ratio = (b02 / b04).max(0.0);
                        *o = (3.9 * ratio.sqrt() + 0.55) as f32;
                    }
                });
            out
        }
        Concept::SedimentPlume => {
            // Python `_extract_plume_metric`: NDTI = (b04 - b03) / (b04 + b03)
            // band_a = Red (B04), band_b = Green (B03)
            let mut out = Array2::<f32>::from_elem(band_a.raw_dim(), f32::NAN);
            Zip::from(&mut out)
                .and(band_a) // B04 (Red)
                .and(band_b) // B03 (Green)
                .for_each(|o, &r, &g| {
                    let r = r as f64;
                    let g = g as f64;
                    let d = r + g;
                    if d.abs() >= 1e-9 && r.is_finite() && g.is_finite() {
                        *o = ((r - g) / d) as f32;
                    }
                });
            out
        }
    }
}

/// Raw 2-D Sobel gradient magnitude, mirroring the core of Python
/// `_extract_sar_metric` / the POC `_concept_shadow_roughness` gradient:
///   b_c = where(finite(b), b, 0.0)
///   Gx = sobel(b_c, axis=1); Gy = sobel(b_c, axis=0); grad = hypot(Gx, Gy)
///
/// Returns the gradient magnitude at EVERY pixel (no NaN masking).  This is the
/// form the optical POC needs because it then runs a `uniform_filter` over the
/// gradient (which must be finite) before applying the validity mask.
///
/// Uses the standard 3×3 Sobel kernels with `reflect` boundary handling
/// (the scipy.ndimage default).
pub(crate) fn sobel_magnitude_raw(band: &Array2<f32>) -> Array2<f32> {
    let (h, w) = band.dim();
    let mut out = Array2::<f32>::zeros((h, w));
    if h == 0 || w == 0 {
        return out;
    }

    // NaN→0 working copy (matches np.where(isfinite, b, 0.0)).
    let at = |r: isize, c: isize| -> f64 {
        // reflect boundary: indices mirror about the edge (a b c | c b a)
        let rr = reflect_index(r, h);
        let cc = reflect_index(c, w);
        let v = band[[rr, cc]];
        if v.is_finite() { v as f64 } else { 0.0 }
    };

    for r in 0..h as isize {
        for c in 0..w as isize {
            // Gx: derivative along columns (axis=1), smoothed along rows.
            let gx = (at(r - 1, c + 1) + 2.0 * at(r, c + 1) + at(r + 1, c + 1))
                - (at(r - 1, c - 1) + 2.0 * at(r, c - 1) + at(r + 1, c - 1));
            // Gy: derivative along rows (axis=0), smoothed along columns.
            let gy = (at(r + 1, c - 1) + 2.0 * at(r + 1, c) + at(r + 1, c + 1))
                - (at(r - 1, c - 1) + 2.0 * at(r - 1, c) + at(r - 1, c + 1));
            out[[r as usize, c as usize]] = gx.hypot(gy) as f32;
        }
    }
    out
}

/// 2-D Sobel gradient magnitude, mirroring Python `_extract_sar_metric`:
///   result[i] = grad[i] where the original band was finite, else NaN
///
/// Builds on [`sobel_magnitude_raw`] and applies the validity mask.
fn sobel_magnitude(band: &Array2<f32>) -> Array2<f32> {
    let mut out = sobel_magnitude_raw(band);
    Zip::from(&mut out).and(band).for_each(|o, &b| {
        if !b.is_finite() {
            *o = f32::NAN;
        }
    });
    out
}

/// Reflect an index about array bounds (scipy `reflect` mode: (d c b a | a b c d | d c b a)).
pub(crate) fn reflect_index(i: isize, n: usize) -> usize {
    if n == 1 {
        return 0;
    }
    let n = n as isize;
    let mut i = i;
    // Period of reflection is 2*n.
    let period = 2 * n;
    i = ((i % period) + period) % period;
    if i >= n {
        i = period - 1 - i;
    }
    i as usize
}

/// Compute the concept metric z-score for a single chip array.
///
/// Builds the per-pixel concept metric grid (see `concept_metric_grid`), then:
///   z-score = (signal_mean − bg_mean) / bg_std
/// over the annular signal / background masks.  For "dark" concepts the sign is
/// flipped so a strong dark signal → positive z.
///
/// NOTE vs Python `_score_scene`: Python computes a single scalar metric over
/// the whole signal region and derives the background spread from the raw first
/// band's pixel values.  Here we evaluate the metric per pixel and take the
/// mean/std of the *metric* over each region — this is the per-pixel form the
/// Rust pipeline already used and keeps the spatial Sobel meaningful.
fn chip_zscore(
    concept: Concept,
    band_a: &Array2<f32>, // role depends on concept (see concept_metric_grid)
    band_b: &Array2<f32>,
    m_per_px: f64,
    signal_m: f64,
    bg_inner_m: f64,
    bg_outer_m: f64,
) -> Option<f64> {
    let (h, w) = band_a.dim();
    let cy = h as f64 / 2.0;
    let cx = w as f64 / 2.0;
    let (sig_idx, bg_idx) = annular_masks(h, w, cy, cx, m_per_px, signal_m, bg_inner_m, bg_outer_m);

    if sig_idx.is_empty() || bg_idx.len() < 4 {
        return None;
    }

    let metric = concept_metric_grid(concept, band_a, band_b);
    let flat: Vec<f32> = metric.iter().copied().collect();

    let sig_vals: Vec<f32> = sig_idx.iter().map(|&i| flat[i]).collect();
    let bg_vals: Vec<f32> = bg_idx.iter().map(|&i| flat[i]).collect();

    let sig_mean = nanmean(&sig_vals)?;
    let bg_mean = nanmean(&bg_vals)?;
    let bg_std = {
        let bg64: Vec<f64> = bg_vals.iter().filter(|v| v.is_finite()).map(|v| *v as f64).collect();
        if bg64.len() < 2 {
            return None;
        }
        let mu = bg64.iter().sum::<f64>() / bg64.len() as f64;
        (bg64.iter().map(|v| (v - mu).powi(2)).sum::<f64>() / bg64.len() as f64).sqrt()
    };
    if bg_std < 1e-9 {
        return Some(0.0);
    }

    let z = (sig_mean - bg_mean) / bg_std;
    // Dark concept: flip sign so positive z = darker centre = strong signal
    Some(if concept.signal_direction() == "dark" { -z } else { z })
}

// ── Main concept scorer ────────────────────────────────────────────────────────

/// Evaluate one concept for one wreck over the full multi-year archive.
///
/// Returns a ConceptResult with n_scenes, hit_rate, best_zscore, and composite score.
/// In dry-run or offline mode (no band chips available) returns a fixture result.
pub async fn score_wreck_concept(
    client: &Client,
    wreck: &WreckTarget,
    concept: Concept,
    knobs: &Knobs,
    chip_cache_dir: &std::path::Path,
) -> Result<ConceptResult> {
    // Chip geometry from knobs (fall back to module defaults when unset/<=0).
    let signal_m = if knobs.chip_signal_m > 0.0 { knobs.chip_signal_m } else { CHIP_SIGNAL_M };
    let bg_inner_m = if knobs.chip_bg_inner_m > 0.0 { knobs.chip_bg_inner_m } else { CHIP_BG_INNER_M };
    let bg_outer_m = if knobs.chip_bg_outer_m > 0.0 { knobs.chip_bg_outer_m } else { CHIP_BG_OUTER_M };
    let chip_r = if knobs.chip_scene_radius_m > 0.0 { knobs.chip_scene_radius_m } else { CHIP_SCENE_RADIUS_M };
    let bbox = chip_bbox(wreck.lat, wreck.lon, chip_r);

    let query = StacQuery {
        bbox: bbox.to_stac_array(),
        date_start: concept.archive_start(),
        date_end: concept.archive_end(),
        max_cloud: concept.max_cloud(),
        month_filter: concept.months(),
        limit: knobs.max_scenes,
    };

    let scenes = match search_scenes(client, &query).await {
        Ok(s) => s,
        Err(e) => {
            warn!("STAC search failed for {} / {}: {e}", wreck.name, concept.name());
            vec![]
        }
    };

    let n_scenes = scenes.len();
    if n_scenes == 0 {
        return Ok(ConceptResult {
            wreck_id: wreck.id.clone(),
            wreck_name: wreck.name.clone(),
            lat: wreck.lat,
            lon: wreck.lon,
            depth_m: wreck.depth_m,
            concept: concept.name().into(),
            n_scenes: 0,
            n_hits: 0,
            hit_rate: 0.0,
            mean_zscore: 0.0,
            best_zscore: 0.0,
            best_date: None,
            score: 0.0,
            notes: "no scenes found".into(),
        });
    }

    // Determine which two band names to request per concept.
    // Mirrors Python CONCEPT_BANDS in wh2k_sentinel_wreck_targeting.py.
    let (band_a_name, band_b_name) = match concept {
        Concept::ShadowRoughness => ("B08", "B08"), // NIR only; use same for both
        Concept::ZebraClarity => ("B02", "B04"),    // Blue, Red (Secchi proxy)
        Concept::SedimentPlume => ("B04", "B03"),   // Red, Green (NDTI)
    };

    let m_per_px = 10.0; // Sentinel-2 10-m bands
    let chip_px = (2.0 * chip_r / m_per_px).ceil() as usize;

    let mut z_scores: Vec<f64> = Vec::new();
    let mut best_zscore: f64 = 0.0;
    let mut best_date: Option<NaiveDate> = None;
    let mut best_curve_ratio: f64 = 0.0;

    for scene in &scenes {
        let href_a = match scene.asset_href(band_a_name) {
            Some(h) => h,
            None => {
                debug!("Scene {} missing band {band_a_name}", scene.id);
                continue;
            }
        };
        let href_b = match scene.asset_href(band_b_name) {
            Some(h) => h,
            None => {
                debug!("Scene {} missing band {band_b_name}", scene.id);
                continue;
            }
        };

        let chip_a = match crate::chip::download_cog_chip(client, href_a, &bbox, chip_px, chip_cache_dir).await {
            Ok(c) => c,
            Err(e) => { debug!("chip download failed {}: {e}", scene.id); continue; }
        };
        let chip_b = if band_a_name == band_b_name {
            chip_a.clone()
        } else {
            match crate::chip::download_cog_chip(client, href_b, &bbox, chip_px, chip_cache_dir).await {
                Ok(c) => c,
                Err(e) => { debug!("chip download failed {}: {e}", scene.id); continue; }
            }
        };

        if let Some(z) = chip_zscore(concept, &chip_a, &chip_b, m_per_px, signal_m, bg_inner_m, bg_outer_m) {
            z_scores.push(z);
            if z > best_zscore {
                best_zscore = z;
                best_date = Some(scene.datetime);
            }

            if knobs.use_curvelet_rescore {
                // Reuse the same per-pixel concept metric grid the z-score uses.
                let metric_grid = concept_metric_grid(concept, &chip_a, &chip_b);

                if let Some(ratio) = curvelet_energy_ratio(metric_grid.view(), knobs.curvelet_num_scales) {
                    if ratio > best_curve_ratio {
                        best_curve_ratio = ratio;
                    }
                }
            }
        }
    }

    let n_hits = z_scores.iter().filter(|&&z| z >= HIT_ZSCORE_MIN).count();
    let hit_rate = if n_scenes > 0 { n_hits as f64 / n_scenes as f64 } else { 0.0 };
    let mean_zscore = if z_scores.is_empty() {
        0.0
    } else {
        z_scores.iter().sum::<f64>() / z_scores.len() as f64
    };

    // Composite score 0–10 — mirrors Python `run_concept_for_wreck`:
    //   hr_score  = hit_rate * 10
    //   z_score10 = min(10, max(0, best_zscore / 4 * 10))
    //   score     = 0.5 * hr_score + 0.5 * z_score10
    // The optional curvelet rescore is an ADDITIVE term applied ONLY when
    // knobs.use_curvelet_rescore is true, so default behaviour matches Python.
    // The depth term from the old Rust scorer has been removed (Python has none).
    let score = {
        let hr_score = hit_rate * 10.0;
        let z_score10 = (best_zscore / 4.0 * 10.0).min(10.0).max(0.0);
        let base = 0.5 * hr_score + 0.5 * z_score10;
        let curve_pts = if knobs.use_curvelet_rescore {
            let t = knobs.curvelet_energy_threshold.max(1e-6);
            (best_curve_ratio / t).min(1.0) * 1.5
        } else {
            0.0
        };
        (base + curve_pts).min(10.0)
    };

    Ok(ConceptResult {
        wreck_id: wreck.id.clone(),
        wreck_name: wreck.name.clone(),
        lat: wreck.lat,
        lon: wreck.lon,
        depth_m: wreck.depth_m,
        concept: concept.name().into(),
        n_scenes,
        n_hits,
        hit_rate,
        mean_zscore,
        best_zscore,
        best_date,
        score,
        notes: if knobs.use_curvelet_rescore {
            format!("curvelet_ratio={best_curve_ratio:.3}")
        } else {
            String::new()
        },
    })
}

/// Evaluate all requested concepts for a single wreck in parallel (rayon).
pub async fn score_wreck_all_concepts(
    client: &Client,
    wreck: &WreckTarget,
    knobs: &Knobs,
    chip_cache_dir: &std::path::Path,
) -> Vec<ConceptResult> {
    let concepts = resolve_concepts(&knobs.concepts);
    let mut results = Vec::new();
    for concept in concepts {
        match score_wreck_concept(client, wreck, concept, knobs, chip_cache_dir).await {
            Ok(r) => results.push(r),
            Err(e) => warn!("concept scoring failed {}/{}: {e}", wreck.name, concept.name()),
        }
    }
    results
}

/// CSV serialisation of concept results.
pub fn results_to_csv(results: &[ConceptResult]) -> String {
    let mut out = String::from(
        "wreck_id,wreck_name,lat,lon,depth_m,concept,n_scenes,n_hits,hit_rate,\
         mean_zscore,best_zscore,best_date,score,notes\n",
    );
    for r in results {
        out.push_str(&format!(
            "{},{},{:.6},{:.6},{:.1},{},{},{},{:.4},{:.4},{:.4},{},{:.2},{}\n",
            r.wreck_id,
            r.wreck_name,
            r.lat,
            r.lon,
            r.depth_m,
            r.concept,
            r.n_scenes,
            r.n_hits,
            r.hit_rate,
            r.mean_zscore,
            r.best_zscore,
            r.best_date.map(|d| d.to_string()).unwrap_or_default(),
            r.score,
            r.notes,
        ));
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use ndarray::Array2;

    #[test]
    fn clarity_metric_matches_secchi_proxy() {
        // Python `_extract_clarity_metric`: secchi = 3.9*sqrt(B02/B04)+0.55,
        // valid where 0.005 < B04 < 0.3.
        let b02 = Array2::from_elem((4, 4), 0.16f32); // Blue
        let b04 = Array2::from_elem((4, 4), 0.04f32); // Red (valid: 0.005<0.04<0.3)
        let grid = concept_metric_grid(Concept::ZebraClarity, &b02, &b04);
        // ratio = 0.16/0.04 = 4 → 3.9*2 + 0.55 = 8.35
        for v in grid.iter() {
            assert_relative_eq!(*v as f64, 8.35, epsilon = 1e-4);
        }
    }

    #[test]
    fn clarity_metric_masks_invalid_red() {
        // B04 out of (0.005, 0.3) range → NaN (matches numpy validity mask).
        let b02 = Array2::from_elem((2, 2), 0.16f32);
        let b04 = Array2::from_elem((2, 2), 0.5f32); // > 0.3 → invalid
        let grid = concept_metric_grid(Concept::ZebraClarity, &b02, &b04);
        assert!(grid.iter().all(|v| v.is_nan()));
    }

    #[test]
    fn plume_metric_matches_ndti() {
        // Python `_extract_plume_metric`: NDTI = (B04 - B03) / (B04 + B03).
        let b04 = Array2::from_elem((3, 3), 0.15f32); // Red (band_a)
        let b03 = Array2::from_elem((3, 3), 0.05f32); // Green (band_b)
        let grid = concept_metric_grid(Concept::SedimentPlume, &b04, &b03);
        // (0.15 - 0.05) / (0.15 + 0.05) = 0.10 / 0.20 = 0.5
        for v in grid.iter() {
            assert_relative_eq!(*v as f64, 0.5, epsilon = 1e-5);
        }
    }

    #[test]
    fn sobel_magnitude_flat_is_zero() {
        // A flat field has zero gradient everywhere (Sobel of a constant = 0).
        let b08 = Array2::from_elem((8, 8), 0.3f32);
        let grad = sobel_magnitude(&b08);
        for v in grad.iter() {
            assert_relative_eq!(*v as f64, 0.0, epsilon = 1e-6);
        }
    }

    #[test]
    fn sobel_magnitude_detects_vertical_edge() {
        // A sharp horizontal step → strong vertical (Gy) gradient at the seam.
        let mut b08 = Array2::<f32>::zeros((6, 6));
        for r in 3..6 {
            for c in 0..6 {
                b08[[r, c]] = 1.0;
            }
        }
        let grad = sobel_magnitude(&b08);
        // The maximum gradient should be sizable and located near the row-2/3 seam.
        let maxg = grad.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(maxg > 1.0, "expected a strong edge response, got {maxg}");
    }

    #[test]
    fn reflect_index_mirrors_bounds() {
        // scipy 'reflect' mode: (a b c | c b a) style mirroring about edges.
        assert_eq!(reflect_index(-1, 4), 0);
        assert_eq!(reflect_index(-2, 4), 1);
        assert_eq!(reflect_index(4, 4), 3);
        assert_eq!(reflect_index(5, 4), 2);
        assert_eq!(reflect_index(2, 4), 2);
    }

    #[test]
    fn zscore_clarity_bright_centre_positive() {
        // Bright (clearer) centre vs a noisy darker background → positive z for
        // the "bright" zebra_clarity direction.
        let m_per_px = 10.0;
        let n = 300usize; // 3000 m / 10 m  → covers bg outer radius (1200 m)
        let mut b02 = Array2::from_elem((n, n), 0.08f32); // bg blue
        let b04 = Array2::from_elem((n, n), 0.04f32);     // red, valid range
        let cy = n / 2;
        let cx = n / 2;
        for r in 0..n {
            for c in 0..n {
                let dy = r as f64 - cy as f64;
                let dx = c as f64 - cx as f64;
                let dist_m = (dy * dy + dx * dx).sqrt() * m_per_px;
                if dist_m <= CHIP_SIGNAL_M {
                    b02[[r, c]] = 0.36; // ratio 9 → much clearer centre
                } else {
                    // Give the background a small deterministic spread so bg_std > 0.
                    let jitter = (((r * 31 + c * 17) % 7) as f32) * 0.002;
                    b02[[r, c]] = 0.08 + jitter;
                }
            }
        }
        let z = chip_zscore(Concept::ZebraClarity, &b02, &b04, m_per_px, CHIP_SIGNAL_M, CHIP_BG_INNER_M, CHIP_BG_OUTER_M).expect("z");
        assert!(z > 1.0, "clear centre should yield positive clarity z, got {z}");
    }

    #[test]
    fn composite_score_matches_python_formula() {
        // Replicates Python `run_concept_for_wreck`:
        //   score = 0.5*(hit_rate*10) + 0.5*min(10, max(0, best_z/4*10))
        // with curvelet OFF (default) — no depth term.
        let hit_rate = 0.5_f64;
        let best_zscore = 2.0_f64;
        let hr_score = hit_rate * 10.0;
        let z_score10 = (best_zscore / 4.0 * 10.0).min(10.0).max(0.0);
        let base = 0.5 * hr_score + 0.5 * z_score10;
        // 0.5*5 + 0.5*5 = 5.0
        assert_relative_eq!(base, 5.0, epsilon = 1e-9);
    }
}
