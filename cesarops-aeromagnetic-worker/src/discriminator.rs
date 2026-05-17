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

pub fn cross_reference_candidate(
    candidate: &mut CandidateMatch,
    wells: &[Wellhead],
    wrecks: &[KnownWreck],
) {
    let mut min_well_dist = f64::MAX;
    let mut nearest_well = None;
    
    for well in wells {
        let dist = haversine_m(candidate.center_lat, candidate.center_lon, well.lat, well.lon);
        if dist < min_well_dist {
            min_well_dist = dist;
            nearest_well = Some(well.name.clone());
        }
    }
    
    let mut min_wreck_dist = f64::MAX;
    let mut nearest_wreck = None;
    
    for wreck in wrecks {
        let dist = haversine_m(candidate.center_lat, candidate.center_lon, wreck.lat, wreck.lon);
        if dist < min_wreck_dist {
            min_wreck_dist = dist;
            nearest_wreck = Some(wreck.name.clone());
        }
    }
    
    if min_well_dist < f64::MAX {
        candidate.well_distance_m = Some(min_well_dist);
        candidate.nearest_wellhead = nearest_well;
    }
    
    if min_wreck_dist < f64::MAX {
        candidate.wreck_distance_m = Some(min_wreck_dist);
        candidate.nearest_known_wreck = nearest_wreck;
    }
    
    // New classification logic: Weighted proximity instead of hard exclusion
    // Very close to a well => apply scaled penalty and flag for satellite cross-referencing
if let Some(dist) = candidate.well_distance_m {
        if dist <= 150.0 {
            candidate.ground_truth = "suspected_wellhead_requires_satellite_check".to_string();
            if let Some(name) = &candidate.nearest_wellhead {
                candidate.ground_truth_name = name.clone();
            }
            // Scale penalty based on proximity (up to -20.0 if right on top of it)
            let proximity_weight = 1.0 - (dist / 150.0);
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
    
    // Close to a wreck => likely a wreck
    if let Some(dist) = candidate.wreck_distance_m {
        if dist <= 200.0 {
            candidate.ground_truth = "wreck".to_string();
            if let Some(name) = &candidate.nearest_known_wreck {
                candidate.ground_truth_name = name.clone();
            }
            // Scale boost based on proximity
            let proximity_weight = 1.0 - (dist / 200.0);
            candidate.bonus_score += 20.0 * proximity_weight; 
            return;
        }
    }
    
    if candidate.ground_truth.is_empty() {
        // Otherwise unknown geometry
        candidate.ground_truth = "unknown".to_string();
    }
}
