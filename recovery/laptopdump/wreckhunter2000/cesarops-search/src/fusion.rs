// Multi-sensor fusion classification
// Each sensor has a specialty - combine for best results

use crate::scanner::Detection;

/// Classify target using multi-sensor fusion
pub fn classify(detection: &Detection) -> String {
    let score = detection.score;
    let aluminum = detection.aluminum_ratio;
    let thermal = detection.thermal_delta;
    
    if aluminum > 1.5 && thermal.abs() > 0.3 {
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
    use crate::{Detection, UTMCoordinate, WGS84Coordinate, Measurements};
    
    #[test]
    fn test_aluminum_classification() {
        let detection = Detection {
            grid_ref: "WH2K-0228-2351".to_string(),
            utm: UTMCoordinate { easting: 457990.7, northing: 4702720.4, zone: 16 },
            wgs84: WGS84Coordinate { lat: 42.4757, lon: -87.5111 },
            measurements: Measurements {
                sar_vv_vh_ratio: Some(1.2),
                thermal_sink: Some(0.45),
                b08_b04_ratio: Some(1.68),
                swot_displacement_cm: Some(0.8),
            },
            classification: Classification {
                classification: "UNCLASSIFIED".to_string(),
                confidence: 0.0,
                reasons: vec![],
                best_sensor: "NONE".to_string(),
                requires_verification: false,
                priority: "LOW".to_string(),
            },
            estimated_length_ft: Some(117.0),
            priority: "LOW".to_string(),
            distance_from_shore_miles: Some(35.2),
            near_known_wreck: Some("FLIGHT_2501".to_string()),
        };
        
        let result = classify(&detection);
        
        assert!(result.classification.contains("ALUMINUM"));
        assert!(result.confidence >= 0.7);
        assert_eq!(result.best_sensor, "OPTICAL");
    }
}
