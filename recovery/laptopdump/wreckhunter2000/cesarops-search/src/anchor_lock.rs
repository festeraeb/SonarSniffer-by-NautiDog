// Anchor-Lock Calibration System for Great Lakes
// Implements the "Anchor-Lock" protocol from MASTER_FORENSIC_LEDGER V2.0
// Uses fixed harbor lights and breakwaters as calibration references

use crate::coordinate::{UTMCoordinate, WGS84Coordinate, wgs84_to_utm};

/// Harbor Light Anchor Points for Great Lakes calibration
/// These are permanent, visible, fixed structures used for georeferencing
#[derive(Debug, Clone)]
pub struct HarborLightAnchor {
    pub name: &'static str,
    pub state: &'static str,
    pub wgs84: WGS84Coordinate,
    pub utm: UTMCoordinate,
    pub structure_type: &'static str,
    pub notes: &'static str,
}

impl HarborLightAnchor {
    pub fn new(name: &'static str, state: &'static str, lat: f64, lon: f64, 
               structure_type: &'static str, notes: &'static str) -> Self {
        let (easting, northing, zone) = wgs84_to_utm(lat, lon);
        Self {
            name,
            state,
            wgs84: WGS84Coordinate { lat, lon },
            utm: UTMCoordinate { easting, northing, zone },
            structure_type,
            notes,
        }
    }
}

/// Great Lakes Harbor Light Network
/// Organized by state/region for cross-validation
pub struct AnchorLockNetwork {
    pub wisconsin: Vec<HarborLightAnchor>,
    pub michigan: Vec<HarborLightAnchor>,
    pub illinois: Vec<HarborLightAnchor>,
    pub indiana: Vec<HarborLightAnchor>,
}

impl AnchorLockNetwork {
    pub fn new() -> Self {
        Self {
            // Wisconsin Side - Lake Michigan West Shore
            wisconsin: vec![
                HarborLightAnchor::new(
                    "North Point Light",
                    "WI",
                    43.0642, -87.8728,
                    "Steel Tower",
                    "Milwaukee harbor entrance - prominent landmark"
                ),
                HarborLightAnchor::new(
                    "Wind Point Light",
                    "WI", 
                    42.7997, -87.8181,
                    "Brick Tower",
                    "Racine - oldest lighthouse in Wisconsin (1880)"
                ),
                HarborLightAnchor::new(
                    "Sheboygan Breakwater Light",
                    "WI",
                    43.7636, -87.6856,
                    "Steel Pierhead",
                    "Sheboygan harbor breakwater"
                ),
            ],
            
            // Michigan Side - Lake Michigan East Shore  
            michigan: vec![
                HarborLightAnchor::new(
                    "Grand Haven Pierhead Light",
                    "MI",
                    43.0636, -86.2544,
                    "Steel Tower",
                    "Grand Haven - 'Coast Guard City USA'"
                ),
                HarborLightAnchor::new(
                    "Holland Harbor Light",
                    "MI",
                    42.7786, -86.2064,
                    "Steel Frame",
                    "Holland - 'Big Red' iconic lighthouse"
                ),
                HarborLightAnchor::new(
                    "Muskegon Breakwater Light",
                    "MI",
                    43.2544, -86.2706,
                    "Steel Tower",
                    "Muskegon harbor entrance"
                ),
                HarborLightAnchor::new(
                    "St. Joseph North Pier Light",
                    "MI",
                    42.1103, -86.4864,
                    "Steel Tower",
                    "St. Joseph - twin lighthouses"
                ),
            ],
            
            // Illinois - Southwest Lake Michigan
            illinois: vec![
                HarborLightAnchor::new(
                    "Chicago Harbor Light",
                    "IL",
                    41.8897, -87.6047,
                    "Steel Caisson",
                    "Chicago breakwater - major reference point"
                ),
                HarborLightAnchor::new(
                    "Waukegan Harbor Light",
                    "IL",
                    42.3636, -87.8036,
                    "Steel Tower",
                    "Waukegan - Zion cluster reference (MASTER_FORENSIC_LEDGER example)"
                ),
                HarborLightAnchor::new(
                    "Evanston Light",
                    "IL",
                    42.0503, -87.6686,
                    "Steel Skeleton",
                    "Evanston - Northwestern University vicinity"
                ),
            ],
            
            // Indiana - Southern Lake Michigan
            indiana: vec![
                HarborLightAnchor::new(
                    "Michigan City East Pierhead Light",
                    "IN",
                    41.7136, -86.8864,
                    "Steel Tower",
                    "Michigan City - active harbor entrance"
                ),
                HarborLightAnchor::new(
                    "Gary Breakwater Light",
                    "IN",
                    41.6136, -87.3036,
                    "Steel Skeleton",
                    "Gary - industrial harbor reference"
                ),
            ],
        }
    }
    
    /// Get all anchors across all regions
    pub fn all_anchors(&self) -> Vec<&HarborLightAnchor> {
        let mut all = Vec::new();
        all.extend(&self.wisconsin);
        all.extend(&self.michigan);
        all.extend(&self.illinois);
        all.extend(&self.indiana);
        all
    }
    
    /// Get anchors for a specific state
    pub fn get_by_state(&self, state: &str) -> Vec<&HarborLightAnchor> {
        match state.to_uppercase().as_str() {
            "WI" | "WISCONSIN" => self.wisconsin.iter().collect(),
            "MI" | "MICHIGAN" => self.michigan.iter().collect(),
            "IL" | "ILLINOIS" => self.illinois.iter().collect(),
            "IN" | "INDIANA" => self.indiana.iter().collect(),
            _ => Vec::new(),
        }
    }
}

/// Calibration result from anchor-lock process
#[derive(Debug, Clone)]
pub struct AnchorLockCalibration {
    pub anchor_name: String,
    pub expected_utm: UTMCoordinate,
    pub detected_pixel_utm: UTMCoordinate,
    pub offset_easting: f64,
    pub offset_northing: f64,
    pub offset_meters: f64,
    pub confidence: f64,
}

impl AnchorLockCalibration {
    /// Calculate the Euclidean distance of the offset
    pub fn total_offset(&self) -> f64 {
        (self.offset_easting.powi(2) + self.offset_northing.powi(2)).sqrt()
    }
    
    /// Apply this calibration offset to a target coordinate
    pub fn apply_correction(&self, target_easting: f64, target_northing: f64) -> (f64, f64) {
        (
            target_easting - self.offset_easting,
            target_northing - self.offset_northing,
        )
    }
}

/// Main anchor-lock calibration processor
pub struct AnchorLockProcessor {
    pub network: AnchorLockNetwork,
    pub calibration_results: Vec<AnchorLockCalibration>,
}

impl AnchorLockProcessor {
    pub fn new() -> Self {
        Self {
            network: AnchorLockNetwork::new(),
            calibration_results: Vec::new(),
        }
    }
    
    /// Process anchor detection from satellite imagery
    /// Returns the calibration offset to apply to all targets
    pub fn calibrate_from_anchors(
        &mut self,
        anchor_name: &str,
        detected_easting: f64,
        detected_northing: f64,
    ) -> Option<AnchorLockCalibration> {
        // Find the anchor in our network
        let anchors = self.network.all_anchors();
        let anchor = anchors.iter().find(|a| a.name == anchor_name)?;
        
        let expected = &anchor.utm;
        
        let offset_easting = detected_easting - expected.easting;
        let offset_northing = detected_northing - expected.northing;
        let offset_meters = (offset_easting.powi(2) + offset_northing.powi(2)).sqrt();
        
        // Calculate confidence based on offset magnitude
        // <10m = excellent, <50m = good, <100m = acceptable, >100m = poor
        let confidence = if offset_meters < 10.0 {
            1.0
        } else if offset_meters < 50.0 {
            0.9
        } else if offset_meters < 100.0 {
            0.7
        } else {
            0.5
        };
        
        let calibration = AnchorLockCalibration {
            anchor_name: anchor_name.to_string(),
            expected_utm: expected.clone(),
            detected_pixel_utm: UTMCoordinate {
                easting: detected_easting,
                northing: detected_northing,
                zone: expected.zone,
            },
            offset_easting,
            offset_northing,
            offset_meters,
            confidence,
        };
        
        self.calibration_results.push(calibration.clone());
        Some(calibration)
    }
    
    /// Get the average offset from all calibrations (weighted by confidence)
    pub fn get_weighted_average_offset(&self) -> Option<(f64, f64, f64)> {
        if self.calibration_results.is_empty() {
            return None;
        }
        
        let total_weight: f64 = self.calibration_results.iter()
            .map(|c| c.confidence)
            .sum();
        
        if total_weight == 0.0 {
            return None;
        }
        
        let avg_easting: f64 = self.calibration_results.iter()
            .map(|c| c.offset_easting * c.confidence)
            .sum::<f64>() / total_weight;
        
        let avg_northing: f64 = self.calibration_results.iter()
            .map(|c| c.offset_northing * c.confidence)
            .sum::<f64>() / total_weight;
        
        let avg_magnitude = (avg_easting.powi(2) + avg_northing.powi(2)).sqrt();
        
        Some((avg_easting, avg_northing, avg_magnitude))
    }
    
    /// Print calibration report
    pub fn print_report(&self) {
        println!("\n================================================================================");
        println!("ANCHOR-LOCK CALIBRATION REPORT");
        println!("================================================================================\n");
        
        println!("ANCHOR POINTS USED:");
        println!("--------------------------------------------------------------------------------");
        for result in &self.calibration_results {
            println!("  Anchor: {}", result.anchor_name);
            println!("    Expected UTM:    E:{:.2} N:{:.2}", 
                     result.expected_utm.easting, result.expected_utm.northing);
            println!("    Detected UTM:    E:{:.2} N:{:.2}", 
                     result.detected_pixel_utm.easting, result.detected_pixel_utm.northing);
            println!("    Offset:          ΔE:{:+.2}m ΔN:{:+.2}m (Total: {:.2}m)", 
                     result.offset_easting, result.offset_northing, result.total_offset());
            println!("    Confidence:      {:.1}%", result.confidence * 100.0);
            println!();
        }
        
        if let Some((avg_e, avg_n, avg_mag)) = self.get_weighted_average_offset() {
            println!("WEIGHTED AVERAGE OFFSET:");
            println!("--------------------------------------------------------------------------------");
            println!("  ΔEasting:  {:+.2} meters", avg_e);
            println!("  ΔNorthing: {:+.2} meters", avg_n);
            println!("  Magnitude: {:.2} meters", avg_mag);
            println!();
            println!("APPLICATION:");
            println!("  Apply this offset to ALL submerged target coordinates:");
            println!("    corrected_easting = detected_easting - ({:.2})", avg_e);
            println!("    corrected_northing = detected_northing - ({:.2})", avg_n);
        }
        
        println!("\n================================================================================\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_anchor_network_creation() {
        let network = AnchorLockNetwork::new();
        assert_eq!(network.wisconsin.len(), 3);
        assert_eq!(network.michigan.len(), 4);
        assert_eq!(network.illinois.len(), 3);
        assert_eq!(network.indiana.len(), 2);
    }
    
    #[test]
    fn test_calibration() {
        let mut processor = AnchorLockProcessor::new();
        
        // Simulate detecting Chicago Harbor Light with 15m offset
        let result = processor.calibrate_from_anchors(
            "Chicago Harbor Light",
            -87.6045, // slightly offset longitude
            41.8898,  // slightly offset latitude
        );
        
        assert!(result.is_some());
        let cal = result.unwrap();
        assert!(cal.total_offset() > 0.0);
    }
}
