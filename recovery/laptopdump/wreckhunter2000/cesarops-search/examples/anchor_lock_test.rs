// Anchor-Lock Calibration Test Program
// Tests the harbor light calibration system against existing HLS data

use cesarops_search::anchor_lock::{AnchorLockNetwork, AnchorLockProcessor};

fn main() {
    println!("================================================================================");
    println!("CESAROPS ANCHOR-LOCK CALIBRATION TEST");
    println!("Great Lakes Harbor Light Network");
    println!("================================================================================\n");
    
    // Initialize the anchor network
    let network = AnchorLockNetwork::new();
    
    // Print all available anchor points
    println!("AVAILABLE ANCHOR POINTS:");
    println!("--------------------------------------------------------------------------------");
    
    println!("\n🏛️  WISCONSIN (West Shore):");
    for anchor in &network.wisconsin {
        println!("   📍 {} ({})", anchor.name, anchor.structure_type);
        println!("      WGS84: {:.4}°N, {:.4}°W", anchor.wgs84.lat, -anchor.wgs84.lon);
        println!("      UTM-16T: E:{:.2} N:{:.2}", anchor.utm.easting, anchor.utm.northing);
        println!("      Notes: {}", anchor.notes);
        println!();
    }
    
    println!("\n🏛️  MICHIGAN (East Shore):");
    for anchor in &network.michigan {
        println!("   📍 {} ({})", anchor.name, anchor.structure_type);
        println!("      WGS84: {:.4}°N, {:.4}°W", anchor.wgs84.lat, -anchor.wgs84.lon);
        println!("      UTM-16T: E:{:.2} N:{:.2}", anchor.utm.easting, anchor.utm.northing);
        println!("      Notes: {}", anchor.notes);
        println!();
    }
    
    println!("\n🏛️  ILLINOIS (Southwest):");
    for anchor in &network.illinois {
        println!("   📍 {} ({})", anchor.name, anchor.structure_type);
        println!("      WGS84: {:.4}°N, {:.4}°W", anchor.wgs84.lat, -anchor.wgs84.lon);
        println!("      UTM-16T: E:{:.2} N:{:.2}", anchor.utm.easting, anchor.utm.northing);
        println!("      Notes: {}", anchor.notes);
        println!();
    }
    
    println!("\n🏛️  INDIANA (Southern):");
    for anchor in &network.indiana {
        println!("   📍 {} ({})", anchor.name, anchor.structure_type);
        println!("      WGS84: {:.4}°N, {:.4}°W", anchor.wgs84.lat, -anchor.wgs84.lon);
        println!("      UTM-16T: E:{:.2} N:{:.2}", anchor.utm.easting, anchor.utm.northing);
        println!("      Notes: {}", anchor.notes);
        println!();
    }
    
    // Simulate calibration with detected anchor positions
    // In production, these would come from analyzing the satellite imagery
    println!("\n================================================================================");
    println!("SIMULATED CALIBRATION TEST");
    println!("================================================================================\n");
    
    let mut processor = AnchorLockProcessor::new();
    
    // Simulate detecting anchors with slight offsets (as would happen with satellite drift)
    // These offsets simulate typical satellite georeferencing errors
    
    println!("Processing anchor detections from HLS tiles...\n");
    
    // Wisconsin anchor - detected with ~25m NE offset
    println!("1. Detecting Wind Point Light (WI)...");
    let _ = processor.calibrate_from_anchors(
        "Wind Point Light",
        -87.8178,  // detected (slightly east of actual)
        42.8000,   // detected (slightly north of actual)
    );
    
    // Michigan anchor - detected with ~15m SW offset
    println!("2. Detecting Holland Harbor Light (MI)...");
    let _ = processor.calibrate_from_anchors(
        "Holland Harbor Light",
        -86.2066,  // detected (slightly west)
        42.7784,   // detected (slightly south)
    );
    
    // Illinois anchor - detected with ~30m offset
    println!("3. Detecting Chicago Harbor Light (IL)...");
    let _ = processor.calibrate_from_anchors(
        "Chicago Harbor Light",
        -87.6044,  // detected
        41.8900,   // detected
    );
    
    // Indiana anchor - detected with ~20m offset
    println!("4. Detecting Michigan City East Pierhead Light (IN)...");
    let _ = processor.calibrate_from_anchors(
        "Michigan City East Pierhead Light",
        -86.8862,  // detected
        41.7138,   // detected
    );
    
    // Print the calibration report
    processor.print_report();
    
    // Show how to apply corrections
    println!("\n================================================================================");
    println!("APPLICATION EXAMPLE");
    println!("================================================================================\n");
    
    if let Some((avg_e, avg_n, avg_mag)) = processor.get_weighted_average_offset() {
        println!("For a detected target at:");
        let detected_e = 450000.0;
        let detected_n = 4700000.0;
        println!("  Detected UTM: E:{:.2} N:{:.2}", detected_e, detected_n);
        
        let corrected_e = detected_e - avg_e;
        let corrected_n = detected_n - avg_n;
        println!("  Corrected UTM: E:{:.2} N:{:.2}", corrected_e, corrected_n);
        println!("  Correction applied: ΔE:{:.2}m ΔN:{:.2}m", avg_e, avg_n);
        println!();
        println!("This correction accounts for satellite georeferencing drift and ensures");
        println!("submerged target coordinates are accurately georeferenced to the lake floor.");
    }
    
    println!("\n================================================================================");
    println!("HLS DATA COMPATIBILITY");
    println!("================================================================================\n");
    
    println!("HLS (Harmonized Landsat/Sentinel) Data Specifications:");
    println!("  • Landsat-8/9 (HLS.L30): 30m resolution, 16-day revisit");
    println!("  • Sentinel-2 (HLS.S30): 20m resolution, 5-day revisit");
    println!("  • UTM Zone 16T coverage for Lake Michigan");
    println!("  • Bands available: B04 (Red), B05 (Red Edge), B08 (NIR), B10-11 (TIR), B12 (SWIR)");
    println!();
    println!("Anchor-Lock Application:");
    println!("  1. Identify harbor light structures in B08 (NIR) - high contrast");
    println!("  2. Extract pixel coordinates of known anchor points");
    println!("  3. Convert to UTM using HLS metadata");
    println!("  4. Calculate offset from known coordinates");
    println!("  5. Apply correction to all detected anomalies");
    println!();
    println!("Resolution Considerations:");
    println!("  • Landsat-8/9: 30m/pixel → ~15m geolocation accuracy expected");
    println!("  • Sentinel-2: 20m/pixel → ~10m geolocation accuracy expected");
    println!("  • Multi-anchor averaging improves accuracy to <10m");
    println!();
    
    println!("================================================================================");
    println!("NEXT STEPS");
    println!("================================================================================");
    println!("1. Load actual HLS tiles from /data/cache/census_raw/");
    println!("2. Run edge detection to find harbor light structures");
    println!("3. Match detected structures to anchor network");
    println!("4. Calculate real calibration offsets");
    println!("5. Apply to wreck/anomaly detection results");
    println!("================================================================================\n");
}
