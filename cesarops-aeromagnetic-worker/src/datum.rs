//! Datum correction — ports `pipelines/mag/datum_correction.py`.
//!
//! Two-stage correction for Great Lakes aeromag candidate coordinates:
//!   1. NAD27 → WGS84 via Abridged Molodensky (CONUS parameters, always applied)
//!   2. Loran-C spatial bias via triangulated rubber-sheeting (IDW) against
//!      verified anchors (only applied when >= n_nearest verified anchors are
//!      within `MAX_ANCHOR_DIST_KM`).
//!
//! Anchor policy (STRICT): only anchors with `verified == true` AND a computed
//! `survey_pos` contribute to rubber-sheeting (ports the same guard in
//! `datum_correction.py::rubber_sheet_shift`).
//!
//! References (from datum_correction.py):
//!   - Abridged Molodensky: Deakin (2004), RMIT University
//!   - CONUS NAD27→WGS84 parameters: NIMA TR8350.2 (3rd ed. 2000), Table 3.3

use serde::{Deserialize, Serialize};
use std::path::Path;

// ── Tunable knobs / datum constants ─────────────────────────────────────────
//
// Ports the `_MOLODENSKY` table and module-level constants from
// datum_correction.py. Grouped here so a mission can override them.

/// Abridged Molodensky parameters (NAD27 Clarke 1866 → WGS84/GRS80, CONUS).
/// Ports `_MOLODENSKY` in datum_correction.py.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MolodenskyParams {
    /// Semi-major axis of the source ellipsoid (Clarke 1866), metres.
    pub a_from: f64,
    /// Flattening of the source ellipsoid (Clarke 1866).
    pub f_from: f64,
    /// Semi-major axis of the target ellipsoid (WGS84), metres.
    pub a_to: f64,
    /// Flattening of the target ellipsoid (WGS84).
    pub f_to: f64,
    /// CONUS 3-parameter shift, metres.
    pub dx: f64,
    pub dy: f64,
    pub dz: f64,
}

impl Default for MolodenskyParams {
    fn default() -> Self {
        // NIMA TR8350.2 Table 3.3 "Continental United States — NAD 27".
        Self {
            a_from: 6_378_206.4,
            f_from: 1.0 / 294.978_698_2,
            a_to: 6_378_137.0,
            f_to: 1.0 / 298.257_223_563,
            dx: -8.0,
            dy: 160.0,
            dz: 176.0,
        }
    }
}

/// Rubber-sheet (IDW triangulation) knobs. Ports `MAX_ANCHOR_DIST_KM`,
/// `n_nearest`, and `power` from datum_correction.py::rubber_sheet_shift.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RubberSheetKnobs {
    /// Ignore anchors farther than this from the candidate (km).
    pub max_anchor_dist_km: f64,
    /// Number of nearest anchors used for the IDW blend.
    pub n_nearest: usize,
    /// IDW power exponent.
    pub power: f64,
}

impl Default for RubberSheetKnobs {
    fn default() -> Self {
        Self {
            max_anchor_dist_km: 250.0, // MAX_ANCHOR_DIST_KM
            n_nearest: 3,
            power: 2.0,
        }
    }
}

/// Full datum-correction configuration (knobs bundle).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatumConfig {
    pub molodensky: MolodenskyParams,
    pub rubber_sheet: RubberSheetKnobs,
}

// ── Anchor library ──────────────────────────────────────────────────────────

/// A datum control anchor. Ports the anchor dict schema in datum_correction.py.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anchor {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub lake: String,
    /// Verified GPS position [lat, lon] in WGS84.
    pub verified_gps: [f64; 2],
    /// Aeromag survey peak position [lat, lon]; `None` until characterised.
    #[serde(default)]
    pub survey_pos: Option<[f64; 2]>,
    /// Only verified anchors contribute to rubber-sheeting.
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub source: String,
}

/// Load the anchor library from JSON. Returns an empty list when the file does
/// not exist (ports the `load_anchors` fallback, but with empty defaults — see
/// the task note: no bundled anchor JSON ships with the worker yet).
pub fn load_anchors(path: &Path) -> Vec<Anchor> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        // TODO: no built-in default anchor library is bundled with the Rust
        // worker (datum_correction.py ships `_DEFAULT_ANCHORS`). Until an
        // anchor JSON is provided, rubber-sheeting is a no-op and only the
        // Molodensky shift applies. Pass anchors explicitly to enable it.
        Err(_) => Vec::new(),
    }
}

/// Save the anchor library to JSON. Ports `save_anchors`.
pub fn save_anchors(anchors: &[Anchor], path: &Path) -> Result<(), String> {
    let text = serde_json::to_string_pretty(anchors).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

// ── NAD27 → WGS84 Abridged Molodensky ───────────────────────────────────────

/// Convert NAD27 (lat, lon) to WGS84 using Abridged Molodensky.
///
/// Ports `nad27_to_wgs84` from datum_correction.py. `h` is ellipsoidal height
/// in metres (default 0 — lake surface). Returns `(lat_wgs84, lon_wgs84)`.
pub fn nad27_to_wgs84(lat_deg: f64, lon_deg: f64, h: f64, p: &MolodenskyParams) -> (f64, f64) {
    let a = p.a_from;
    let f = p.f_from;
    let da = p.a_to - a;
    let df = p.f_to - f;
    let (dx, dy, dz) = (p.dx, p.dy, p.dz);

    let phi = lat_deg.to_radians();
    let lam = lon_deg.to_radians();
    let sinphi = phi.sin();
    let cosphi = phi.cos();
    let sinlam = lam.sin();
    let coslam = lam.cos();

    let e2 = 2.0 * f - f * f;
    let n = a / (1.0 - e2 * sinphi * sinphi).sqrt();
    let m = a * (1.0 - e2) / (1.0 - e2 * sinphi * sinphi).powf(1.5);
    let b = a * (1.0 - f);

    let dphi = (-dx * sinphi * coslam - dy * sinphi * sinlam
        + dz * cosphi
        + da * (n * e2 * sinphi * cosphi) / a
        + df * (m / b + n * b / a) * sinphi * cosphi)
        / (m + h);

    let dlam = (-dx * sinlam + dy * coslam) / ((n + h) * cosphi);

    (lat_deg + dphi.to_degrees(), lon_deg + dlam.to_degrees())
}

// ── Distance helper ─────────────────────────────────────────────────────────

/// Great-circle distance in km. Ports `_haversine_km` from datum_correction.py.
pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0;
    let a = (((lat2 - lat1) / 2.0).to_radians().sin()).powi(2)
        + lat1.to_radians().cos()
            * lat2.to_radians().cos()
            * (((lon2 - lon1) / 2.0).to_radians().sin()).powi(2);
    r * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
}

/// Compute the shift vector `(delta_lat, delta_lon)` = GPS − survey_pos.
/// Ports `build_shift_vector`. Returns `None` if `survey_pos` is missing.
fn build_shift_vector(anchor: &Anchor) -> Option<(f64, f64)> {
    let srv = anchor.survey_pos?;
    Some((
        anchor.verified_gps[0] - srv[0],
        anchor.verified_gps[1] - srv[1],
    ))
}

// ── Rubber-sheeting (IDW triangulation) ──────────────────────────────────────

/// Diagnostic metadata returned by the rubber-sheet step.
#[derive(Debug, Clone, Serialize)]
pub struct RubberSheetMeta {
    pub method: String,
    pub n_used: usize,
    pub anchors: Vec<String>,
    pub distances_km: Vec<f64>,
    pub weights: Vec<f64>,
}

/// Compute the IDW-interpolated `(delta_lat, delta_lon, meta)` for a candidate.
///
/// Ports `rubber_sheet_shift` from datum_correction.py. Only anchors with
/// `verified == true` AND a `survey_pos` within `max_anchor_dist_km` are used;
/// if fewer than `n_nearest` are usable the shift is `(0, 0)` (no-op).
pub fn rubber_sheet_shift(
    lat: f64,
    lon: f64,
    anchors: &[Anchor],
    knobs: &RubberSheetKnobs,
) -> (f64, f64, RubberSheetMeta) {
    // (distance_km, (dlat, dlon), id)
    let mut usable: Vec<(f64, (f64, f64), String)> = Vec::new();
    for a in anchors {
        if !a.verified {
            continue;
        }
        if a.survey_pos.is_none() {
            continue;
        }
        let shift = match build_shift_vector(a) {
            Some(s) => s,
            None => continue,
        };
        let d = haversine_km(lat, lon, a.verified_gps[0], a.verified_gps[1]);
        if d > knobs.max_anchor_dist_km {
            continue;
        }
        usable.push((d, shift, a.id.clone()));
    }

    if usable.len() < knobs.n_nearest {
        return (
            0.0,
            0.0,
            RubberSheetMeta {
                method: "no_anchors".to_string(),
                n_used: usable.len(),
                anchors: Vec::new(),
                distances_km: Vec::new(),
                weights: Vec::new(),
            },
        );
    }

    // Sort by distance, take the nearest n_nearest.
    usable.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let nearest = &usable[..knobs.n_nearest];

    // IDW — guard against zero distance (anchor IS the candidate).
    let weights: Vec<f64> = nearest
        .iter()
        .map(|(d, _, _)| {
            if *d > 1e-9 {
                1.0 / d.powf(knobs.power)
            } else {
                1e9
            }
        })
        .collect();

    let total_w: f64 = weights.iter().sum();
    let dlat = nearest
        .iter()
        .zip(&weights)
        .map(|((_, s, _), w)| w * s.0)
        .sum::<f64>()
        / total_w;
    let dlon = nearest
        .iter()
        .zip(&weights)
        .map(|((_, s, _), w)| w * s.1)
        .sum::<f64>()
        / total_w;

    let meta = RubberSheetMeta {
        method: "rubber_sheet_idw".to_string(),
        n_used: nearest.len(),
        anchors: nearest.iter().map(|(_, _, id)| id.clone()).collect(),
        distances_km: nearest.iter().map(|(d, _, _)| (d * 100.0).round() / 100.0).collect(),
        weights: weights.iter().map(|w| ((w / total_w) * 1e4).round() / 1e4).collect(),
    };
    (dlat, dlon, meta)
}

// ── Full correction pipeline ──────────────────────────────────────────────────

/// Input datum of the raw candidate coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Datum {
    Nad27,
    Wgs84,
}

/// Result of correcting a single candidate point.
#[derive(Debug, Clone, Serialize)]
pub struct CorrectionResult {
    pub raw_lat: f64,
    pub raw_lon: f64,
    pub molodensky_lat: f64,
    pub molodensky_lon: f64,
    pub corrected_lat: f64,
    pub corrected_lon: f64,
    pub total_shift_m: f64,
    pub method: String,
    pub anchors_used: usize,
}

/// Apply the full datum correction (Molodensky → rubber-sheet) to one point.
/// Ports `correct_candidate` from datum_correction.py.
pub fn correct_candidate(
    raw_lat: f64,
    raw_lon: f64,
    anchors: &[Anchor],
    datum: Datum,
    config: &DatumConfig,
) -> CorrectionResult {
    // 1. Molodensky NAD27→WGS84 (skipped if already WGS84).
    let (mol_lat, mol_lon) = match datum {
        Datum::Nad27 => nad27_to_wgs84(raw_lat, raw_lon, 0.0, &config.molodensky),
        Datum::Wgs84 => (raw_lat, raw_lon),
    };

    // 2. Rubber-sheet Loran-C bias correction from verified anchor shifts.
    let (rs_dlat, rs_dlon, meta) =
        rubber_sheet_shift(mol_lat, mol_lon, anchors, &config.rubber_sheet);
    let final_lat = mol_lat + rs_dlat;
    let final_lon = mol_lon + rs_dlon;

    let total_shift_m = haversine_km(raw_lat, raw_lon, final_lat, final_lon) * 1000.0;

    CorrectionResult {
        raw_lat,
        raw_lon,
        molodensky_lat: mol_lat,
        molodensky_lon: mol_lon,
        corrected_lat: final_lat,
        corrected_lon: final_lon,
        total_shift_m,
        method: meta.method,
        anchors_used: meta.n_used,
    }
}

/// A candidate coordinate pair the pipeline can hand to the corrector.
pub trait HasCoords {
    fn coords(&self) -> (f64, f64);
    fn set_corrected(&mut self, lat: f64, lon: f64, shift_m: f64, method: &str, anchors_used: usize);
}

/// Batch-correct a list of coordinate-bearing candidates in place.
/// Ports `batch_correct` from datum_correction.py.
pub fn batch_correct<T: HasCoords>(
    candidates: &mut [T],
    anchors: &[Anchor],
    datum: Datum,
    config: &DatumConfig,
) {
    for c in candidates.iter_mut() {
        let (lat, lon) = c.coords();
        let r = correct_candidate(lat, lon, anchors, datum, config);
        c.set_corrected(
            r.corrected_lat,
            r.corrected_lon,
            r.total_shift_m,
            &r.method,
            r.anchors_used,
        );
    }
}

/// Convenience entry point for the pipeline: correct a single `(lat, lon)` and
/// return the corrected `(lat, lon)`. Uses default config and the supplied
/// anchors (pass an empty slice to apply Molodensky only).
pub fn correct_coords(
    lat: f64,
    lon: f64,
    anchors: &[Anchor],
    datum: Datum,
    config: &DatumConfig,
) -> (f64, f64) {
    let r = correct_candidate(lat, lon, anchors, datum, config);
    (r.corrected_lat, r.corrected_lon)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Molodensky on a known Lake Erie point. Values pinned to the Python
    /// source of truth `datum_correction.py::nad27_to_wgs84` for
    /// (42.1642, -80.0806): WGS84 = (42.16527048, -80.08036184), i.e. the
    /// 3-parameter CONUS shift moves the point ~+119 m north and ~+20 m east.
    /// (The module docstring's NADCON figures are an unrelated approximation;
    /// the analytic Molodensky output is authoritative and is what we port.)
    #[test]
    fn test_molodensky_known_point() {
        let p = MolodenskyParams::default();
        // Presque Isle area, Lake Erie (~42.16 N, -80.08 W).
        let (lat, lon) = nad27_to_wgs84(42.1642, -80.0806, 0.0, &p);

        // Must match the Python reference to ~1e-6 deg (sub-metre).
        assert!(
            (lat - 42.165_270_48).abs() < 1e-6,
            "WGS84 latitude must match Python reference, got {lat}"
        );
        assert!(
            (lon - (-80.080_361_84)).abs() < 1e-6,
            "WGS84 longitude must match Python reference, got {lon}"
        );

        let dlat_m = (lat - 42.1642) * 111_320.0;
        let dlon_m = (lon - (-80.0806)) * 111_320.0 * (42.1642_f64.to_radians()).cos();

        // CONUS Molodensky moves the point north and (slightly) east.
        assert!(lat > 42.1642, "WGS84 latitude must increase vs NAD27");
        assert!(lon > -80.0806, "WGS84 longitude must move east vs NAD27");
        assert!(
            (110.0..130.0).contains(&dlat_m),
            "north shift ~119 m expected, got {dlat_m:.1} m"
        );
        assert!(
            (10.0..30.0).contains(&dlon_m),
            "east shift ~20 m expected, got {dlon_m:.1} m"
        );
        // Total shift ~120 m.
        let total = haversine_km(42.1642, -80.0806, lat, lon) * 1000.0;
        assert!(
            (100.0..140.0).contains(&total),
            "total datum shift ~120 m expected, got {total:.1} m"
        );
    }

    /// Rubber-sheet IDW with synthetic anchors. Three anchors each report a
    /// uniform +0.001° lat / -0.001° lon survey offset; the IDW blend of a point
    /// near them must reproduce that uniform shift (weights sum to 1).
    #[test]
    fn test_rubber_sheet_idw_synthetic() {
        let mk = |id: &str, lat: f64, lon: f64| Anchor {
            id: id.to_string(),
            name: String::new(),
            lake: "erie".to_string(),
            verified_gps: [lat, lon],
            // survey_pos is 0.001 south / 0.001 east of GPS, so the shift
            // vector (GPS - survey) is +0.001 lat, -0.001 lon for every anchor.
            survey_pos: Some([lat - 0.001, lon + 0.001]),
            verified: true,
            source: String::new(),
        };
        let anchors = vec![
            mk("a", 42.10, -80.10),
            mk("b", 42.12, -80.08),
            mk("c", 42.08, -80.12),
        ];
        let knobs = RubberSheetKnobs::default();
        let (dlat, dlon, meta) = rubber_sheet_shift(42.10, -80.10, &anchors, &knobs);

        assert_eq!(meta.method, "rubber_sheet_idw");
        assert_eq!(meta.n_used, 3);
        // Uniform shift field → IDW must reproduce the common shift exactly.
        assert!((dlat - 0.001).abs() < 1e-9, "dlat should be +0.001, got {dlat}");
        assert!((dlon + 0.001).abs() < 1e-9, "dlon should be -0.001, got {dlon}");
        // Weights must sum to 1.
        let wsum: f64 = meta.weights.iter().sum();
        assert!((wsum - 1.0).abs() < 1e-3, "IDW weights must sum to 1, got {wsum}");
    }

    /// Too few usable anchors (or all unverified) → no-op shift.
    #[test]
    fn test_rubber_sheet_insufficient_anchors() {
        let knobs = RubberSheetKnobs::default();
        // Only two verified anchors with survey_pos → fewer than n_nearest=3.
        let anchors = vec![
            Anchor {
                id: "a".into(),
                name: String::new(),
                lake: "erie".into(),
                verified_gps: [42.10, -80.10],
                survey_pos: Some([42.10, -80.10]),
                verified: true,
                source: String::new(),
            },
            // Unverified anchor must be ignored even though it has survey_pos.
            Anchor {
                id: "b".into(),
                name: String::new(),
                lake: "erie".into(),
                verified_gps: [42.11, -80.11],
                survey_pos: Some([42.11, -80.11]),
                verified: false,
                source: String::new(),
            },
        ];
        let (dlat, dlon, meta) = rubber_sheet_shift(42.10, -80.10, &anchors, &knobs);
        assert_eq!(meta.method, "no_anchors");
        assert_eq!(dlat, 0.0);
        assert_eq!(dlon, 0.0);
    }

    /// Anchors beyond MAX_ANCHOR_DIST_KM are excluded.
    #[test]
    fn test_rubber_sheet_distance_cutoff() {
        let knobs = RubberSheetKnobs::default();
        // Three verified anchors near the equator far from the candidate point.
        let far = |id: &str, lat: f64, lon: f64| Anchor {
            id: id.to_string(),
            name: String::new(),
            lake: String::new(),
            verified_gps: [lat, lon],
            survey_pos: Some([lat - 0.001, lon]),
            verified: true,
            source: String::new(),
        };
        let anchors = vec![far("a", 0.0, 0.0), far("b", 0.1, 0.1), far("c", -0.1, 0.0)];
        // Candidate in Lake Erie, thousands of km away.
        let (_, _, meta) = rubber_sheet_shift(42.10, -80.10, &anchors, &knobs);
        assert_eq!(meta.method, "no_anchors", "distant anchors must be excluded");
    }

    /// Full pipeline: Molodensky then rubber-sheet. With no anchors only the
    /// Molodensky shift applies and the corrected point differs from raw.
    #[test]
    fn test_correct_candidate_molodensky_only() {
        let cfg = DatumConfig::default();
        let r = correct_candidate(42.1642, -80.0806, &[], Datum::Nad27, &cfg);
        assert_eq!(r.method, "no_anchors");
        assert_eq!(r.anchors_used, 0);
        assert!(r.total_shift_m > 0.0, "Molodensky must move the point");
        assert!(r.corrected_lat > r.raw_lat);
        // WGS84 input → no Molodensky, no anchors → identity.
        let r2 = correct_candidate(42.1642, -80.0806, &[], Datum::Wgs84, &cfg);
        assert!(r2.total_shift_m < 1e-6, "WGS84 + no anchors must be identity");
    }
}
