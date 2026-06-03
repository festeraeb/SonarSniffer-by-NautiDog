//! AOI-discovery POC — full-scene peak finder.
//!
//! Ports `wh2k_sentinel_optical_poc.py` (the optical proof-of-concept that
//! finds wreck-scale anomalies across a whole scene, as opposed to scoring a
//! known wreck location).  Previously `mission.rs::stage_poc_aoi` shelled out
//! to `python3 wh2k_sentinel_optical_poc.py`; this module replaces that.
//!
//! Ported functions (Python name → Rust):
//!   `_downsample`               → [`downsample`]            (block-average, NaN-aware)
//!   `_masked_zscore`            → [`masked_zscore`]
//!   `_pixel_coords`             → [`pixel_coords`]
//!   `_find_peak_clusters`       → [`find_peak_clusters`]    (maximum_filter NMS)
//!   `_cross_reference`          → [`cross_reference`]       (nearby_radius_m=2000)
//!   `_concept_shadow_roughness` → [`concept_shadow_roughness`] (Sobel B08 + bg subtract)
//!   `_concept_zebra_clarity`    → [`concept_zebra_clarity`]    (Secchi + 50px bg + residual)
//!   `_concept_sediment_plume`   → [`concept_sediment_plume`]   (NDTI / delta-NDTI)
//!
//! The Sobel gradient is reused from `concept.rs` (`sobel_magnitude_raw`).
//! `scipy.ndimage.uniform_filter` / `maximum_filter` are reimplemented as
//! separable box filters with `reflect` boundary handling (the scipy default).

use crate::{
    chip::{download_cog_chip, haversine_m},
    concept::{reflect_index, sobel_magnitude_raw},
    stac::{search_scenes, Scene, StacQuery},
    types::{BBox, Knobs},
};
use anyhow::Result;
use chrono::NaiveDate;
use ndarray::Array2;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

// ── Candidate output (mirrors Python OpticalCandidate dataclass) ──────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpticalCandidate {
    pub lat: f64,
    pub lon: f64,
    /// shadow_roughness | zebra_clarity | sediment_plume
    pub concept: String,
    /// 0–10 optical confidence
    pub score: f64,
    /// integer 0–10 after cross-ref proxy discount
    pub wreck_score: i64,
    /// ISO date of source image
    pub scene_date: String,
    /// raw signal (gradient, clarity, NDTI)
    pub metric: f64,
    /// z-score vs local lake background
    pub metric_zscore: f64,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub known_wreck_nearby: bool,
    #[serde(default = "default_nearest_known_m")]
    pub nearest_known_m: f64,
}

fn default_nearest_known_m() -> f64 {
    99999.0
}

/// OpenMemory guardrail: cap optical z-scores before peak extraction.
pub const BLUE_GREEN_Z_CAP: f64 = 4.0;

/// Mask a border band (3×3 erosion equivalent at margin px) to suppress edge false positives.
fn mask_edge_band(arr: &mut Array2<f32>, margin: usize) {
    if margin == 0 {
        return;
    }
    let (h, w) = arr.dim();
    for r in 0..h {
        for c in 0..w {
            if r < margin || c < margin || r + margin >= h || c + margin >= w {
                arr[[r, c]] = f32::NAN;
            }
        }
    }
}

fn blue_green_score_from_z(zsc: f64) -> (f64, i64) {
    let score = (zsc / BLUE_GREEN_Z_CAP * 10.0).min(10.0).max(0.0);
    let wreck_score = ((score * 0.8).round() as i64).clamp(0, 10);
    (round2(score), wreck_score)
}

// ── Tunable config (wired from Knobs) ─────────────────────────────────────────

/// POC tunables — wired from [`Knobs`] in `stage_poc_aoi` instead of hardcoding.
#[derive(Debug, Clone)]
pub struct PocConfig {
    /// z-score threshold for peak detection (`zscore_threshold`).
    pub zscore_threshold: f64,
    /// NMS window override (px). 0 → use the per-concept Python default.
    pub min_separation_px: usize,
    /// Max candidates per concept (`max_candidates`).
    pub max_candidates: usize,
    /// Longest-axis target after block-average downsample (Python `max_dim`).
    pub downsample_max_dim: usize,
    /// `_cross_reference` nearby_radius_m.
    pub xref_nearby_radius_m: f64,
}

impl PocConfig {
    pub fn from_knobs(k: &Knobs) -> Self {
        Self {
            zscore_threshold: k.poc_zscore_threshold,
            min_separation_px: k.poc_min_separation_px,
            max_candidates: if k.poc_max_candidates == 0 { 25 } else { k.poc_max_candidates },
            downsample_max_dim: if k.poc_downsample_max_dim == 0 { 2000 } else { k.poc_downsample_max_dim },
            xref_nearby_radius_m: k.xref_nearby_radius_m,
        }
    }

    /// Effective NMS window: explicit knob override, else the per-concept default.
    fn min_sep(&self, concept_default: usize) -> usize {
        if self.min_separation_px > 0 { self.min_separation_px } else { concept_default }
    }
}

impl Default for PocConfig {
    fn default() -> Self {
        Self {
            zscore_threshold: 2.5,
            min_separation_px: 0,
            max_candidates: 25,
            downsample_max_dim: 2000,
            xref_nearby_radius_m: 2000.0,
        }
    }
}

// ── _downsample ───────────────────────────────────────────────────────────────

/// Block-average downsample by an integer factor, ignoring NaN.
///
/// Mirrors Python `_downsample`: trims to a multiple of `factor`, reshapes into
/// `(rows/f, f, cols/f, f)` blocks and takes `np.nanmean` over each block.  A
/// block that is entirely NaN becomes NaN.
pub fn downsample(arr: &Array2<f32>, factor: usize) -> Array2<f32> {
    if factor <= 1 {
        return arr.clone();
    }
    let (rows, cols) = arr.dim();
    let rows2 = (rows / factor) * factor;
    let cols2 = (cols / factor) * factor;
    let out_r = rows2 / factor;
    let out_c = cols2 / factor;
    let mut out = Array2::<f32>::from_elem((out_r, out_c), f32::NAN);
    for br in 0..out_r {
        for bc in 0..out_c {
            let mut sum = 0.0f64;
            let mut n = 0usize;
            for dr in 0..factor {
                for dc in 0..factor {
                    let v = arr[[br * factor + dr, bc * factor + dc]];
                    if v.is_finite() {
                        sum += v as f64;
                        n += 1;
                    }
                }
            }
            if n > 0 {
                out[[br, bc]] = (sum / n as f64) as f32;
            }
        }
    }
    out
}

/// `factor = max(1, max(shape) // max_dim)` — Python downsample factor selection.
fn downsample_factor(shape: (usize, usize), max_dim: usize) -> usize {
    (shape.0.max(shape.1) / max_dim.max(1)).max(1)
}

// ── _masked_zscore ──────────────────────────────────────────────────────────

/// Z-score over finite pixels (NaN-aware), mirroring Python `_masked_zscore`.
///
/// Returns all-zeros when fewer than 100 finite pixels are present or when the
/// finite spread is ~0.  NaN pixels remain NaN in the output.
pub fn masked_zscore(arr: &Array2<f32>) -> Array2<f32> {
    let finite: Vec<f64> = arr.iter().filter(|v| v.is_finite()).map(|v| *v as f64).collect();
    if finite.len() < 100 {
        return Array2::zeros(arr.raw_dim());
    }
    let n = finite.len() as f64;
    let mu = finite.iter().sum::<f64>() / n;
    let var = finite.iter().map(|v| (v - mu).powi(2)).sum::<f64>() / n; // np.nanstd ddof=0
    let sigma = var.sqrt();
    if sigma < 1e-9 {
        return Array2::zeros(arr.raw_dim());
    }
    arr.mapv(|v| if v.is_finite() { ((v as f64 - mu) / sigma) as f32 } else { f32::NAN })
}

// ── _pixel_coords ─────────────────────────────────────────────────────────────

/// Build (lat_grid, lon_grid) for an array of shape (rows, cols) over `bbox`.
///
/// Mirrors Python `_pixel_coords`:
///   lats = linspace(north, south, rows)   (row 0 = north = lat_max)
///   lons = linspace(west,  east,  cols)   (col 0 = west  = lon_min)
pub fn pixel_coords(rows: usize, cols: usize, bbox: &BBox) -> (Array2<f64>, Array2<f64>) {
    let lin = |start: f64, end: f64, n: usize, i: usize| -> f64 {
        if n <= 1 { start } else { start + (end - start) * (i as f64) / ((n - 1) as f64) }
    };
    let mut lat_grid = Array2::<f64>::zeros((rows, cols));
    let mut lon_grid = Array2::<f64>::zeros((rows, cols));
    for r in 0..rows {
        let lat = lin(bbox.lat_max, bbox.lat_min, rows, r); // north → south
        for c in 0..cols {
            lat_grid[[r, c]] = lat;
            lon_grid[[r, c]] = lin(bbox.lon_min, bbox.lon_max, cols, c); // west → east
        }
    }
    (lat_grid, lon_grid)
}

// ── scipy.ndimage filters (separable, reflect boundary) ───────────────────────

/// Local-mean baseline residual (NaN-aware). Used by temporal clarity stack per Gemini collab.
pub fn baseline_residual(map: &Array2<f32>, win: usize) -> Array2<f32> {
    if win <= 1 {
        return map.clone();
    }
    let cleaned = map.mapv(|v| if v.is_finite() { v } else { 0.0 });
    let bg = uniform_filter(&cleaned, win);
    let valid = map.mapv(|v| if v.is_finite() { 1.0f32 } else { 0.0 });
    let bg_valid = uniform_filter(&valid, win);
    let (rows, cols) = map.dim();
    let mut out = Array2::<f32>::from_elem((rows, cols), f32::NAN);
    for r in 0..rows {
        for c in 0..cols {
            if map[[r, c]].is_finite() && bg_valid[[r, c]] > 0.1 {
                let bg_mean = bg[[r, c]] as f64 / (bg_valid[[r, c]] as f64).max(1e-6);
                out[[r, c]] = (map[[r, c]] as f64 - bg_mean) as f32;
            }
        }
    }
    out
}

/// Per-pixel glint roughness proxy (local B02/B03 variance), no peak extraction.
pub fn glint_variance_map(b02: &Array2<f32>, b03: &Array2<f32>) -> Array2<f32> {
    if b02.is_empty() || b03.is_empty() || b02.dim() != b03.dim() {
        return Array2::zeros((0, 0));
    }
    let (rows, cols) = b02.dim();
    let mut bright = Array2::<f32>::from_elem((rows, cols), f32::NAN);
    for r in 0..rows {
        for c in 0..cols {
            let blue = b02[[r, c]];
            let green = b03[[r, c]];
            if blue > 0.001 && green > 0.001 && blue.is_finite() && green.is_finite() {
                bright[[r, c]] = (blue + green) * 0.5;
            }
        }
    }
    let win = 5usize;
    let mean = uniform_filter(&bright, win);
    let mean_sq = uniform_filter(&bright.mapv(|v| if v.is_finite() { v * v } else { f32::NAN }), win);
    let mut variance = Array2::<f32>::from_elem((rows, cols), f32::NAN);
    for r in 0..rows {
        for c in 0..cols {
            let m = mean[[r, c]];
            let ms = mean_sq[[r, c]];
            if m.is_finite() && ms.is_finite() {
                let v = ms - m * m;
                if v.is_finite() && v > 0.0 {
                    variance[[r, c]] = v;
                }
            }
        }
    }
    variance
}

/// `scipy.ndimage.uniform_filter` (box mean) with `reflect` boundary.
///
/// Separable: horizontal then vertical 1-D box means.  Window of length `size`
/// is centred (`lo = size/2` on the left).  Operates in f64 internally.
fn uniform_filter(arr: &Array2<f32>, size: usize) -> Array2<f32> {
    if size <= 1 {
        return arr.clone();
    }
    let (h, w) = arr.dim();
    let lo = (size / 2) as isize;
    // Horizontal pass (axis=1).
    let mut tmp = Array2::<f64>::zeros((h, w));
    for r in 0..h {
        for c in 0..w {
            let mut sum = 0.0f64;
            for k in 0..size {
                let idx = c as isize - lo + k as isize;
                let cc = reflect_index(idx, w);
                sum += arr[[r, cc]] as f64;
            }
            tmp[[r, c]] = sum / size as f64;
        }
    }
    // Vertical pass (axis=0).
    let mut out = Array2::<f32>::zeros((h, w));
    for c in 0..w {
        for r in 0..h {
            let mut sum = 0.0f64;
            for k in 0..size {
                let idx = r as isize - lo + k as isize;
                let rr = reflect_index(idx, h);
                sum += tmp[[rr, c]];
            }
            out[[r, c]] = (sum / size as f64) as f32;
        }
    }
    out
}

/// `scipy.ndimage.maximum_filter` (box max) with `reflect` boundary.
///
/// Separable square max.  NaN pixels are treated as −∞ so finite neighbours win
/// (a NaN pixel can therefore never be a local maximum — matching the fact that
/// `score_map == maximum_filter(score_map)` is always False at NaN pixels in
/// numpy).
fn maximum_filter(arr: &Array2<f32>, size: usize) -> Array2<f32> {
    if size <= 1 {
        return arr.clone();
    }
    let (h, w) = arr.dim();
    let lo = (size / 2) as isize;
    let val = |v: f32| -> f64 { if v.is_finite() { v as f64 } else { f64::NEG_INFINITY } };

    // Horizontal pass.
    let mut tmp = Array2::<f64>::from_elem((h, w), f64::NEG_INFINITY);
    for r in 0..h {
        for c in 0..w {
            let mut m = f64::NEG_INFINITY;
            for k in 0..size {
                let idx = c as isize - lo + k as isize;
                let cc = reflect_index(idx, w);
                m = m.max(val(arr[[r, cc]]));
            }
            tmp[[r, c]] = m;
        }
    }
    // Vertical pass.
    let mut out = Array2::<f32>::from_elem((h, w), f32::NEG_INFINITY);
    for c in 0..w {
        for r in 0..h {
            let mut m = f64::NEG_INFINITY;
            for k in 0..size {
                let idx = r as isize - lo + k as isize;
                let rr = reflect_index(idx, h);
                m = m.max(tmp[[rr, c]]);
            }
            out[[r, c]] = m as f32;
        }
    }
    out
}

// ── _find_peak_clusters ──────────────────────────────────────────────────────

/// Non-max-suppression peak finder.  Returns `(lat, lon, score_raw, zscore)`
/// tuples sorted by descending z-score, capped to `max_candidates`.
///
/// Mirrors Python `_find_peak_clusters`:
///   zs = masked_zscore(score_map)
///   thresh = (zs >= threshold) & finite(score_map)
///   local_max = (score_map == maximum_filter(score_map, size=min_separation_px))
///   peaks = thresh & local_max
pub fn find_peak_clusters(
    score_map: &Array2<f32>,
    lat_grid: &Array2<f64>,
    lon_grid: &Array2<f64>,
    zscore_threshold: f64,
    min_separation_px: usize,
    max_candidates: usize,
) -> Vec<(f64, f64, f64, f64)> {
    let zs = masked_zscore(score_map);
    let any_above = zs.iter().zip(score_map.iter()).any(|(z, s)| (*z as f64) >= zscore_threshold && s.is_finite());
    if !any_above {
        return vec![];
    }
    let local_max = maximum_filter(score_map, min_separation_px.max(1));
    let (h, w) = score_map.dim();
    let mut candidates: Vec<(f64, f64, f64, f64)> = Vec::new();
    for r in 0..h {
        for c in 0..w {
            let s = score_map[[r, c]];
            let z = zs[[r, c]] as f64;
            if s.is_finite() && z >= zscore_threshold && s == local_max[[r, c]] {
                candidates.push((lat_grid[[r, c]], lon_grid[[r, c]], s as f64, z));
            }
        }
    }
    // Sort by z-score descending, keep top N.
    candidates.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    candidates.truncate(max_candidates);
    candidates
}

// ── _cross_reference ─────────────────────────────────────────────────────────

/// Flag each candidate with whether a known wreck lies within `nearby_radius_m`.
///
/// Mirrors Python `_cross_reference` (default nearby_radius_m=2000).
/// `known_wrecks` is a slice of (lat, lon) points.
pub fn cross_reference(
    candidates: &mut [OpticalCandidate],
    known_wrecks: &[(f64, f64)],
    nearby_radius_m: f64,
) {
    for c in candidates.iter_mut() {
        let best = known_wrecks
            .iter()
            .map(|(wlat, wlon)| haversine_m(c.lat, c.lon, *wlat, *wlon))
            .fold(99999.0_f64, f64::min);
        c.nearest_known_m = best;
        c.known_wreck_nearby = best <= nearby_radius_m;
    }
}

// ── Concept A: shadow / surface roughness ────────────────────────────────────

/// `_concept_shadow_roughness`: NIR (B08) Sobel roughness, broad background
/// subtraction via `uniform_filter`, z-scored residual, NMS peaks.
pub fn concept_shadow_roughness(
    b08: &Array2<f32>,
    bbox: &BBox,
    scene_date: &str,
    cfg: &PocConfig,
) -> Vec<OpticalCandidate> {
    if b08.len() < 100 {
        warn!("Concept A: B08 band missing or too small");
        return vec![];
    }
    let factor = downsample_factor(b08.dim(), cfg.downsample_max_dim);
    let b08 = if factor > 1 { downsample(b08, factor) } else { b08.clone() };

    // Sobel gradient magnitude of NaN→0 B08 (matches b08_clean = where(finite, b08, 0)).
    let grad_mag = sobel_magnitude_raw(&b08);

    // Broad background subtraction: uniform_filter size = max(3, 30 // factor).
    let bg_size = std::cmp::max(3, 30 / factor);
    let bg = uniform_filter(&grad_mag, bg_size);
    let mut residual = &grad_mag - &bg;
    // residual = where(finite(b08), residual, NaN)
    ndarray::Zip::from(&mut residual).and(&b08).for_each(|res, &v| {
        if !v.is_finite() {
            *res = f32::NAN;
        }
    });

    let (rows, cols) = residual.dim();
    let (lat_grid, lon_grid) = pixel_coords(rows, cols, bbox);
    let peaks = find_peak_clusters(
        &residual,
        &lat_grid,
        &lon_grid,
        cfg.zscore_threshold,
        cfg.min_sep(15),
        cfg.max_candidates,
    );

    peaks
        .into_iter()
        .map(|(lat, lon, metric, zsc)| {
            let score = (zsc / 5.0 * 10.0).min(10.0);
            let wreck_score = ((score * 0.8).round() as i64).clamp(0, 10);
            OpticalCandidate {
                lat,
                lon,
                concept: "shadow_roughness".into(),
                score: round2(score),
                wreck_score,
                scene_date: scene_date.into(),
                metric: round_n(metric, 6),
                metric_zscore: round_n(zsc, 3),
                note: "NIR Sobel roughness anomaly".into(),
                known_wreck_nearby: false,
                nearest_known_m: 99999.0,
            }
        })
        .collect()
}

// ── Concept B: zebra mussel clarity ──────────────────────────────────────────

/// `_concept_zebra_clarity`: Secchi clarity proxy, 50-px local background,
/// residual z-scored, NMS peaks.
pub fn concept_zebra_clarity(
    b02: &Array2<f32>,
    b04: &Array2<f32>,
    bbox: &BBox,
    scene_date: &str,
    cfg: &PocConfig,
) -> Vec<OpticalCandidate> {
    if b02.is_empty() || b04.is_empty() {
        warn!("Concept B: B02 or B04 missing");
        return vec![];
    }
    let factor = downsample_factor(b02.dim(), cfg.downsample_max_dim);
    let (b02, b04) = if factor > 1 {
        (downsample(b02, factor), downsample(b04, factor))
    } else {
        (b02.clone(), b04.clone())
    };
    if b02.dim() != b04.dim() {
        warn!("Concept B: B02 shape != B04 shape");
        return vec![];
    }

    // ratio = where (b04>0.005)&(b04<0.3)&finite → b02/b04 else NaN
    // secchi = where finite(ratio) → 3.9*sqrt(ratio)+0.55 else NaN
    let (rows, cols) = b02.dim();
    let mut secchi = Array2::<f32>::from_elem((rows, cols), f32::NAN);
    ndarray::Zip::from(&mut secchi).and(&b02).and(&b04).for_each(|s, &blue, &red| {
        let blue = blue as f64;
        let red = red as f64;
        if red > 0.005 && red < 0.3 && blue.is_finite() && red.is_finite() {
            let ratio = blue / red;
            if ratio.is_finite() && ratio >= 0.0 {
                *s = (3.9 * ratio.sqrt() + 0.55) as f32;
            }
        }
    });

    // Background over 50-px window: mean of valid secchi (NaN→0) / fraction valid.
    let secchi_clean = secchi.mapv(|v| if v.is_finite() { v } else { 0.0 });
    let bg = uniform_filter(&secchi_clean, 50);
    let valid = secchi.mapv(|v| if v.is_finite() { 1.0f32 } else { 0.0 });
    let bg_valid = uniform_filter(&valid, 50);

    let mut residual = Array2::<f32>::from_elem((rows, cols), f32::NAN);
    for r in 0..rows {
        for c in 0..cols {
            if secchi[[r, c]].is_finite() && bg_valid[[r, c]] > 0.1 {
                let bg_mean = bg[[r, c]] as f64 / (bg_valid[[r, c]] as f64).max(1e-6);
                residual[[r, c]] = (secchi[[r, c]] as f64 - bg_mean) as f32;
            }
        }
    }

    let (lat_grid, lon_grid) = pixel_coords(rows, cols, bbox);
    let peaks = find_peak_clusters(
        &residual,
        &lat_grid,
        &lon_grid,
        cfg.zscore_threshold,
        cfg.min_sep(10),
        cfg.max_candidates,
    );

    peaks
        .into_iter()
        .map(|(lat, lon, metric, zsc)| {
            let score = (zsc / 5.0 * 10.0).min(10.0);
            let wreck_score = ((score * 0.85).round() as i64).clamp(0, 10);
            OpticalCandidate {
                lat,
                lon,
                concept: "zebra_clarity".into(),
                score: round2(score),
                wreck_score,
                scene_date: scene_date.into(),
                metric: round_n(metric, 4),
                metric_zscore: round_n(zsc, 3),
                note: "Secchi clarity anomaly — possible zebra mussel colonisation".into(),
                known_wreck_nearby: false,
                nearest_known_m: 99999.0,
            }
        })
        .collect()
}

// ── Concept C: post-storm sediment plume ─────────────────────────────────────

/// `_concept_sediment_plume`: NDTI turbidity (or delta-NDTI vs a baseline),
/// z-scored, NMS peaks.
///
/// `baseline` is the optional clear-water (B04, B03) pair used for delta-NDTI.
pub fn concept_sediment_plume(
    b04: &Array2<f32>,
    b03: &Array2<f32>,
    bbox: &BBox,
    scene_date: &str,
    baseline: Option<(&Array2<f32>, &Array2<f32>)>,
    cfg: &PocConfig,
) -> Vec<OpticalCandidate> {
    if b03.is_empty() || b04.is_empty() {
        warn!("Concept C: B03 or B04 missing");
        return vec![];
    }
    let factor = downsample_factor(b03.dim(), cfg.downsample_max_dim);
    let (b04d, b03d) = if factor > 1 {
        (downsample(b04, factor), downsample(b03, factor))
    } else {
        (b04.clone(), b03.clone())
    };
    if b03d.dim() != b04d.dim() {
        warn!("Concept C: B03 shape != B04 shape");
        return vec![];
    }

    let ndti_of = |red: &Array2<f32>, green: &Array2<f32>| -> Array2<f32> {
        let (rows, cols) = red.dim();
        let mut out = Array2::<f32>::from_elem((rows, cols), f32::NAN);
        ndarray::Zip::from(&mut out).and(red).and(green).for_each(|o, &r, &g| {
            let r = r as f64;
            let g = g as f64;
            let denom = r + g;
            if denom > 0.005 && r < 0.3 && r.is_finite() && g.is_finite() {
                *o = ((r - g) / denom) as f32;
            }
        });
        out
    };

    let mut ndti = ndti_of(&b04d, &b03d);

    // Optional delta-NDTI vs a clear-water baseline.
    if let Some((b04_base, b03_base)) = baseline {
        let mut b04b = b04_base.clone();
        let mut b03b = b03_base.clone();
        if factor > 1 && b03b.dim() != b03d.dim() {
            let base_factor = downsample_factor(b03b.dim(), cfg.downsample_max_dim);
            b04b = downsample(&b04b, base_factor);
            b03b = downsample(&b03b, base_factor);
        }
        if b03b.dim() != b03d.dim() {
            warn!(
                "Concept C: storm shape {:?} != baseline shape {:?} — skipping delta-NDTI",
                b03d.dim(),
                b03b.dim()
            );
        } else {
            // baseline NDTI uses (denom_b > 0.005) & finite (no b04<0.3 clamp in Python)
            let (rows, cols) = b04b.dim();
            let mut ndti_base = Array2::<f32>::from_elem((rows, cols), f32::NAN);
            ndarray::Zip::from(&mut ndti_base).and(&b04b).and(&b03b).for_each(|o, &r, &g| {
                let r = r as f64;
                let g = g as f64;
                let denom = r + g;
                if denom > 0.005 && r.is_finite() && g.is_finite() {
                    *o = ((r - g) / denom) as f32;
                }
            });
            // delta = where finite(ndti) & finite(ndti_base) → ndti - ndti_base else NaN
            let mut delta = Array2::<f32>::from_elem(ndti.raw_dim(), f32::NAN);
            ndarray::Zip::from(&mut delta).and(&ndti).and(&ndti_base).for_each(|d, &a, &b| {
                if a.is_finite() && b.is_finite() {
                    *d = a - b;
                }
            });
            ndti = delta;
            info!("Concept C: using delta-NDTI (post-storm minus baseline)");
        }
    } else {
        info!("Concept C: using absolute NDTI (no baseline provided)");
    }

    let (rows, cols) = ndti.dim();
    let (lat_grid, lon_grid) = pixel_coords(rows, cols, bbox);
    let peaks = find_peak_clusters(
        &ndti,
        &lat_grid,
        &lon_grid,
        cfg.zscore_threshold,
        cfg.min_sep(10),
        cfg.max_candidates,
    );

    peaks
        .into_iter()
        .map(|(lat, lon, metric, zsc)| {
            let score = (zsc / 5.0 * 10.0).min(10.0);
            let wreck_score = ((score * 0.8).round() as i64).clamp(0, 10);
            OpticalCandidate {
                lat,
                lon,
                concept: "sediment_plume".into(),
                score: round2(score),
                wreck_score,
                scene_date: scene_date.into(),
                metric: round_n(metric, 6),
                metric_zscore: round_n(zsc, 3),
                note: "NDTI turbidity anomaly — possible post-storm sediment plume anchor".into(),
                known_wreck_nearby: false,
                nearest_known_m: 99999.0,
            }
        })
        .collect()
}

fn round2(v: f64) -> f64 {
    round_n(v, 2)
}
fn round_n(v: f64, n: i32) -> f64 {
    let p = 10f64.powi(n);
    (v * p).round() / p
}

// ── Full-scene runner (replaces the python3 subprocess) ───────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PocOutcome {
    pub n_scenes: usize,
    pub clear_date: String,
    pub storm_date: String,
    pub candidates: Vec<OpticalCandidate>,
}

/// Fetch the lowest-cloud scene in `[start, end]` and load B02/B03/B04/B08 chips
/// resampled to `target_px`.  Returns `(band_map, scene_date)`.
///
/// Mirrors `_fetch_best_scene` (the Rust path fetches the AOI as one resampled
/// window per band rather than a CRS-aware rasterio window — see module docs).
async fn fetch_best_scene(
    client: &Client,
    bbox: &BBox,
    start: NaiveDate,
    end: NaiveDate,
    max_cloud: f64,
    target_px: usize,
    chip_cache_dir: &std::path::Path,
) -> (std::collections::HashMap<String, Array2<f32>>, String) {
    let mut bands: std::collections::HashMap<String, Array2<f32>> = std::collections::HashMap::new();
    let q = StacQuery {
        bbox: bbox.to_stac_array(),
        date_start: start,
        date_end: end,
        max_cloud,
        month_filter: &[],
        limit: 15,
    };
    let scenes: Vec<Scene> = match search_scenes(client, &q).await {
        Ok(s) => s,
        Err(e) => {
            warn!("POC STAC search failed: {e}");
            return (bands, "unknown".into());
        }
    };
    let scene = match scenes.first() {
        Some(s) => s,
        None => return (bands, "unknown".into()),
    };
    let scene_date = scene.datetime.to_string();
    for band in ["B02", "B03", "B04", "B08"] {
        if let Some(href) = scene.asset_href(band) {
            match download_cog_chip(client, href, bbox, target_px, chip_cache_dir).await {
                Ok(arr) => {
                    bands.insert(band.into(), arr);
                }
                Err(e) => debug!("POC band {band} download failed: {e}"),
            }
        }
    }
    (bands, scene_date)
}

/// Run the full-scene optical POC over an AOI: fetch scene(s), run all three
/// concepts, cross-reference known wrecks, and return ranked candidates.
///
/// Ports the orchestration in `wh2k_sentinel_optical_poc.py::main`.
pub async fn run_poc_aoi(
    client: &Client,
    bbox: &BBox,
    date_start: NaiveDate,
    date_end: NaiveDate,
    knobs: &Knobs,
    chip_cache_dir: &std::path::Path,
    known_wrecks: &[(f64, f64)],
) -> Result<PocOutcome> {
    let cfg = PocConfig::from_knobs(knobs);
    let target_px = cfg.downsample_max_dim;

    // Clear-water scene (B02/B03/B04/B08).
    let (clear_bands, clear_date) =
        fetch_best_scene(client, bbox, date_start, date_end, knobs.max_cloud, target_px, chip_cache_dir).await;

    // Post-storm scene for sediment plume (relaxed cloud tolerance like Python).
    let storm_start = NaiveDate::parse_from_str(&knobs.storm_date_start, "%Y-%m-%d").unwrap_or(date_start);
    let storm_end = NaiveDate::parse_from_str(&knobs.storm_date_end, "%Y-%m-%d").unwrap_or(date_end);
    let (storm_bands, storm_date) =
        fetch_best_scene(client, bbox, storm_start, storm_end, 40.0, target_px, chip_cache_dir).await;

    let mut all: Vec<OpticalCandidate> = Vec::new();

    if let Some(b08) = clear_bands.get("B08") {
        all.extend(concept_shadow_roughness(b08, bbox, &clear_date, &cfg));
    }
    if let (Some(b02), Some(b04)) = (clear_bands.get("B02"), clear_bands.get("B04")) {
        all.extend(concept_zebra_clarity(b02, b04, bbox, &clear_date, &cfg));
    }
    // sediment_plume: prefer storm scene as primary + clear as baseline.
    let (primary, sed_date, baseline): (&std::collections::HashMap<String, Array2<f32>>, &str, Option<(&Array2<f32>, &Array2<f32>)>) =
        if !storm_bands.is_empty() {
            let bl = match (clear_bands.get("B04"), clear_bands.get("B03")) {
                (Some(b04), Some(b03)) => Some((b04, b03)),
                _ => None,
            };
            (&storm_bands, storm_date.as_str(), bl)
        } else {
            (&clear_bands, clear_date.as_str(), None)
        };
    if let (Some(b04), Some(b03)) = (primary.get("B04"), primary.get("B03")) {
        all.extend(concept_sediment_plume(b04, b03, bbox, sed_date, baseline, &cfg));
    }

    if !known_wrecks.is_empty() {
        cross_reference(&mut all, known_wrecks, cfg.xref_nearby_radius_m);
    }

    all.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    Ok(PocOutcome {
        n_scenes: if clear_bands.is_empty() { 0 } else { 1 } + if storm_bands.is_empty() { 0 } else { 1 },
        clear_date,
        storm_date,
        candidates: all,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsample_block_average_ignores_nan() {
        // 4x4 → 2x2 with factor 2; one block has a NaN that must be ignored.
        let mut a = Array2::<f32>::zeros((4, 4));
        for r in 0..4 {
            for c in 0..4 {
                a[[r, c]] = (r * 4 + c) as f32;
            }
        }
        a[[0, 0]] = f32::NAN; // block (0,0) = mean of {1,4,5} = 3.333..
        let d = downsample(&a, 2);
        assert_eq!(d.dim(), (2, 2));
        assert!((d[[0, 0]] as f64 - (1.0 + 4.0 + 5.0) / 3.0).abs() < 1e-4);
        // block (1,1) = mean of {10,11,14,15} = 12.5
        assert!((d[[1, 1]] as f64 - 12.5).abs() < 1e-4);
    }

    #[test]
    fn masked_zscore_zero_when_too_few_finite() {
        // < 100 finite values → all zeros (matches Python early return).
        let a = Array2::<f32>::from_elem((5, 5), 3.0);
        let z = masked_zscore(&a);
        assert!(z.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn masked_zscore_centres_and_scales() {
        // 12x12 = 144 finite values; a single spike must produce a positive z.
        let mut a = Array2::<f32>::zeros((12, 12));
        for (i, v) in a.iter_mut().enumerate() {
            *v = (i % 3) as f32; // some deterministic spread
        }
        a[[6, 6]] = 50.0;
        let z = masked_zscore(&a);
        assert!(z[[6, 6]] as f64 > 3.0, "spike should be a strong positive z");
    }

    #[test]
    fn uniform_filter_constant_is_constant() {
        let a = Array2::<f32>::from_elem((10, 10), 2.5);
        let f = uniform_filter(&a, 5);
        for v in f.iter() {
            assert!((*v as f64 - 2.5).abs() < 1e-5);
        }
    }

    #[test]
    fn maximum_filter_picks_local_max() {
        let mut a = Array2::<f32>::zeros((7, 7));
        a[[3, 3]] = 9.0;
        let m = maximum_filter(&a, 3);
        // The 3x3 neighbourhood around (3,3) should now all be 9.
        assert_eq!(m[[2, 2]], 9.0);
        assert_eq!(m[[3, 3]], 9.0);
        assert_eq!(m[[4, 4]], 9.0);
        // A pixel two cells away (outside the window) stays 0.
        assert_eq!(m[[0, 0]], 0.0);
    }

    #[test]
    fn find_peak_clusters_finds_single_spike() {
        // Build a 20x20 field with one strong, isolated spike. NMS should return
        // exactly that pixel as the top candidate.
        let mut field = Array2::<f32>::zeros((20, 20));
        for (i, v) in field.iter_mut().enumerate() {
            *v = ((i * 7) % 5) as f32 * 0.01; // tiny deterministic spread
        }
        field[[10, 12]] = 100.0;
        let bbox = BBox { lat_min: 41.0, lon_min: -83.0, lat_max: 42.0, lon_max: -82.0 };
        let (lat_g, lon_g) = pixel_coords(20, 20, &bbox);
        let peaks = find_peak_clusters(&field, &lat_g, &lon_g, 2.5, 8, 25);
        assert!(!peaks.is_empty(), "should find the spike");
        let top = peaks[0];
        // lat_grid row 10 of 20 over [42 (north) .. 41 (south)]
        let expected_lat = lat_g[[10, 12]];
        let expected_lon = lon_g[[10, 12]];
        assert!((top.0 - expected_lat).abs() < 1e-9);
        assert!((top.1 - expected_lon).abs() < 1e-9);
        assert!(top.3 > 2.5, "spike z above threshold");
    }

    #[test]
    fn blue_green_z_cap_limits_score() {
        let (score, ws) = blue_green_score_from_z(99.0);
        assert!(score <= 10.0 && score > 9.0);
        assert!(ws <= 10);
        let (score2, _) = blue_green_score_from_z(2.0);
        assert!(score2 < score);
    }

    #[test]
    fn find_peak_clusters_respects_threshold() {
        // Flat-ish field with no pixel above threshold → no peaks.
        let mut field = Array2::<f32>::zeros((15, 15));
        for (i, v) in field.iter_mut().enumerate() {
            *v = (i % 2) as f32 * 0.001;
        }
        let bbox = BBox { lat_min: 41.0, lon_min: -83.0, lat_max: 42.0, lon_max: -82.0 };
        let (lat_g, lon_g) = pixel_coords(15, 15, &bbox);
        let peaks = find_peak_clusters(&field, &lat_g, &lon_g, 5.0, 8, 25);
        assert!(peaks.is_empty(), "nothing should clear a 5σ threshold here");
    }

    #[test]
    fn cross_reference_flags_nearby() {
        let mut cands = vec![OpticalCandidate {
            lat: 41.5,
            lon: -82.5,
            concept: "shadow_roughness".into(),
            score: 5.0,
            wreck_score: 4,
            scene_date: "2024-01-01".into(),
            metric: 0.1,
            metric_zscore: 2.5,
            note: String::new(),
            known_wreck_nearby: false,
            nearest_known_m: 99999.0,
        }];
        // A wreck ~100 m away and another far away.
        let wrecks = vec![(41.5009, -82.5), (40.0, -80.0)];
        cross_reference(&mut cands, &wrecks, 2000.0);
        assert!(cands[0].known_wreck_nearby);
        assert!(cands[0].nearest_known_m < 200.0);
    }

    #[test]
    fn pixel_coords_orientation() {
        let bbox = BBox { lat_min: 41.0, lon_min: -83.0, lat_max: 42.0, lon_max: -82.0 };
        let (lat_g, lon_g) = pixel_coords(3, 3, &bbox);
        // Row 0 = north (lat_max), last row = south (lat_min).
        assert!((lat_g[[0, 0]] - 42.0).abs() < 1e-9);
        assert!((lat_g[[2, 0]] - 41.0).abs() < 1e-9);
        // Col 0 = west (lon_min), last col = east (lon_max).
        assert!((lon_g[[0, 0]] - (-83.0)).abs() < 1e-9);
        assert!((lon_g[[0, 2]] - (-82.0)).abs() < 1e-9);
    }
}


// ── Offline local-tile POC (no STAC, reads on-disk Sentinel-2) ────────────────

use rayon::prelude::*;

/// A scene's loaded bands for local processing.
struct LocalScene {
    date: String,
    b02: Array2<f32>, // blue
    b03: Array2<f32>, // green
}

/// Blue-green clarity concept (B02/B03) — replacement for zebra_clarity(B02/B04)
/// when red band doesn't penetrate to target depth.
/// Secchi-like: clarity = log(B02) / log(B03). Areas with column disturbance
/// (plume, turbidity from wreck-driven current) show lower clarity ratio.
pub fn concept_blue_green_clarity(
    b02: &Array2<f32>,
    b03: &Array2<f32>,
    bbox: &BBox,
    scene_date: &str,
    cfg: &PocConfig,
) -> Vec<OpticalCandidate> {
    if b02.is_empty() || b03.is_empty() {
        return vec![];
    }
    let factor = downsample_factor(b02.dim(), cfg.downsample_max_dim);
    let (b02, b03) = if factor > 1 {
        (downsample(b02, factor), downsample(b03, factor))
    } else {
        (b02.clone(), b03.clone())
    };
    if b02.dim() != b03.dim() {
        return vec![];
    }
    let (rows, cols) = b02.dim();

    // Clarity index: log(B02)/log(B03) — ratio of blue to green log-reflectance
    // High clarity water → high ratio; wreck-disturbed column → lower ratio (anomaly)
    let mut clarity = Array2::<f32>::from_elem((rows, cols), f32::NAN);
    for r in 0..rows {
        for c in 0..cols {
            let blue = b02[[r, c]];
            let green = b03[[r, c]];
            if blue > 0.001 && green > 0.001 && blue.is_finite() && green.is_finite() {
                clarity[[r, c]] = blue.ln() / green.ln();
            }
        }
    }

    mask_edge_band(&mut clarity, 3);

    // Z-score: negative z = lower clarity than surroundings = anomaly
    let z = masked_zscore(&clarity);
    // Invert + cap |z| before NMS (OpenMemory Z_max = 4.0)
    let mut neg_z = z.mapv(|v| {
        if !v.is_finite() {
            return f32::NAN;
        }
        let nz = -v;
        nz.clamp(-(BLUE_GREEN_Z_CAP as f32), BLUE_GREEN_Z_CAP as f32)
    });

    let min_sep = cfg.min_sep(15);
    let (lat_grid, lon_grid) = pixel_coords(rows, cols, bbox);
    let z_thresh = cfg.zscore_threshold.min(BLUE_GREEN_Z_CAP);
    let peaks = find_peak_clusters(
        &neg_z,
        &lat_grid,
        &lon_grid,
        z_thresh,
        min_sep,
        cfg.max_candidates,
    );

    peaks
        .into_iter()
        .map(|(lat, lon, metric, zsc)| {
            let (score, wreck_score) = blue_green_score_from_z(zsc);
            OpticalCandidate {
                concept: "blue_green_clarity".into(),
                lat,
                lon,
                score,
                wreck_score,
                scene_date: scene_date.into(),
                metric: round_n(metric, 6),
                metric_zscore: round_n(zsc, 3),
                known_wreck_nearby: false,
                nearest_known_m: default_nearest_known_m(),
                note: "blue-green clarity (z-capped)".into(),
            }
        })
        .collect()
}

/// Glint / surface-roughness concept (FLEET TOOL 5): Sobel on local B02/B03 variance.
pub fn concept_glint_roughness(
    b02: &Array2<f32>,
    b03: &Array2<f32>,
    bbox: &BBox,
    scene_date: &str,
    cfg: &PocConfig,
) -> Vec<OpticalCandidate> {
    if b02.is_empty() || b03.is_empty() {
        return vec![];
    }
    let factor = downsample_factor(b02.dim(), cfg.downsample_max_dim);
    let (b02, b03) = if factor > 1 {
        (downsample(b02, factor), downsample(b03, factor))
    } else {
        (b02.clone(), b03.clone())
    };
    if b02.dim() != b03.dim() {
        return vec![];
    }
    let (rows, cols) = b02.dim();
    let mut bright = Array2::<f32>::from_elem((rows, cols), f32::NAN);
    for r in 0..rows {
        for c in 0..cols {
            let blue = b02[[r, c]];
            let green = b03[[r, c]];
            if blue > 0.001 && green > 0.001 && blue.is_finite() && green.is_finite() {
                bright[[r, c]] = (blue + green) * 0.5;
            }
        }
    }
    mask_edge_band(&mut bright, 3);
    let win = 5usize;
    let mean = uniform_filter(&bright, win);
    let mean_sq = uniform_filter(&bright.mapv(|v| if v.is_finite() { v * v } else { f32::NAN }), win);
    let mut variance = Array2::<f32>::from_elem((rows, cols), f32::NAN);
    for r in 0..rows {
        for c in 0..cols {
            let m = mean[[r, c]];
            let ms = mean_sq[[r, c]];
            if m.is_finite() && ms.is_finite() {
                let v = ms - m * m;
                if v.is_finite() && v > 0.0 {
                    variance[[r, c]] = v;
                }
            }
        }
    }
    let z_var = masked_zscore(&variance);
    let mut grad = sobel_magnitude_raw(&z_var);
    mask_edge_band(&mut grad, 3);
    let z_grad = masked_zscore(&grad);
    let score_map = z_grad.mapv(|v| {
        if !v.is_finite() {
            return f32::NAN;
        }
        v.clamp(-(BLUE_GREEN_Z_CAP as f32), BLUE_GREEN_Z_CAP as f32)
    });
    let min_sep = cfg.min_sep(15);
    let (lat_grid, lon_grid) = pixel_coords(rows, cols, bbox);
    let z_thresh = cfg.zscore_threshold.min(BLUE_GREEN_Z_CAP);
    let peaks = find_peak_clusters(
        &score_map,
        &lat_grid,
        &lon_grid,
        z_thresh,
        min_sep,
        cfg.max_candidates,
    );
    peaks
        .into_iter()
        .map(|(lat, lon, metric, zsc)| {
            let (score, wreck_score) = blue_green_score_from_z(zsc);
            OpticalCandidate {
                concept: "glint_roughness".into(),
                lat,
                lon,
                score,
                wreck_score,
                scene_date: scene_date.into(),
                metric: round_n(metric, 6),
                metric_zscore: round_n(zsc, 3),
                known_wreck_nearby: false,
                nearest_known_m: default_nearest_known_m(),
                note: "Sobel(local variance) glint transition".into(),
            }
        })
        .collect()
}

/// Run the optical POC on LOCAL (already-downloaded) Sentinel-2 tiles.
/// Uses rayon for scene-level parallelism. No network access.
///
/// `scene_dir`: directory containing `<scene_id>.<band>.tif` files.
/// Returns candidates from all scenes, fused and ranked.
pub fn run_poc_aoi_local(
    scene_dir: &std::path::Path,
    bbox: &BBox,
    knobs: &crate::types::Knobs,
    known_wrecks: &[(f64, f64)],
    target_px: usize,
) -> anyhow::Result<PocOutcome> {
    use std::collections::HashMap;

    let cfg = PocConfig::from_knobs(knobs);

    // Discover scenes by globbing for *.blue.tif
    let mut scene_ids: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Ok(entries) = std::fs::read_dir(scene_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".blue.tif") {
                let id = name.trim_end_matches(".blue.tif").to_string();
                if seen.insert(id.clone()) {
                    scene_ids.push(id);
                }
            }
        }
    }
    scene_ids.sort();
    let n_scenes = scene_ids.len();
    tracing::info!("Local POC: found {n_scenes} scenes in {}", scene_dir.display());

    if n_scenes == 0 {
        return Ok(PocOutcome {
            n_scenes: 0,
            clear_date: "none".into(),
            storm_date: "none".into(),
            candidates: vec![],
        });
    }

    // Load scenes in parallel with rayon (each opens its own GDAL dataset)
    #[cfg(feature = "gdal")]
    let scenes: Vec<Option<LocalScene>> = scene_ids
        .par_iter()
        .map(|id| {
            let blue_path = scene_dir.join(format!("{id}.blue.tif"));
            let green_path = scene_dir.join(format!("{id}.green.tif"));
            let b02 = crate::chip::decode_local_band(&blue_path, bbox, target_px).ok()?;
            let b03 = crate::chip::decode_local_band(&green_path, bbox, target_px).ok()?;
            // Extract date from scene ID (e.g. S2B_16TFR_20240913_0_L2A → 20240913)
            let date = id.split('_').nth(2).unwrap_or("unknown").to_string();
            Some(LocalScene { date, b02, b03 })
        })
        .collect();

    #[cfg(not(feature = "gdal"))]
    let scenes: Vec<Option<LocalScene>> = vec![];

    let loaded: Vec<LocalScene> = scenes.into_iter().flatten().collect();
    tracing::info!("Local POC: loaded {} of {} scenes", loaded.len(), n_scenes);

    // Run blue-green clarity + glint roughness on each scene (parallel per scene)
    let mut all_candidates: Vec<OpticalCandidate> = loaded
        .par_iter()
        .flat_map(|scene| {
            let mut c = concept_blue_green_clarity(&scene.b02, &scene.b03, bbox, &scene.date, &cfg);
            c.extend(concept_glint_roughness(
                &scene.b02,
                &scene.b03,
                bbox,
                &scene.date,
                &cfg,
            ));
            c
        })
        .collect();

    // Cross-reference against known wrecks
    if !known_wrecks.is_empty() {
        cross_reference(&mut all_candidates, known_wrecks, cfg.xref_nearby_radius_m);
    }

    all_candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    let clear_date = loaded.first().map(|s| s.date.clone()).unwrap_or_default();
    let storm_date = loaded.last().map(|s| s.date.clone()).unwrap_or_default();

    Ok(PocOutcome {
        n_scenes: loaded.len(),
        clear_date,
        storm_date,
        candidates: all_candidates,
    })
}
