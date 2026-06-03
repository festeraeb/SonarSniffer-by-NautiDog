//! Basin-aware composite scoring + disposition false-positive filter.
//!
//! Task 2 ports `erie_scanner_pipeline.py::_apply_basin_scoring` and the
//! `ERIE_BASINS` sub-basin table: multiplicative adjustments to a candidate's
//! composite score based on wellhead/known-wreck proximity and the geology of
//! the Lake Erie sub-basin it falls in.
//!
//! Task 3 ports the raised/scrapped + `geological_false_positive` exclusion
//! from `mag_data_pipeline.py::stage_cross_reference`: a candidate that matches
//! a known wreck whose disposition means it is no longer on the bottom (raised,
//! salvaged, …) — or that is tagged as a geological false positive — is flagged
//! and down-ranked.

// ── ERIE_BASINS table (erie_scanner_pipeline.py) ────────────────────────────

/// One Lake Erie sub-basin bounding box. Ports an entry of `ERIE_BASINS`.
#[derive(Debug, Clone, Copy)]
pub struct BasinBounds {
    pub name: &'static str,
    pub lon_min: f64,
    pub lon_max: f64,
    pub lat_min: f64,
    pub lat_max: f64,
}

/// Lake Erie sub-basins, in the same order as the Python `ERIE_BASINS` dict so
/// boundary ties resolve identically (first match wins).
pub const ERIE_BASINS: [BasinBounds; 3] = [
    BasinBounds {
        name: "western",
        lon_min: -83.50,
        lon_max: -82.00,
        lat_min: 41.35,
        lat_max: 42.00,
    },
    BasinBounds {
        name: "central",
        lon_min: -82.00,
        lon_max: -80.00,
        lat_min: 41.60,
        lat_max: 42.60,
    },
    BasinBounds {
        name: "eastern",
        lon_min: -80.00,
        lon_max: -78.80,
        lat_min: 42.00,
        lat_max: 42.90,
    },
];

/// Western-basin low-amplitude threshold (nT). Ports the `< 100` test in
/// `_apply_basin_scoring`.
pub const WESTERN_LOW_AMP_NT: f64 = 100.0;

/// Regional Precambrian basement strike (NE–SW). Ports `REGIONAL_STRIKE_DEG` in
/// `geo_filter_candidates.py`.
pub const REGIONAL_STRIKE_DEG: f64 = 45.0;

/// Angular deviation (0–90°) of dipole long-axis from regional strike.
pub fn strike_deviation_deg(azimuth_deg: f64) -> f64 {
    let diff = ((azimuth_deg % 180.0) - (REGIONAL_STRIKE_DEG % 180.0)).abs();
    diff.min(180.0 - diff)
}

/// Multiplicative strike adjustment + reason. Ports `_extra_score` off-axis block
/// in `geo_filter_candidates.py` (+10 when >60° off strike, −8 when <20° aligned).
pub fn apply_strike_scoring(score: f64, elongation_azimuth_deg: Option<f64>) -> (f64, Option<String>) {
    let Some(az) = elongation_azimuth_deg else {
        return (score, None);
    };
    let dev = strike_deviation_deg(az);
    if dev > 60.0 {
        let bonus = 1.0 + 10.0 / 100.0; // +10 on 0–100 man-made scale → ~10% composite bump
        (
            score * bonus,
            Some(format!(
                "+10% dipole axis {az:.0}° is {dev:.0}° off regional NE-SW geology (anomalous orientation)"
            )),
        )
    } else if dev < 20.0 {
        (
            score * 0.92,
            Some(format!(
                "-8% dipole axis {az:.0}° aligns with regional NE-SW strike (geological-consistent)"
            )),
        )
    } else {
        (score, None)
    }
}

/// Identify the Lake Erie sub-basin a point falls in (first match wins, as in
/// the Python dict iteration order). Returns `None` outside the defined basins.
pub fn identify_basin(lat: f64, lon: f64) -> Option<&'static str> {
    for b in ERIE_BASINS.iter() {
        if b.lon_min <= lon && lon <= b.lon_max && b.lat_min <= lat && lat <= b.lat_max {
            return Some(b.name);
        }
    }
    None
}

/// Inputs to basin scoring. Mirrors the `CandidateMatch` fields read by
/// `_apply_basin_scoring`.
pub struct BasinScoringInput {
    pub center_lat: f64,
    pub center_lon: f64,
    /// Distance to the nearest wellhead (m), if any.
    pub wellhead_distance_m: Option<f64>,
    /// Distance to the nearest known wreck (m), if any.
    pub wreck_distance_m: Option<f64>,
    /// Whether the candidate has a dipolar signature (Wave 1 classification).
    pub is_dipolar: bool,
    /// Peak absolute anomaly amplitude (nT) for the western low-amp test.
    pub amplitude_peak_abs: f64,
    /// Name of the nearest known wreck (for the reason string).
    pub nearest_known_wreck: Option<String>,
    /// Long-axis azimuth (0–180°) from CPU dipole PCA when available.
    pub elongation_azimuth_deg: Option<f64>,
}

/// Apply Lake Erie basin-specific multiplicative score adjustments.
///
/// Ports `erie_scanner_pipeline.py::_apply_basin_scoring`. Returns the adjusted
/// score plus a list of human-readable reason strings (matching the Python
/// `all_reasons` appends). If the point is outside every basin the score is
/// returned unchanged (the Python early `return`).
pub fn apply_basin_scoring(composite_score: f64, input: &BasinScoringInput) -> (f64, Vec<String>) {
    let mut score = composite_score;
    let mut reasons: Vec<String> = Vec::new();

    let basin = match identify_basin(input.center_lat, input.center_lon) {
        Some(b) => b,
        None => return (score, reasons), // outside defined basins
    };

    // ── Wellhead proximity penalty ──
    if let Some(d) = input.wellhead_distance_m {
        if d < 500.0 {
            score *= 0.3;
            reasons.push(format!("-70% wellhead within {d:.0}m"));
        } else if d < 1000.0 {
            score *= 0.6;
            reasons.push(format!("-40% wellhead within {d:.0}m"));
        } else if d < 2000.0 {
            score *= 0.8;
            reasons.push(format!("-20% wellhead within {d:.0}m"));
        }
    }

    // ── Known wreck proximity bonus ──
    if let Some(d) = input.wreck_distance_m {
        let name = input
            .nearest_known_wreck
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        if d < 1000.0 {
            score *= 1.5;
            reasons.push(format!("+50% known wreck '{name}' within {d:.0}m"));
        } else if d < 3000.0 {
            score *= 1.2;
            reasons.push(format!("+20% near wreck '{name}' within {d:.0}m"));
        }
    }

    // ── Basin-specific adjustments ──
    match basin {
        "western" => {
            // Shallow, mineral deposits & shore infrastructure → raise the bar.
            if input.amplitude_peak_abs < WESTERN_LOW_AMP_NT {
                score *= 0.9;
                reasons.push("-10% western basin low amplitude (< 100 nT)".to_string());
            }
        }
        "central" => {
            // Most gas wells are here; penalise monopolar (wellhead-like) signals.
            if !input.is_dipolar {
                score *= 0.7;
                reasons.push("-30% central basin monopolar (wellhead characteristic)".to_string());
            }
        }
        "eastern" => {
            // Deepest, fewest wells, best wreck hunting; bonus for dipolar signals.
            if input.is_dipolar {
                score *= 1.1;
                reasons.push("+10% eastern basin dipolar signature bonus".to_string());
            }
        }
        _ => {}
    }

    let (score, strike_reason) = apply_strike_scoring(score, input.elongation_azimuth_deg);
    if let Some(r) = strike_reason {
        reasons.push(r);
    }

    (score, reasons)
}

// ── Disposition false-positive filter (mag_data_pipeline.py) ────────────────

/// Disposition values that mean the wreck is no longer on the bottom.
/// Ports `RAISED_DISPOSITIONS` from `mag_data_pipeline.py::stage_cross_reference`.
pub const RAISED_DISPOSITIONS: [&str; 5] =
    ["raised_scrapped", "raised", "salvaged", "removed", "refloated"];

/// The `magnetic_potential` value that marks a known geological false positive.
pub const GEOLOGICAL_FALSE_POSITIVE: &str = "geological_false_positive";

/// Multiplier applied to the composite score of a candidate matched to a
/// raised/scrapped/false-positive wreck. The Python pipeline flags such matches
/// (`likely_false_positive = true`) and treats their magnetic signature as
/// geological; we down-rank hard so they fall to the bottom of the ranking.
pub const DISPOSITION_FALSE_POSITIVE_PENALTY: f64 = 0.1;

/// True if a wreck's disposition means any magnetic signature near it is likely
/// geological (raised/scrapped/salvaged/removed/refloated, or explicitly tagged
/// `geological_false_positive`).
///
/// Ports the disposition test in `stage_cross_reference`:
///   `salvage in RAISED_DISPOSITIONS or mag_pot == "geological_false_positive"`.
pub fn is_disposition_false_positive(salvage_status: &str, magnetic_potential: &str) -> bool {
    let salvage = salvage_status.trim().to_lowercase();
    let mag_pot = magnetic_potential.trim().to_lowercase();
    RAISED_DISPOSITIONS.contains(&salvage.as_str()) || mag_pot == GEOLOGICAL_FALSE_POSITIVE
}

/// Result of evaluating the disposition filter for a candidate.
pub struct DispositionFilter {
    pub likely_false_positive: bool,
    pub reason: Option<String>,
    /// Multiplier to apply to the composite score (1.0 when not flagged).
    pub score_multiplier: f64,
}

/// Evaluate the disposition false-positive filter for a candidate matched to a
/// known wreck. Ports the `likely_false_positive` branch of
/// `stage_cross_reference`.
///
/// `matched` indicates the candidate fell within the wreck match radius (the
/// Python `nearby` test); only then does disposition matter.
pub fn evaluate_disposition(
    matched: bool,
    wreck_name: &str,
    salvage_status: &str,
    magnetic_potential: &str,
) -> DispositionFilter {
    if matched && is_disposition_false_positive(salvage_status, magnetic_potential) {
        let disp = if salvage_status.trim().is_empty() {
            "removed".to_string()
        } else {
            salvage_status.trim().to_string()
        };
        DispositionFilter {
            likely_false_positive: true,
            reason: Some(format!(
                "{wreck_name} was {disp} — any magnetic signature is likely geological"
            )),
            score_multiplier: DISPOSITION_FALSE_POSITIVE_PENALTY,
        }
    } else {
        DispositionFilter {
            likely_false_positive: false,
            reason: None,
            score_multiplier: 1.0,
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn base_input(lat: f64, lon: f64) -> BasinScoringInput {
        BasinScoringInput {
            center_lat: lat,
            center_lon: lon,
            wellhead_distance_m: None,
            wreck_distance_m: None,
            is_dipolar: false,
            amplitude_peak_abs: 500.0,
            nearest_known_wreck: None,
            elongation_azimuth_deg: None,
        }
    }

    #[test]
    fn test_strike_deviation_off_axis_bonus() {
        let (s, reason) = apply_strike_scoring(10.0, Some(120.0));
        assert!(s > 10.0);
        assert!(reason.is_some());
    }

    #[test]
    fn test_strike_deviation_aligned_penalty() {
        let (s, reason) = apply_strike_scoring(10.0, Some(45.0));
        assert!(s < 10.0);
        assert!(reason.is_some());
    }

    #[test]
    fn test_basin_identification() {
        // Western basin point.
        assert_eq!(identify_basin(41.7, -82.5), Some("western"));
        // Central basin point.
        assert_eq!(identify_basin(42.0, -81.0), Some("central"));
        // Eastern basin point.
        assert_eq!(identify_basin(42.5, -79.5), Some("eastern"));
        // Way outside Lake Erie.
        assert_eq!(identify_basin(10.0, 10.0), None);
    }

    #[test]
    fn test_outside_basin_no_change() {
        let input = base_input(10.0, 10.0);
        let (score, reasons) = apply_basin_scoring(5.0, &input);
        assert_eq!(score, 5.0, "score must be unchanged outside any basin");
        assert!(reasons.is_empty());
    }

    /// Wellhead penalty multipliers: ×0.3 / ×0.6 / ×0.8 at <500/<1000/<2000m.
    #[test]
    fn test_wellhead_penalty_multipliers() {
        // Central basin, dipolar so the central monopolar penalty does not fire.
        let mut input = base_input(42.0, -81.0);
        input.is_dipolar = true;

        input.wellhead_distance_m = Some(400.0);
        let (s, _) = apply_basin_scoring(10.0, &input);
        assert!((s - 3.0).abs() < 1e-9, "×0.3 at <500m, got {s}");

        input.wellhead_distance_m = Some(800.0);
        let (s, _) = apply_basin_scoring(10.0, &input);
        assert!((s - 6.0).abs() < 1e-9, "×0.6 at <1000m, got {s}");

        input.wellhead_distance_m = Some(1500.0);
        let (s, _) = apply_basin_scoring(10.0, &input);
        assert!((s - 8.0).abs() < 1e-9, "×0.8 at <2000m, got {s}");

        input.wellhead_distance_m = Some(2500.0);
        let (s, _) = apply_basin_scoring(10.0, &input);
        assert!((s - 10.0).abs() < 1e-9, "no penalty beyond 2000m, got {s}");
    }

    /// Known-wreck bonus multipliers: ×1.5 / ×1.2 at <1000/<3000m.
    #[test]
    fn test_known_wreck_bonus_multipliers() {
        let mut input = base_input(42.5, -79.5); // eastern, not dipolar → no eastern bonus
        input.wreck_distance_m = Some(500.0);
        let (s, _) = apply_basin_scoring(10.0, &input);
        assert!((s - 15.0).abs() < 1e-9, "×1.5 at <1000m, got {s}");

        input.wreck_distance_m = Some(2000.0);
        let (s, _) = apply_basin_scoring(10.0, &input);
        assert!((s - 12.0).abs() < 1e-9, "×1.2 at <3000m, got {s}");

        input.wreck_distance_m = Some(4000.0);
        let (s, _) = apply_basin_scoring(10.0, &input);
        assert!((s - 10.0).abs() < 1e-9, "no bonus beyond 3000m, got {s}");
    }

    /// Western basin low-amplitude ×0.9 when peak < 100 nT.
    #[test]
    fn test_western_low_amplitude() {
        let mut input = base_input(41.7, -82.5);
        input.amplitude_peak_abs = 50.0;
        let (s, reasons) = apply_basin_scoring(10.0, &input);
        assert!((s - 9.0).abs() < 1e-9, "×0.9 western low-amp, got {s}");
        assert!(reasons.iter().any(|r| r.contains("western basin low amplitude")));

        // High amplitude → no western penalty.
        input.amplitude_peak_abs = 200.0;
        let (s, _) = apply_basin_scoring(10.0, &input);
        assert!((s - 10.0).abs() < 1e-9, "no western penalty at 200 nT, got {s}");
    }

    /// Central basin monopolar ×0.7 (only when NOT dipolar).
    #[test]
    fn test_central_monopolar_penalty() {
        let mut input = base_input(42.0, -81.0);
        input.is_dipolar = false;
        let (s, _) = apply_basin_scoring(10.0, &input);
        assert!((s - 7.0).abs() < 1e-9, "×0.7 central monopolar, got {s}");

        input.is_dipolar = true;
        let (s, _) = apply_basin_scoring(10.0, &input);
        assert!((s - 10.0).abs() < 1e-9, "no penalty for dipolar in central, got {s}");
    }

    /// Eastern basin dipolar ×1.1 (only when dipolar).
    #[test]
    fn test_eastern_dipolar_bonus() {
        let mut input = base_input(42.5, -79.5);
        input.is_dipolar = true;
        let (s, _) = apply_basin_scoring(10.0, &input);
        assert!((s - 11.0).abs() < 1e-9, "×1.1 eastern dipolar bonus, got {s}");

        input.is_dipolar = false;
        let (s, _) = apply_basin_scoring(10.0, &input);
        assert!((s - 10.0).abs() < 1e-9, "no eastern bonus for monopolar, got {s}");
    }

    /// Combined: wreck bonus and eastern dipolar bonus compound multiplicatively.
    #[test]
    fn test_combined_multipliers_compound() {
        let mut input = base_input(42.5, -79.5);
        input.is_dipolar = true;
        input.wreck_distance_m = Some(500.0); // ×1.5
        input.nearest_known_wreck = Some("SS Test".to_string());
        let (s, reasons) = apply_basin_scoring(10.0, &input);
        // 10 × 1.5 (wreck) × 1.1 (eastern dipolar) = 16.5
        assert!((s - 16.5).abs() < 1e-9, "compound multipliers, got {s}");
        assert_eq!(reasons.len(), 2);
    }

    // ── Disposition filter tests (Task 3) ──

    #[test]
    fn test_disposition_predicate() {
        assert!(is_disposition_false_positive("raised_scrapped", ""));
        assert!(is_disposition_false_positive("RAISED", ""));
        assert!(is_disposition_false_positive("  salvaged ", ""));
        assert!(is_disposition_false_positive("removed", ""));
        assert!(is_disposition_false_positive("refloated", ""));
        assert!(is_disposition_false_positive("", "geological_false_positive"));
        assert!(!is_disposition_false_positive("intact", "high"));
        assert!(!is_disposition_false_positive("", ""));
    }

    #[test]
    fn test_disposition_downranks_matched_raised_wreck() {
        let f = evaluate_disposition(true, "SS Raised", "raised_scrapped", "");
        assert!(f.likely_false_positive);
        assert!(f.reason.as_deref().unwrap().contains("SS Raised"));
        assert!((f.score_multiplier - DISPOSITION_FALSE_POSITIVE_PENALTY).abs() < 1e-12);

        // A penalised score must be much lower.
        let original = 50.0;
        assert!(original * f.score_multiplier < original);
    }

    #[test]
    fn test_disposition_no_effect_when_unmatched_or_intact() {
        // Not within match radius → no effect even if raised.
        let f = evaluate_disposition(false, "SS Raised", "raised", "");
        assert!(!f.likely_false_positive);
        assert!((f.score_multiplier - 1.0).abs() < 1e-12);

        // Matched but intact → no effect.
        let f = evaluate_disposition(true, "SS Intact", "intact", "high");
        assert!(!f.likely_false_positive);
        assert!((f.score_multiplier - 1.0).abs() < 1e-12);
    }
}
