use std::f64::consts::PI;

pub struct Wellhead {
    pub well_id: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub status: String,
    pub well_type: String,
    pub township: String,
    pub county: String,
    pub target: String,
    pub is_lake_erie: bool,
}

pub struct KnownWreck {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub vessel_type: String,
    pub length_ft: f64,
    pub depth_ft: f64,
    pub source: String,
    /// Salvage disposition (e.g. "raised_scrapped", "salvaged"); empty if unknown.
    /// Drives the disposition false-positive filter ported in scoring.rs
    /// (mag_data_pipeline.py::stage_cross_reference).
    pub salvage_status: String,
    /// Magnetic potential tag (e.g. "geological_false_positive"); empty if unknown.
    pub magnetic_potential: String,
}

pub struct CandidateMatch {
    pub label_id: u32,
    pub center_lat: f64,
    pub center_lon: f64,
    pub dipole_score: f32,
    pub dipole_verdict: String,
    pub ground_truth: String,
    pub ground_truth_name: String,
    pub well_distance_m: Option<f64>,
    pub nearest_wellhead: Option<String>,
    pub wreck_distance_m: Option<f64>,
    pub nearest_known_wreck: Option<String>,
    /// Salvage disposition of the nearest known wreck (for the disposition
    /// false-positive filter in scoring.rs). Empty when unknown / no match.
    pub nearest_wreck_salvage_status: String,
    /// Magnetic-potential tag of the nearest known wreck. Empty when unknown.
    pub nearest_wreck_magnetic_potential: String,
    pub bonus_score: f64,
    pub curvelet_energy_ratio: Option<f32>,
}

/// Great-circle distance in metres between two WGS-84 points.
pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6_371_000.0;
    
    let lat1_rad = lat1 * PI / 180.0;
    let lat2_rad = lat2 * PI / 180.0;
    
    let dlat = (lat2 - lat1) * PI / 180.0;
    let dlon = (lon2 - lon1) * PI / 180.0;
    
    let a = (dlat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (dlon / 2.0).sin().powi(2);
        
    r * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
}

/// Apply the Loran-C systematic warp correction for the Lake Erie region.
///
/// Ports `_apply_loran_c_warp` from `pipelines/mag/erie_wellhead_discriminator.py`.
///
/// Aero-mag surveys pre-GPS used the Great Lakes Loran-C chain (8970), which
/// introduces a systematic, per-basin coordinate offset (ASF / mixed land-water
/// propagation / chain geometry). The correction is applied before cross-
/// referencing candidate coordinates against known wells and wrecks.
///
/// - Eastern basin (lon > -80.5): shift ~250 m NE
/// - Central basin (-81.5 < lon <= -80.5): shift ~300 m N
/// - Western basin (lon <= -81.5): shift ~200 m NW
pub fn apply_loran_c_warp(lat: f64, lon: f64) -> (f64, f64) {
    let m_per_deg_lat = 111_320.0;
    let m_per_deg_lon = 111_320.0 * (lat * PI / 180.0).cos();

    let (dlat, dlon) = if lon > -80.5 {
        // Eastern basin — NE component (0.707 ≈ cos/sin 45°).
        (250.0 / m_per_deg_lat * 0.707, 250.0 / m_per_deg_lon * 0.707)
    } else if lon > -81.5 {
        // Central basin — due north.
        (300.0 / m_per_deg_lat, 0.0)
    } else {
        // Western basin — NW component.
        (200.0 / m_per_deg_lat * 0.707, -200.0 / m_per_deg_lon * 0.707)
    };

    (lat + dlat, lon + dlon)
}

pub fn cross_reference_candidate(
    candidate: &mut CandidateMatch,
    wells: &[Wellhead],
    wrecks: &[KnownWreck],
    wellhead_radius_m: f64,
    wreck_radius_m: f64,
    apply_loran_correction: bool,
) {
    // ── Loran-C correction for aero-mag targets ──
    // Ports the `apply_loran_correction` branch of
    // erie_wellhead_discriminator.py::cross_reference_candidates: the warped
    // coordinates are used for the well/wreck distance tests.
    let (search_lat, search_lon) = if apply_loran_correction {
        apply_loran_c_warp(candidate.center_lat, candidate.center_lon)
    } else {
        (candidate.center_lat, candidate.center_lon)
    };

    let mut min_well_dist = f64::MAX;
    let mut nearest_well = None;
    let mut nearest_well_status = String::new();

    for well in wells {
        let dist = haversine_m(search_lat, search_lon, well.lat, well.lon);
        if dist < min_well_dist {
            min_well_dist = dist;
            nearest_well = Some(well.name.clone());
            nearest_well_status = well.status.clone();
        }
    }
    
    let mut min_wreck_dist = f64::MAX;
    let mut nearest_wreck = None;
    let mut nearest_wreck_salvage = String::new();
    let mut nearest_wreck_magpot = String::new();
    
    for wreck in wrecks {
        let dist = haversine_m(search_lat, search_lon, wreck.lat, wreck.lon);
        if dist < min_wreck_dist {
            min_wreck_dist = dist;
            nearest_wreck = Some(wreck.name.clone());
            nearest_wreck_salvage = wreck.salvage_status.clone();
            nearest_wreck_magpot = wreck.magnetic_potential.clone();
        }
    }
    
    if min_well_dist < f64::MAX {
        candidate.well_distance_m = Some(min_well_dist);
        candidate.nearest_wellhead = nearest_well;
    }
    
    if min_wreck_dist < f64::MAX {
        candidate.wreck_distance_m = Some(min_wreck_dist);
        candidate.nearest_known_wreck = nearest_wreck;
        candidate.nearest_wreck_salvage_status = nearest_wreck_salvage;
        candidate.nearest_wreck_magnetic_potential = nearest_wreck_magpot;
    }
    
    // New classification logic: Weighted proximity instead of hard exclusion
    // Very close to a well => apply scaled penalty and flag for satellite cross-referencing.
    // Radius ports `wellhead_radius_m` from erie_central_aeromag_orchestrator.py.
    if let Some(dist) = candidate.well_distance_m {
        if dist <= wellhead_radius_m {
            candidate.ground_truth = "suspected_wellhead_requires_satellite_check".to_string();
            if let Some(name) = &candidate.nearest_wellhead {
                candidate.ground_truth_name = if nearest_well_status.is_empty() {
                    name.clone()
                } else {
                    format!("{name} [well status: {nearest_well_status}]")
                };
            }
            // Scale penalty based on proximity (up to -20.0 if right on top of it)
            let proximity_weight = 1.0 - (dist / wellhead_radius_m);
            candidate.bonus_score -= 20.0 * proximity_weight;
            // Do not return here, allow other factors to contribute so the orchestrator can decide
        }
    }

    // NauticUVs integration: Curvelet energy boost
    if let Some(energy) = candidate.curvelet_energy_ratio {
        if energy > 3.5 {
            // Significant structural energy found via curvelet transform
            candidate.bonus_score += (energy as f64 - 3.5) * 5.0;
        }
    }
    
    // Close to a wreck => likely a wreck.
    // Radius ports `wreck_radius_m` from erie_wellhead_discriminator.py::cross_reference_candidates.
    if let Some(dist) = candidate.wreck_distance_m {
        if dist <= wreck_radius_m {
            candidate.ground_truth = "wreck".to_string();
            if let Some(name) = &candidate.nearest_known_wreck {
                candidate.ground_truth_name = name.clone();
            }
            // Scale boost based on proximity
            let proximity_weight = 1.0 - (dist / wreck_radius_m);
            candidate.bonus_score += 20.0 * proximity_weight; 
            return;
        }
    }
    
    if candidate.ground_truth.is_empty() {
        // Otherwise unknown geometry
        candidate.ground_truth = "unknown".to_string();
    }
}
