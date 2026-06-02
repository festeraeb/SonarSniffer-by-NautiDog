// Coordinate system - UTM-16T with grid references
// Pure Rust implementation

use serde::{Deserialize, Serialize};

const GRID_CELL_SIZE_M: u32 = 2000;
const GRID_PREFIX: &str = "WH2K";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchArea {
    pub lat_min: f64,
    pub lon_min: f64,
    pub lat_max: f64,
    pub lon_max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridCell {
    pub grid_ref: String,
    pub center_utm: UTMCoordinate,
    pub center_wgs84: WGS84Coordinate,
    pub bounds: GridBounds,
    pub distance_from_shore_miles: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UTMCoordinate {
    pub easting: f64,
    pub northing: f64,
    pub zone: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WGS84Coordinate {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridBounds {
    pub easting_min: f64,
    pub easting_max: f64,
    pub northing_min: f64,
    pub northing_max: f64,
}

/// Convert WGS84 to UTM-16T
pub fn wgs84_to_utm(lat: f64, lon: f64) -> (f64, f64, u8) {
    // Use proj crate if available, otherwise approximation
    // This is a simplified version - use proj::Proj for production
    
    // Zone 16 central meridian: -87°
    let zone = 16u8;
    let central_meridian = -87.0;

    // Simplified Transverse Mercator (use proj for accuracy)
    let k0 = 0.9996;

    // Approximate conversion
    let easting = 500000.0 + (lon - central_meridian) * 111320.0 * lat.cos();
    let northing = lat * 111320.0 * k0;
    
    (easting, northing, zone)
}

/// Convert UTM-16T to WGS84
pub fn utm_to_wgs84(easting: f64, northing: f64) -> (f64, f64) {
    // Simplified inverse conversion (use proj for production)
    let central_meridian = -87.0;
    let k0 = 0.9996;
    
    let lat = northing / (111320.0 * k0);
    let lon = central_meridian + (easting - 500000.0) / (111320.0 * lat.cos());
    
    (lat, lon)
}

/// Generate grid reference: WH2K-XXXX-YYYY
pub fn get_grid_reference(easting: f64, northing: f64, cell_size: u32) -> String {
    let grid_e = (easting / cell_size as f64) as u32;
    let grid_n = (northing / cell_size as f64) as u32;
    format!("{}-{:04}-{:04}", GRID_PREFIX, grid_e, grid_n)
}

/// Generate search grid
pub fn generate_grid(area: &SearchArea, cell_size: u32) -> Vec<GridCell> {
    let (easting_min, northing_min, _) = wgs84_to_utm(area.lat_min, area.lon_min);
    let (easting_max, northing_max, _) = wgs84_to_utm(area.lat_max, area.lon_max);
    
    let mut grid = Vec::new();
    
    let start_e = (easting_min / cell_size as f64) as u32;
    let end_e = (easting_max / cell_size as f64) as u32;
    let start_n = (northing_min / cell_size as f64) as u32;
    let end_n = (northing_max / cell_size as f64) as u32;
    
    for e in start_e..=end_e {
        for n in start_n..=end_n {
            let center_e = (e as f64 + 0.5) * cell_size as f64;
            let center_n = (n as f64 + 0.5) * cell_size as f64;
            
            let (lat, lon) = utm_to_wgs84(center_e, center_n);
            
            // Approximate distance from Chicago shoreline
            let shore_lat = 41.8781;
            let shore_lon = -87.6298;
            let distance_miles = haversine_miles(shore_lat, shore_lon, lat, lon);
            
            grid.push(GridCell {
                grid_ref: get_grid_reference(center_e, center_n, cell_size),
                center_utm: UTMCoordinate {
                    easting: center_e,
                    northing: center_n,
                    zone: 16,
                },
                center_wgs84: WGS84Coordinate { lat, lon },
                bounds: GridBounds {
                    easting_min: e as f64 * cell_size as f64,
                    easting_max: (e + 1) as f64 * cell_size as f64,
                    northing_min: n as f64 * cell_size as f64,
                    northing_max: (n + 1) as f64 * cell_size as f64,
                },
                distance_from_shore_miles: Some(distance_miles),
            });
        }
    }
    
    grid
}

/// Haversine distance in miles
fn haversine_miles(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 3959.0; // Earth radius in miles
    
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    
    let c = 2.0 * a.sqrt().asin();
    
    R * c
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_grid_reference() {
        let grid_ref = get_grid_reference(457990.7, 4702720.4, 2000);
        assert_eq!(grid_ref, "WH2K-0228-2351");
    }
    
    #[test]
    fn test_utm_conversion() {
        let (easting, northing, zone) = wgs84_to_utm(42.4757, -87.5111);
        assert_eq!(zone, 16);
        assert!(easting > 450000.0 && easting < 470000.0);
        assert!(northing > 4690000.0 && northing < 4710000.0);
    }
}
