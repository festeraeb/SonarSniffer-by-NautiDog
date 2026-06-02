// Multi-sensor fusion classification
// Each sensor has a specialty - combine for best results

use crate::scanner::Detection;

/// Classify target using multi-sensor fusion
pub fn classify(detection: &Detection) -> String {
    let score = detection.score;
    let aluminum = detection.aluminum_ratio;
    let thermal = detection.thermal_delta;

    if score > 0.9 {
        "HIGH_CONFIDENCE".to_string()
    } else if aluminum > 1.5 && thermal.abs() > 0.3 {
        "LIKELY_ALUMINUM".to_string()
    } else if thermal.abs() > 0.7 {
        "HEAVY_STEEL_MASS".to_string()
    } else if aluminum > 1.2 {
        "POSSIBLE_ALUMINUM".to_string()
    } else if thermal.abs() > 0.4 {
        "POSSIBLE_STEEL".to_string()
    } else {
        "UNCLASSIFIED".to_string()
    }
}

/// Sensor weight configuration
pub struct SensorWeights {
    pub sar: f64,
    pub thermal: f64,
    pub optical: f64,
    pub swot: f64,
}

impl Default for SensorWeights {
    fn default() -> Self {
        Self {
            sar: 0.35,      // Best for heavy metal
            thermal: 0.30,  // Best for mass detection
            optical: 0.25,  // Best for aluminum
            swot: 0.10,     // Best for large displacement
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::Detection;
    // use crate::scanner::Classification; // Removed: no Classification in scanner
    use crate::coordinate::{UTMCoordinate, WGS84Coordinate};
    // use crate::scanner::Measurements; // Removed: Measurements does not exist in scanner
    
    #[test]
    fn test_aluminum_classification() {
        let detection = Detection {
            id: 1,
            score: 0.9,
            classification: "UNCLASSIFIED".to_string(),
            aluminum_ratio: 1.7,
            thermal_delta: 0.5,
            estimated_length_ft: 117.0,
            estimated_mass_tons: 10.0,
            pixel_row: 100,
            pixel_col: 200,
            utm_easting: 457990.7,
            utm_northing: 4702720.4,
            wgs84_lat: 42.4757,
            wgs84_lon: -87.5111,
            source_tile: "tile_001".to_string(),
            anchor_lock_offset: String::new(),
            grid_ref: String::from("16T 57990 27204"),
            utm: UTMCoordinate {
                easting: 457990.7,
                northing: 4702720.4,
                zone: 16,
            },
            wgs84: WGS84Coordinate {
                lat: 42.4757,
                lon: -87.5111,
            },
        };
        
        let result = classify(&detection);
        
        assert!(result.contains("ALUMINUM"));
    }
}
