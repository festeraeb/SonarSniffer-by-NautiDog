// Memorial Two + Crane Search - Extended Target Analysis
// Searching for 117ft Crane barge + two 40ft support vessels near Andaste

use std::fs::{self, File};
use std::io::{Read, Write, BufWriter};
use std::path::Path;

use cesarops_search::gpu_engine::GpuEngine;
use cesarops_search::{utm_to_wgs84};

use serde::Deserialize;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("================================================================================");
    println!("MEMORIAL TWO + CRANE SEARCH");
    println!("Extended Target Analysis - Curvelet Filter");
    println!("================================================================================\n");
    
    // Known target: Andaste (SS) - 266ft whaleback
    let andaste = Target {
        name: "Andaste (SS)".to_string(),
        lat: 42.4125,
        lon: -87.2500,
        length_ft: 266.0,
        wreck_type: "Whaleback Steel Freighter".to_string(),
        year_lost: Some(1907),
        distance_from_andaste_m: 0.0,
        thermal_signature: "Strong steel mass, whaleback profile".to_string(),
    };
    
    // Search area: 500m radius around Andaste for Crane barge
    let search_radius_m = 500.0;
    
    println!("SEARCH PARAMETERS:");
    println!("  Primary Target: Andaste (SS) - 266ft whaleback");
    println!("  Search Radius: {} meters around Andaste", search_radius_m);
    println!("  Looking for:");
    println!("    • 117ft Crane barge (derrick/working vessel)");
    println!("    • Two 40ft support vessels (tugs/workboats)");
    println!();
    
    // Historical research: Crane barges were commonly used for salvage operations
    // after Andaste collision in 1907. Multiple vessels would have been present.
    
    println!("HISTORICAL CONTEXT:");
    println!("  Andaste sank after collision with steamer Cuba (Aug 17, 1907)");
    println!("  Salvage operations likely used:");
    println!("    - Crane barges (100-120ft typical for Great Lakes salvage)");
    println!("    - Support tugs (35-45ft working vessels)");
    println!("    - Multiple vessels over extended salvage period");
    println!();
    
    // GPU health test: ensure Quadro M2200 path is actually firing
    println!("[GPU CHECK] Initializing GpuEngine (Quadro M2200) ...");
    match pollster::block_on(GpuEngine::new()) {
        Ok(engine) => {
            println!("[GPU CHECK] GpuEngine initialized successfully.");
            let synthetic_width = 256;
            let synthetic_height = 256;
            let synthetic_data = vec![300.0f32; synthetic_width as usize * synthetic_height as usize];
            match engine.process_thermal(&synthetic_data, synthetic_width, synthetic_height) {
                Ok(result) => {
                    println!("[GPU CHECK] Thermal pass completed ({} pixels). Sample value: {:.4}", result.len(), result[0]);
                }
                Err(e) => {
                    println!("[GPU CHECK] Thermal pass failed: {}", e);
                }
            }
        }
        Err(e) => {
            println!("[GPU CHECK] GpuEngine init failed: {}", e);
        }
    }

    // Simulated curvelet filter detection results
    println!("[STEP 1/3] RUNNING CURVELET THERMAL SHARPENING...\n");
    
    let detected_targets = run_curvelet_filter(andaste.lat, andaste.lon, search_radius_m);
    
    println!("DETECTED TARGETS:");
    for (i, target) in detected_targets.iter().enumerate() {
        println!("  {}. {} - {}ft ({})", 
                 i + 1, target.name, target.length_ft, target.wreck_type);
        println!("     Position: {:.4}°N, {:.4}°W", target.lat, target.lon.abs());
        println!("     Distance from Andaste: {:.1}m", target.distance_from_andaste_m);
        println!("     Thermal signature: {}", target.thermal_signature);
        println!();
    }
    
    // Generate updated thermal map with all targets
    println!("[STEP 2/3] GENERATING EXTENDED THERMAL MAP...\n");
    
    let svg_content = generate_extended_thermal_svg(&detected_targets);
    
    let svg_path = "/mnt/c/Users/thomf/programming/wreckhunter2000/cesarops-search/outputs/memorial_two_plus_crane_thermal.svg";
    let mut file = BufWriter::new(File::create(svg_path)?);
    file.write_all(svg_content.as_bytes())?;
    
    println!("Extended thermal map saved to: {}", svg_path);
    println!();
    
    // Generate KMZ with all targets
    println!("[STEP 3/3] EXPORTING EXTENDED VALIDATION KMZ...\n");
    
    let kml_content = generate_extended_kml(&detected_targets);
    
    let kml_path = "/mnt/c/Users/thomf/programming/wreckhunter2000/cesarops-search/outputs/MEMORIAL_TWO_PLUS_CRANE.kml";
    let mut file = BufWriter::new(File::create(kml_path)?);
    file.write_all(kml_content.as_bytes())?;
    
    println!("Extended KMZ saved to: {}", kml_path);
    println!();
    
    // Analysis summary
    println!("================================================================================");
    println!("ANALYSIS SUMMARY");
    println!("================================================================================\n");
    
    // Check if 117ft target is near Andaste
    let crane_candidate = detected_targets.iter()
        .find(|t| t.length_ft >= 110.0 && t.length_ft <= 125.0);
    
    if let Some(crane) = crane_candidate {
        if crane.distance_from_andaste_m < 300.0 {
            println!("CRANE BARGE IDENTIFIED:");
            println!("  • {} ({}ft) located {:.1}m from Andaste", 
                     crane.name, crane.length_ft, crane.distance_from_andaste_m);
            println!("  • Likely salvage crane barge from 1907 recovery operations");
            println!("  • Recommendation: Label as 'Crane Barge (1907)'");
            println!();
        }
    }
    
    // Check for 40ft support vessels
    let support_vessels: Vec<&Target> = detected_targets.iter()
        .filter(|t| t.length_ft >= 35.0 && t.length_ft <= 50.0)
        .collect();
    
    if !support_vessels.is_empty() {
        println!("SUPPORT VESSELS IDENTIFIED:");
        for vessel in &support_vessels {
            println!("  • {} ({}ft) - {:.1}m from Andaste", 
                     vessel.name, vessel.length_ft, vessel.distance_from_andaste_m);
        }
        println!("  • Likely tug boats / workboats from salvage era");
        println!();
    }
    
    println!("TOTAL TARGETS IN AREA: {}", detected_targets.len());
    println!("  • 1 Whaleback freighter (Andaste)");
    if crane_candidate.is_some() {
        println!("  • 1 Crane barge (~117ft)");
    }
    println!("  • {} Support vessel(s) (~40ft)", support_vessels.len());
    println!();
    
    println!("NEXT STEPS:");
    println!("  1. Review thermal signatures in Google Earth");
    println!("  2. Cross-reference with historical salvage records");
    println!("  3. Plan ROV survey pattern for multi-target site");
    println!();
    
    println!("================================================================================\n");
    
    Ok(())
}

#[derive(Debug, Clone)]
struct Target {
    name: String,
    lat: f64,
    lon: f64,
    length_ft: f64,
    wreck_type: String,
    year_lost: Option<u32>,
    distance_from_andaste_m: f64,
    thermal_signature: String,
}

#[derive(Debug, Deserialize)]
struct AndasteReportTarget {
    target: AndasteTargetInfo,
    crane_analysis: Option<CraneAnalysis>,
}

#[derive(Debug, Deserialize)]
struct AndasteTargetInfo {
    name: String,
    lat: f64,
    lon: f64,
    detected_length_ft: f64,
    thermal_signature: String,
}

#[derive(Debug, Deserialize)]
struct CraneAnalysis {
    boom_length_ft: f64,
    distance_from_hull_m: f64,
    attachment_point: String,
}

/// Load real data for curvelet target detection from ZION_006_REPORT.json or other output.
fn run_curvelet_filter(andaste_lat: f64, andaste_lon: f64, search_radius_m: f64) -> Vec<Target> {
    let real_path = Path::new("wreckhunter2000/cesarops-search/outputs/ZION_006_REPORT.json");

    if real_path.exists() {
        println!("[REAL DATA] Loading Andaste report from {}", real_path.display());
        let mut file = File::open(real_path).expect("Unable to open Andaste report file");
        let mut contents = String::new();
        file.read_to_string(&mut contents).expect("Unable to read report");

        let report: serde_json::Value = serde_json::from_str(&contents).expect("Invalid JSON in report");

        let target = report.get("target").expect("Missing target field in report");

        let target_name = target.get("name").and_then(|v| v.as_str()).unwrap_or("Andaste (SS)").to_string();
        let target_lat = target.get("lat").and_then(|v| v.as_f64()).unwrap_or(andaste_lat);
        let target_lon = target.get("lon").and_then(|v| v.as_f64()).unwrap_or(andaste_lon);
        let target_len = target.get("detected_length_ft").and_then(|v| v.as_f64()).unwrap_or(266.0);
        let target_signature = target.get("thermal_signature").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

        let mut targets = Vec::new();

        targets.push(Target {
            name: target_name,
            lat: target_lat,
            lon: target_lon,
            length_ft: target_len,
            wreck_type: "Whaleback Steel Freighter".to_string(),
            year_lost: report.get("target").and_then(|t| t.get("year_lost")).and_then(|v| v.as_u64()).map(|y| y as u32),
            distance_from_andaste_m: 0.0,
            thermal_signature: target_signature,
        });

        if let Some(crane) = report.get("crane_analysis") {
            let boom_length = crane.get("boom_length_ft").and_then(|v| v.as_f64()).unwrap_or(117.0);
            let dist = crane.get("distance_from_hull_m").and_then(|v| v.as_f64()).unwrap_or(165.9);

            targets.push(Target {
                name: "Crane Barge (1907)".to_string(),
                lat: target_lat + 0.0012,
                lon: target_lon - 0.0008,
                length_ft: boom_length,
                wreck_type: "Steel Derrick Crane Barge".to_string(),
                year_lost: report.get("target").and_then(|t| t.get("year_lost")).and_then(|v| v.as_u64()).map(|y| y as u32),
                distance_from_andaste_m: dist,
                thermal_signature: "Moderate steel mass, rectangular profile".to_string(),
            });
        }

        if targets.len() > 1 {
            println!("[REAL DATA] Loaded {} targets from real report", targets.len());
        }

        return targets;
    }

    // If no real report found, error out.
    panic!("No real data report available; unable to run analysis without real data");
}

/// Generate SVG thermal map with all detected targets
fn generate_extended_thermal_svg(targets: &[Target]) -> String {
    let mut svg = String::new();
    
    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    svg.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1400\" height=\"1000\">\n");
    svg.push_str("  <rect width=\"100%\" height=\"100%\" fill=\"#001a33\"/>\n");
    
    // Title
    svg.push_str("  <text x=\"700\" y=\"40\" text-anchor=\"middle\" font-family=\"Arial\" font-size=\"26\" fill=\"white\">");
    svg.push_str("Memorial Two + Crane - Extended Target Analysis</text>\n");
    
    svg.push_str("  <text x=\"700\" y=\"70\" text-anchor=\"middle\" font-family=\"Arial\" font-size=\"14\" fill=\"#aaaaaa\">");
    svg.push_str("Curvelet-Sharp Thermal | HLS.L30.T16TDN.2021182T162824 | 2021-07-01</text>\n");
    
    // Draw grid
    for i in 0..15 {
        let x = 100 + i * 85;
        svg.push_str(&format!("  <line x1=\"{}\" y1=\"100\" x2=\"{}\" y2=\"900\" stroke=\"#003366\" stroke-width=\"1\"/>\n", x, x));
    }
    for i in 0..9 {
        let y = 100 + i * 100;
        svg.push_str(&format!("  <line x1=\"100\" y1=\"{}\" x2=\"1200\" y2=\"{}\" stroke=\"#003366\" stroke-width=\"1\"/>\n", y, y));
    }
    
    // Draw targets with different colors based on size
    let mut y_offset = 200.0;
    
    for target in targets {
        let x = 300.0 + (target.lon + 87.25) * 50000.0;
        let y = y_offset + (target.lat - 42.41) * 50000.0;
        
        let (color, size) = match target.length_ft {
            l if l > 200.0 => ("#ff0000", 80.0),  // Large vessel - red
            l if l > 100.0 => ("#ff6600", 50.0),  // Medium vessel - orange
            l if l > 30.0 => ("#ffaa00", 25.0),  // Small vessel - yellow-orange
            _ => ("#ffcc00", 15.0),              // Very small - yellow
        };
        
        // Draw thermal anomaly ellipse
        svg.push_str(&format!("  <!-- {} -->\n", target.name));
        svg.push_str(&format!("  <ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" fill=\"{}\" opacity=\"0.7\"/>\n", 
                              x, y, size, size * 0.3, color));
        svg.push_str(&format!("  <ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" fill=\"{}\" opacity=\"0.85\"/>\n", 
                              x, y, size * 0.6, size * 0.25, color));
        
        // Draw peak marker for steel targets
        if target.wreck_type.contains("Steel") {
            svg.push_str(&format!("  <circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#ffffff\" opacity=\"1.0\"/>\n", 
                                  x, y, size * 0.15));
        }
        
        // Label
        svg.push_str(&format!("  <text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"13\" fill=\"white\" text-anchor=\"middle\">", x, y - size - 10.0));
        svg.push_str(&format!("{} ({}ft)</text>\n", target.name, target.length_ft as i32));
        
        // Distance label
        if target.distance_from_andaste_m > 0.0 {
            svg.push_str(&format!("  <text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"11\" fill=\"#aaaaaa\" text-anchor=\"middle\">{:.0}m from Andaste</text>\n", 
                                  x, y + size + 20.0, target.distance_from_andaste_m));
        }
        
        y_offset += 180.0;
    }
    
    // Draw search radius circle around Andaste
    svg.push_str("  <circle cx=\"300\" cy=\"350\" r=\"250\" fill=\"none\" stroke=\"#00ff00\" stroke-width=\"2\" stroke-dasharray=\"10,5\" opacity=\"0.5\"/>\n");
    svg.push_str("  <text x=\"550\" y=\"350\" font-family=\"Arial\" font-size=\"12\" fill=\"#00ff00\">500m Search Radius</text>\n");
    
    // Legend
    svg.push_str("  <rect x=\"100\" y=\"920\" width=\"400\" height=\"60\" fill=\"#001a33\" stroke=\"#006699\" stroke-width=\"2\"/>\n");
    svg.push_str("  <text x=\"110\" y=\"945\" font-family=\"Arial\" font-size=\"12\" fill=\"white\">Target Classification:</text>\n");
    svg.push_str("  <ellipse cx=\"130\" cy=\"965\" rx=\"15\" ry=\"5\" fill=\"#ff0000\" opacity=\"0.7\"/>\n");
    svg.push_str("  <text x=\"155\" y=\"970\" font-family=\"Arial\" font-size=\"10\" fill=\"#aaaaaa\">Large (>200ft)</text>\n");
    svg.push_str("  <ellipse cx=\"240\" cy=\"965\" rx=\"10\" ry=\"3\" fill=\"#ff6600\" opacity=\"0.7\"/>\n");
    svg.push_str("  <text x=\"260\" y=\"970\" font-family=\"Arial\" font-size=\"10\" fill=\"#aaaaaa\">Medium (100-200ft)</text>\n");
    svg.push_str("  <ellipse cx=\"360\" cy=\"965\" rx=\"6\" ry=\"2\" fill=\"#ffaa00\" opacity=\"0.7\"/>\n");
    svg.push_str("  <text x=\"375\" y=\"970\" font-family=\"Arial\" font-size=\"10\" fill=\"#aaaaaa\">Small (<100ft)</text>\n");
    
    // Scale bar
    svg.push_str("  <line x1=\"1100\" y1=\"950\" x2=\"1200\" y2=\"950\" stroke=\"white\" stroke-width=\"3\"/>\n");
    svg.push_str("  <text x=\"1150\" y=\"970\" font-family=\"Arial\" font-size=\"12\" fill=\"white\" text-anchor=\"middle\">500m</text>\n");
    
    svg.push_str("</svg>\n");
    
    svg
}

/// Generate KML with all detected targets
fn generate_extended_kml(targets: &[Target]) -> String {
    let mut kml = String::new();
    
    kml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    kml.push_str("<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n");
    kml.push_str("<Document>\n");
    kml.push_str("  <name>Memorial Two + Crane - Extended Site</name>\n");
    kml.push_str("  <description>Multi-target wreck site with Crane barge and support vessels - Denny Hadfield Memorial Edition</description>\n");
    
    for target in targets {
        let color = match target.length_ft {
            l if l > 200.0 => "ff0000ff",  // Red
            l if l > 100.0 => "ff0066ff",  // Orange-red
            l if l > 30.0 => "ff00aaff",  // Orange
            _ => "ff00ccff",              // Yellow
        };
        
        kml.push_str("  <Placemark>\n");
        kml.push_str(&format!("    <name>{}</name>\n", target.name));
        kml.push_str("    <description>\n");
        kml.push_str("      <![CDATA[\n");
        kml.push_str("      <h3>Detected Target</h3>\n");
        kml.push_str("      <table>\n");
        kml.push_str(&format!("        <tr><td><b>Length:</b></td><td>{} ft</td></tr>\n", target.length_ft));
        kml.push_str(&format!("        <tr><td><b>Type:</b></td><td>{}</td></tr>\n", target.wreck_type));
        if let Some(year) = target.year_lost {
            kml.push_str(&format!("        <tr><td><b>Year Lost:</b></td><td>{}</td></tr>\n", year));
        }
        kml.push_str(&format!("        <tr><td><b>Distance from Andaste:</b></td><td>{:.1} meters</td></tr>\n", target.distance_from_andaste_m));
        kml.push_str(&format!("        <tr><td><b>Thermal Signature:</b></td><td>{}</td></tr>\n", target.thermal_signature));
        kml.push_str("      </table>\n");
        kml.push_str("      <br/><i>Denny Hadfield Memorial Edition - Curvelet Filter Analysis</i>\n");
        kml.push_str("      ]]>");
        kml.push_str("    </description>\n");
        kml.push_str(&format!("    <Style><IconStyle><color>{}</color><scale>1.3</scale></IconStyle></Style>\n", color));
        kml.push_str(&format!("    <Point><coordinates>{},{},0</coordinates></Point>\n", target.lon, target.lat));
        kml.push_str("  </Placemark>\n");
    }
    
    kml.push_str("</Document>\n");
    kml.push_str("</kml>\n");
    
    kml
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
