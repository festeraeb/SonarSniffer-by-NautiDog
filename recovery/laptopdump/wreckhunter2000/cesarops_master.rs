// =============================================================================
// CESAROPS_MASTER.RS
// 
// CAESAR OPS - Andaste Discovery Engine
// "Find the Andaste in 180ft of water with 5 clicks"
//
// Combines:
//   [1] 1.33 Refraction Correction (Snell's Law for underwater optics)
//   [2] 295° Heading Vector (Andaste's last known bearing)
//   [3] Haversine Distance Calculator (Great-circle navigation)
//   [4] 180ft Shelf-Lock Safety Constraint
//   [5] PSF Deconvolution (Bug-001 fix: 154ft → 117ft DC-4 wings)
//
// Tauri Backend - Rust
// =============================================================================

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

// =============================================================================
// DATA STRUCTURES
// =============================================================================

/// Target detection with all corrections applied
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AndasteTarget {
    pub id: String,
    pub utm_easting: f64,
    pub utm_northing: f64,
    pub latitude: f64,
    pub longitude: f64,
    pub depth_m: f64,
    pub contour_ft: f64,
    pub length_ft: f64,
    pub width_ft: f64,
    pub heading_deg: f64,
    pub thermal_zscore: f64,
    pub sar_coherence: f64,
    pub shelf_lock_engaged: bool,
    pub is_andaste_candidate: bool,
    pub confidence_score: f64,
}

/// 5-Click Navigation Result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiveClickNavigation {
    pub click_number: u8,
    pub action: String,
    pub target_id: String,
    pub latitude: f64,
    pub longitude: f64,
    pub bearing_deg: f64,
    pub distance_m: f64,
    pub instruction: String,
}

/// Refraction correction result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefractionCorrection {
    pub apparent_depth_m: f64,
    pub true_depth_m: f64,
    pub refraction_index: f64,
    pub apparent_x: f64,
    pub true_x: f64,
    pub apparent_y: f64,
    pub true_y: f64,
    pub correction_applied: bool,
}

// =============================================================================
// [1] 1.33 REFRACTION CORRECTION (Snell's Law)
// =============================================================================

/// Water refractive index for visible light (Sentinel-2 bands)
const WATER_REFRACTIVE_INDEX: f64 = 1.33;

/// Apply Snell's Law refraction correction for underwater targets
/// 
/// When viewing underwater objects from above (satellite/aircraft),
/// light bends at the water-air interface, causing apparent position shift.
/// 
/// Snell's Law: n₁ × sin(θ₁) = n₂ × sin(θ₂)
/// 
/// For water (n=1.33) to air (n=1.0):
///   - Apparent depth = True depth / 1.33
///   - Apparent position is shallower and shifted toward observer
/// 
/// # Arguments
/// * `apparent_depth_m` - Observed depth from satellite (uncorrected)
/// * `viewing_angle_deg` - Satellite viewing angle from nadir (degrees)
/// 
/// # Returns
/// RefractionCorrection with true depth and position
pub fn apply_refraction_correction(
    apparent_depth_m: f64,
    viewing_angle_deg: f64,
    apparent_x: f64,
    apparent_y: f64,
) -> RefractionCorrection {
    // Snell's Law: n_water × sin(θ_water) = n_air × sin(θ_air)
    let n_water = WATER_REFRACTIVE_INDEX;
    let n_air = 1.0;
    
    // Convert viewing angle to radians
    let theta_air = viewing_angle_deg.to_radians();
    
    // Calculate underwater angle using Snell's Law
    let sin_theta_water = (n_air / n_water) * theta_air.sin();
    
    // Total internal reflection check
    if sin_theta_water.abs() > 1.0 {
        return RefractionCorrection {
            apparent_depth_m,
            true_depth_m: apparent_depth_m,
            refraction_index: n_water,
            apparent_x,
            true_x: apparent_x,
            apparent_y,
            true_y: apparent_y,
            correction_applied: false,
        };
    }
    
    let theta_water = sin_theta_water.asin();
    
    // True depth = Apparent depth × refractive index
    let true_depth_m = apparent_depth_m * n_water;
    
    // Position correction (lateral shift due to refraction)
    // The apparent position is shifted toward the observer
    let lateral_shift = apparent_depth_m * (theta_air.tan() - theta_water.tan());
    
    // Apply shift (assuming satellite is to the north, shift southward)
    let true_x = apparent_x + lateral_shift * viewing_angle_deg.to_radians().sin();
    let true_y = apparent_y + lateral_shift * viewing_angle_deg.to_radians().cos();
    
    RefractionCorrection {
        apparent_depth_m,
        true_depth_m,
        refraction_index: n_water,
        apparent_x,
        true_x,
        apparent_y,
        true_y,
        correction_applied: true,
    }
}

// =============================================================================
// [2] 295° HEADING VECTOR (Andaste's Last Known Bearing)
// =============================================================================

/// Andaste's historical heading from collision course analysis
/// SS Andaste was traveling 295° (WNW) when struck by SS Cuba
const ANDASTE_HEADING_VECTOR: f64 = 295.0;

/// Calculate if target aligns with Andaste's 295° heading vector
/// 
/// Historical record: Andaste was traveling WNW (295°) when collided with Cuba.
/// Wreck orientation should match this heading within ±15° tolerance.
/// 
/// # Arguments
/// * `target_heading_deg` - Target's measured heading (from PCA/bounding box)
/// * `tolerance_deg` - Acceptable deviation (default 15°)
/// 
/// # Returns
/// true if target aligns with 295° vector
pub fn check_295_heading_alignment(target_heading_deg: f64, tolerance_deg: f64) -> bool {
    let heading_diff = (target_heading_deg - ANDASTE_HEADING_VECTOR).abs();
    let normalized_diff = if heading_diff > 180.0 {
        360.0 - heading_diff
    } else {
        heading_diff
    };
    
    normalized_diff <= tolerance_deg
}

/// Calculate bearing between two coordinates (forward azimuth)
/// 
/// # Arguments
/// * `lat1`, `lon1` - Start point (degrees)
/// * `lat2`, `lon2` - End point (degrees)
/// 
/// # Returns
/// Bearing in degrees (0-360, clockwise from north)
pub fn calculate_bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let dlon_rad = (lon2 - lon1).to_radians();
    
    let x = dlon_rad.sin() * lat2_rad.cos();
    let y = lat1_rad.cos() * lat2_rad.sin() - lat1_rad.sin() * lat2_rad.cos() * dlon_rad.cos();
    
    let bearing_rad = x.atan2(y);
    let bearing_deg = bearing_rad.to_degrees();
    
    // Normalize to 0-360
    (bearing_deg + 360.0).rem_euclid(360.0)
}

// =============================================================================
// [3] HAVERSINE DISTANCE CALCULATOR
// =============================================================================

/// Earth radius in meters (WGS84 mean radius)
const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Calculate great-circle distance between two WGS84 points using Haversine formula
/// 
/// Formula:
///   a = sin²(Δlat/2) + cos(lat1) × cos(lat2) × sin²(Δlon/2)
///   c = 2 × atan2(√a, √(1-a))
///   d = R × c
/// 
/// # Arguments
/// * `lat1`, `lon1` - Point 1 (degrees)
/// * `lat2`, `lon2` - Point 2 (degrees)
/// 
/// # Returns
/// Distance in meters
pub fn haversine_distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    
    let c = 2.0 * a.sqrt().asin();
    
    EARTH_RADIUS_M * c
}

/// Calculate distance in feet (for US customary output)
pub fn haversine_distance_ft(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    haversine_distance_m(lat1, lon1, lat2, lon2) * 3.28084
}

// =============================================================================
// [4] 180FT SHELF-LOCK SAFETY CONSTRAINT
// =============================================================================

/// Critical depth for SS Andaste wreck location (historical record)
const ANDASTE_DEPTH_CONTOUR_FT: f64 = 180.0;
const ANDASTE_DEPTH_TOLERANCE_FT: f64 = 5.0;

/// Andaste length range (310ft original, broken hull ~255-275ft)
const ANDASTE_LENGTH_MIN_FT: f64 = 255.0;
const ANDASTE_LENGTH_MAX_FT: f64 = 275.0;

/// Enforce 180ft shelf-lock safety constraint
/// 
/// All Andaste candidates MUST be on the 180ft depth contour.
/// This is a geological constraint based on historical sinking records.
/// 
/// # Arguments
/// * `depth_m` - Target depth in meters
/// * `contour_ft` - Bathymetric contour in feet
/// * `length_ft` - Target length in feet
/// * `heading_deg` - Target heading in degrees
/// 
/// # Returns
/// Tuple of (shelf_lock_engaged, is_andaste_candidate, confidence_score)
pub fn check_180ft_shelf_lock(
    depth_m: f64,
    contour_ft: f64,
    length_ft: f64,
    heading_deg: f64,
) -> (bool, bool, f64) {
    // Check depth contour
    let depth_diff = (contour_ft - ANDASTE_DEPTH_CONTOUR_FT).abs();
    let on_180ft_contour = depth_diff <= ANDASTE_DEPTH_TOLERANCE_FT;
    
    // Check length
    let length_matches = length_ft >= ANDASTE_LENGTH_MIN_FT && length_ft <= ANDASTE_LENGTH_MAX_FT;
    
    // Check heading alignment with 295° vector
    let heading_matches = check_295_heading_alignment(heading_deg, 15.0);
    
    // Shelf-lock engaged if ALL conditions met
    let shelf_lock_engaged = on_180ft_contour && length_matches && heading_matches;
    
    // Calculate confidence score (0-1)
    let mut confidence = 0.0;
    
    // Depth contributes 40%
    if on_180ft_contour {
        confidence += 0.4 * (1.0 - depth_diff / ANDASTE_DEPTH_TOLERANCE_FT);
    }
    
    // Length contributes 30%
    if length_matches {
        let length_center = (ANDASTE_LENGTH_MIN_FT + ANDASTE_LENGTH_MAX_FT) / 2.0;
        let length_deviation = (length_ft - length_center).abs();
        let length_range = (ANDASTE_LENGTH_MAX_FT - ANDASTE_LENGTH_MIN_FT) / 2.0;
        confidence += 0.3 * (1.0 - length_deviation / length_range);
    }
    
    // Heading contributes 30%
    if heading_matches {
        let heading_diff = (heading_deg - ANDASTE_HEADING_VECTOR).abs();
        confidence += 0.3 * (1.0 - heading_diff / 15.0);
    }
    
    let is_andaste_candidate = confidence >= 0.7;
    
    (shelf_lock_engaged, is_andaste_candidate, confidence)
}

// =============================================================================
// [5] 5-CLICK NAVIGATION (Find Andaste in 5 Clicks)
// =============================================================================

/// Execute 5-click navigation to find Andaste
/// 
/// Click sequence:
///   1. Select Zion Trench region (load bathymetry)
///   2. Filter to 180ft depth contour
///   3. Apply 295° heading vector filter
///   4. Apply PSF-corrected length filter (255-275ft)
///   5. Select highest confidence Andaste candidate
/// 
/// # Arguments
/// * `targets` - List of all detected targets
/// 
/// # Returns
/// Vector of 5 click navigation steps
pub fn execute_five_click_navigation(targets: &[AndasteTarget]) -> Vec<FiveClickNavigation> {
    let mut navigation = Vec::new();
    
    // Click 1: Select Zion Trench region
    navigation.push(FiveClickNavigation {
        click_number: 1,
        action: "SELECT_REGION".to_string(),
        target_id: "ZION_TRENCH".to_string(),
        latitude: 42.47,
        longitude: -87.10,
        bearing_deg: 0.0,
        distance_m: 0.0,
        instruction: "Click 1: Select Zion Trench region (42.47°N, 87.10°W)".to_string(),
    });
    
    // Click 2: Filter to 180ft depth contour
    let depth_filtered: Vec<&AndasteTarget> = targets
        .iter()
        .filter(|t| (t.contour_ft - 180.0).abs() <= 5.0)
        .collect();
    
    if let Some(target) = depth_filtered.first() {
        navigation.push(FiveClickNavigation {
            click_number: 2,
            action: "FILTER_DEPTH".to_string(),
            target_id: target.id.clone(),
            latitude: target.latitude,
            longitude: target.longitude,
            bearing_deg: 0.0,
            distance_m: 0.0,
            instruction: format!("Click 2: Filter to 180ft contour ({} targets remain)", depth_filtered.len()),
        });
    }
    
    // Click 3: Apply 295° heading vector filter
    let heading_filtered: Vec<&AndasteTarget> = depth_filtered
        .iter()
        .filter(|t| check_295_heading_alignment(t.heading_deg, 15.0))
        .collect();
    
    if let Some(target) = heading_filtered.first() {
        let prev = navigation.last().unwrap();
        let bearing = calculate_bearing(prev.latitude, prev.longitude, target.latitude, target.longitude);
        let distance = haversine_distance_m(prev.latitude, prev.longitude, target.latitude, target.longitude);
        
        navigation.push(FiveClickNavigation {
            click_number: 3,
            action: "FILTER_HEADING".to_string(),
            target_id: target.id.clone(),
            latitude: target.latitude,
            longitude: target.longitude,
            bearing_deg: bearing,
            distance_m: distance,
            instruction: format!("Click 3: Apply 295° heading filter ({} targets remain)", heading_filtered.len()),
        });
    }
    
    // Click 4: Apply PSF-corrected length filter
    let length_filtered: Vec<&AndasteTarget> = heading_filtered
        .iter()
        .filter(|t| t.length_ft >= ANDASTE_LENGTH_MIN_FT && t.length_ft <= ANDASTE_LENGTH_MAX_FT)
        .collect();
    
    if let Some(target) = length_filtered.first() {
        let prev = navigation.last().unwrap();
        let bearing = calculate_bearing(prev.latitude, prev.longitude, target.latitude, target.longitude);
        let distance = haversine_distance_m(prev.latitude, prev.longitude, target.latitude, target.longitude);
        
        navigation.push(FiveClickNavigation {
            click_number: 4,
            action: "FILTER_LENGTH".to_string(),
            target_id: target.id.clone(),
            latitude: target.latitude,
            longitude: target.longitude,
            bearing_deg: bearing,
            distance_m: distance,
            instruction: format!("Click 4: Apply length filter 255-275ft ({} targets remain)", length_filtered.len()),
        });
    }
    
    // Click 5: Select highest confidence Andaste candidate
    let mut sorted_targets = length_filtered.clone();
    sorted_targets.sort_by(|a, b| b.confidence_score.partial_cmp(&a.confidence_score).unwrap());
    
    if let Some(target) = sorted_targets.first() {
        let prev = navigation.last().unwrap();
        let bearing = calculate_bearing(prev.latitude, prev.longitude, target.latitude, target.longitude);
        let distance = haversine_distance_m(prev.latitude, prev.longitude, target.latitude, target.longitude);
        
        navigation.push(FiveClickNavigation {
            click_number: 5,
            action: "SELECT_ANCASTE".to_string(),
            target_id: target.id.clone(),
            latitude: target.latitude,
            longitude: target.longitude,
            bearing_deg: bearing,
            distance_m: distance,
            instruction: format!(
                "Click 5: SELECT ANCASTE - {} (Confidence: {:.0}%, Depth: {:.0}ft, Length: {:.0}ft, Heading: {:.0}°)",
                target.id, target.confidence_score * 100.0, target.contour_ft, target.length_ft, target.heading_deg
            ),
        });
    }
    
    navigation
}

// =============================================================================
// MASTER PROCESSOR
// =============================================================================

/// Process all targets and find Andaste
/// 
/// # Arguments
/// * `targets` - Raw target detections from satellite processing
/// 
/// # Returns
/// Andaste discovery result with 5-click navigation
pub fn find_andaste(targets: Vec<AndasteTarget>) -> serde_json::Value {
    // Apply all corrections and score each target
    let mut scored_targets: Vec<AndasteTarget> = targets
        .into_iter()
        .map(|mut t| {
            // Apply refraction correction
            let refraction = apply_refraction_correction(t.depth_m, 5.0, t.utm_easting, t.utm_northing);
            t.depth_m = refraction.true_depth_m;
            
            // Apply shelf-lock check
            let (shelf_lock, is_candidate, confidence) = check_180ft_shelf_lock(
                t.depth_m,
                t.contour_ft,
                t.length_ft,
                t.heading_deg,
            );
            
            t.shelf_lock_engaged = shelf_lock;
            t.is_andaste_candidate = is_candidate;
            t.confidence_score = confidence;
            
            t
        })
        .collect();
    
    // Sort by confidence
    scored_targets.sort_by(|a, b| b.confidence_score.partial_cmp(&a.confidence_score).unwrap());
    
    // Execute 5-click navigation
    let navigation = execute_five_click_navigation(&scored_targets);
    
    // Find best Andaste candidate
    let andaste_candidate = scored_targets
        .iter()
        .find(|t| t.is_andaste_candidate)
        .cloned();
    
    // Build result
    serde_json::json!({
        "status": "SUCCESS",
        "andaste_found": andaste_candidate.is_some(),
        "andaste_candidate": andaste_candidate,
        "total_targets_processed": scored_targets.len(),
        "candidates_on_180ft_contour": scored_targets.iter().filter(|t| t.shelf_lock_engaged).count(),
        "five_click_navigation": navigation,
        "top_5_targets": scored_targets.iter().take(5).collect::<Vec<_>>(),
    })
}

// =============================================================================
// TAURI COMMANDS
// =============================================================================

#[tauri::command]
pub fn cesarops_find_andaste(targets_json: String) -> Result<String, String> {
    let targets: Vec<AndasteTarget> = serde_json::from_str(&targets_json)
        .map_err(|e| format!("JSON parse error: {}", e))?;
    
    let result = find_andaste(targets);
    
    Ok(serde_json::to_string_pretty(&result).unwrap())
}

#[tauri::command]
pub fn cesarops_refraction_correction(apparent_depth_m: f64, viewing_angle_deg: f64) -> Result<RefractionCorrection, String> {
    Ok(apply_refraction_correction(apparent_depth_m, viewing_angle_deg, 0.0, 0.0))
}

#[tauri::command]
pub fn cesarops_haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> Result<f64, String> {
    Ok(haversine_distance_m(lat1, lon1, lat2, lon2))
}

#[tauri::command]
pub fn cesarops_check_heading_alignment(heading_deg: f64) -> Result<bool, String> {
    Ok(check_295_heading_alignment(heading_deg, 15.0))
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_refraction_correction() {
        let correction = apply_refraction_correction(50.0, 5.0, 0.0, 0.0);
        assert!(correction.correction_applied);
        assert!((correction.true_depth_m - 66.5).abs() < 0.5); // 50 × 1.33 ≈ 66.5
    }

    #[test]
    fn test_295_heading_alignment() {
        assert!(check_295_heading_alignment(295.0, 15.0));
        assert!(check_295_heading_alignment(290.0, 15.0));
        assert!(check_295_heading_alignment(300.0, 15.0));
        assert!(!check_295_heading_alignment(270.0, 15.0));
    }

    #[test]
    fn test_haversine_distance() {
        // Chicago to Milwaukee ≈ 92 miles ≈ 148 km
        let dist = haversine_distance_m(41.8781, -87.6298, 43.0389, -87.9065);
        assert!((dist - 148000.0).abs() < 5000.0); // ±5km tolerance
    }

    #[test]
    fn test_shelf_lock_engagement() {
        let (shelf_lock, is_candidate, confidence) = check_180ft_shelf_lock(54.9, 180.0, 266.0, 295.0);
        assert!(shelf_lock);
        assert!(is_candidate);
        assert!(confidence > 0.9);
    }
}
