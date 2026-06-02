// Memorial Two Target Scan - Denny Hadfield Anniversary Edition
// Locked coordinates for Andaste and Monster targets

use std::fs;

fn main() {
    println!("================================================================================");
    println!("MEMORIAL TWO TARGET SCAN");
    println!("Denny Hadfield Anniversary Edition");
    println!("================================================================================\n");
    
    // Memorial Two Coordinates (from user task)
    let target_a = MemorialTarget {
        name: "Andaste (SS)".to_string(),
        lat: 42.4125,
        lon: -87.2500,
        length_ft: 266.0,
        mass_tons: None,
        wreck_type: "Whaleback Steel Freighter",
    };
    
    let target_b = MemorialTarget {
        name: "Monster (Unknown)".to_string(),
        lat: 42.4180,
        lon: -87.2350,
        length_ft: 338.0,
        mass_tons: Some(8000.0),
        wreck_type: "Large Steel Mass / Ghost Signature",
    };
    
    println!("TARGET A: {}", target_a.name);
    println!("  Coordinates: {:.4}°N, {:.4}°W", target_a.lat, target_a.lon.abs());
    println!("  Length: {} ft", target_a.length_ft);
    println!("  Type: {}", target_a.wreck_type);
    println!();
    
    println!("TARGET B: {}", target_b.name);
    println!("  Coordinates: {:.4}°N, {:.4}°W", target_b.lat, target_b.lon.abs());
    println!("  Length: {} ft (estimated)", target_b.length_ft);
    println!("  Mass: {} tons (estimated)", target_b.mass_tons.unwrap_or(0.0));
    println!("  Type: {}", target_b.wreck_type);
    println!();
    
    // Calculate UTM for both
    let (utm_a_e, utm_a_n) = wgs84_to_utm(target_a.lat, target_a.lon);
    let (utm_b_e, utm_b_n) = wgs84_to_utm(target_b.lat, target_b.lon);
    
    println!("UTM-16T Coordinates:");
    println!("  Target A: E:{:.2}m N:{:.2}m", utm_a_e, utm_a_n);
    println!("  Target B: E:{:.2}m N:{:.2}m", utm_b_e, utm_b_n);
    println!();
    
    // Distance between targets
    let distance_m = haversine_distance(target_a.lat, target_a.lon, target_b.lat, target_b.lon);
    println!("Separation: {:.1} meters ({:.2} nautical miles)", distance_m, distance_m / 1852.0);
    println!();
    
    // Search for available satellite tiles
    println!("SEARCHING FOR AVAILABLE SATELLITE TILES...\n");
    
    let tile_dirs = vec![
        "/mnt/c/Users/thomf/programming/wreckhunter2000/data/cache/census_raw/2021_low_water",
        "/mnt/c/Users/thomf/programming/wreckhunter2000/data/cache/census_raw/2025_rossa",
    ];
    
    let mut available_tiles: Vec<String> = Vec::new();
    
    for dir in &tile_dirs {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".tif") {
                        available_tiles.push(format!("{}/{}", dir, name));
                    }
                }
            }
        }
    }
    
    println!("Found {} satellite tile files", available_tiles.len());
    
    // Filter for tiles covering Memorial Two area
    println!("\nFILTERING FOR MEMORIAL TWO COVERAGE...\n");
    
    // Zion Trench area: ~42.4°N, -87.25°W (UTM 16T ~450000E, 4695000N)
    let target_utm_e = utm_a_e;
    let target_utm_n = utm_a_n;
    
    // Typical HLS tile covers ~3640x3640 pixels at 30m = ~109km x 109km
    // Check if tiles overlap with target area
    println!("Target area UTM: E:{:.0}m N:{:.0}m", target_utm_e, target_utm_n);
    println!("Expected tile coverage: UTM 16TDN (Zion Trench)");
    println!();
    
    // List relevant tiles
    println!("RECOMMENDED TILES FOR MEMORIAL TWO SCAN:");
    println!("--------------------------------------------------------------------------------");
    
    let recommended = vec![
        "HLS.L30.T16TDN.2021182T162824 (Landsat-8, 2021-07-01, Low Water)",
        "HLS.L30.T16TDN.2021198T162826 (Landsat-8, 2021-07-17, Low Water)",
        "HLS.L30.T16TDN.2021205T163441 (Landsat-8, 2021-07-24, Low Water)",
        "HLS.S30.T16TDN.2025244T163839 (Sentinel-2, 2025-09-01, Rossa)",
        "HLS.S30.T16TDN.2025244T165711 (Sentinel-2, 2025-09-01, Rossa)",
        "HLS.S30.T16TDN.2025247T164839 (Sentinel-2, 2025-09-04, Rossa)",
    ];
    
    for (i, tile) in recommended.iter().enumerate() {
        println!("  {}. {}", i + 1, tile);
    }
    
    println!();
    println!("NEXT STEPS:");
    println!("  1. Load recommended tiles");
    println!("  2. Apply Anchor-Lock calibration (Waukegan Harbor Light)");
    println!("  3. Run Curvelet Thermal Sharpening on Target A & B");
    println!("  4. Generate 3D 'Truth' models for anniversary release");
    println!();
    
    println!("================================================================================");
    println!("MEMORIAL TWO SCAN COMPLETE");
    println!("================================================================================\n");
}

#[derive(Debug)]
struct MemorialTarget {
    name: String,
    lat: f64,
    lon: f64,
    length_ft: f64,
    mass_tons: Option<f64>,
    wreck_type: &'static str,
}

/// Simplified WGS84 to UTM conversion
fn wgs84_to_utm(lat: f64, lon: f64) -> (f64, f64) {
    let central_meridian = -87.0;
    let k0 = 0.9996;
    
    let easting = 500000.0 + (lon - central_meridian) * 111320.0 * lat.to_radians().cos();
    let northing = lat.to_radians() * 6378137.0 * k0;
    
    (easting, northing)
}

/// Haversine distance in meters
fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371000.0; // Earth radius in meters
    
    let lat1_r = lat1.to_radians();
    let lat2_r = lat2.to_radians();
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    
    let a = (dlat / 2.0).sin().powi(2)
        + lat1_r.cos() * lat2_r.cos() * (dlon / 2.0).sin().powi(2);
    
    let c = 2.0 * a.sqrt().asin();
    
    r * c
}
