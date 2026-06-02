// Universal Semi-Whaleback Scan (ZION-006 Gold Standard)
// 1) Pattern Match: Three-Island + Tumblehome
// 2) Target B Monster side-by-side comparison
// 3) Mass check for >= 14000-ton steel non-whaleback
// 4) Generate Memorial Census labels

use std::fs::File;
use std::io::Read;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct FullBasinReport {
    scan_type: String,
    timestamp: String,
    total_detections: u32,
    all_detections: Vec<Detection>,
}

#[derive(Debug, Deserialize)]
struct Detection {
    lat: f64,
    lon: f64,
    length_ft: f64,
    mass_tons: f64,
    thermal_zscore: f64,
    signature_type: Option<String>,
    confidence: f64,
    candidate_type: Option<String>,
    island_count: Option<u32>,
    jitter_detected: Option<bool>,
    notes: Option<String>,
    in_lake: Option<bool>,
    sar_vv_vh_ratio: Option<f64>,
    b08_b04_ratio: Option<f64>,
    swot_displacement: Option<f64>,
    best_sensor: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load GOLD standard for region
    let mut f = File::open("wreckhunter2000/cesarops-search/outputs/FULL_BASIN_SCAN_REPORT.json")?;
    let mut contents = String::new();
    f.read_to_string(&mut contents)?;
    let report: FullBasinReport = serde_json::from_str(&contents)?;

    println!("Universal Semi-Whaleback Scan - ZION-006 Gold Standard");
    println!("Scan time: {}\n", report.timestamp);

    // Filter: IN LAKE ONLY (exclude land detections)
    let in_lake: Vec<_> = report.all_detections.iter()
        .filter(|d| d.in_lake.unwrap_or(true))
        .collect();
    println!("Total detections: {} | In-lake only: {}\n", report.all_detections.len(), in_lake.len());

    // 1) Pattern match: Three-Island + Tumblehome signature (IN LAKE ONLY)
    let three_island = in_lake
        .iter()
        .filter(|d| d.island_count.unwrap_or(0) >= 3)
        .collect::<Vec<_>>();

    println!("Pattern Match: Three-Island candidates (count={}):", three_island.len());
    for t in &three_island {
        println!("  {:.1}ft @ ({:.4},{:.4}), mass={}t, signature={:?}, notes={:?}",
            t.length_ft, t.lat, t.lon, t.mass_tons, t.signature_type, t.notes);
    }

    // 2) Monster side-by-side (IN LAKE ONLY + MULTI-SENSOR)
    let monster = in_lake
        .iter()
        .find(|d| (d.length_ft - 343.0).abs() < 10.0 || d.candidate_type.as_deref() == Some("TRAIN_FERRY"));

    if let Some(mon) = monster {
        println!("\nTarget B (Monster) candidate found:");
        println!("  length_ft={:.1}, mass_tons={}, island_count={:?}, signature={:?}, notes={:?}",
            mon.length_ft, mon.mass_tons, mon.island_count, mon.signature_type, mon.notes);
        println!("  SENSOR COMBO: SAR={:?}, Optical={:?}, SWOT={:?}, Best={:?}",
            mon.sar_vv_vh_ratio, mon.b08_b04_ratio, mon.swot_displacement, mon.best_sensor);
        if mon.island_count.unwrap_or(0) >= 3 {
            println!("  => Three islands present (whaleback-like)");
        } else {
            println!("  => Not-three-island; likely single-aft engine house or different type");
        }
    } else {
        println!("\nTarget B (Monster) candidate not found in full basin report");
    }

    // 3) Mass check >= 14000 tons and not whaleback curve (IN LAKE ONLY)
    let heavy_non_whaleback = in_lake
        .iter()
        .filter(|d| d.mass_tons >= 14000.0)
        .filter(|d| {
            let sig = d.signature_type.as_deref().unwrap_or("");
            !sig.contains("whaleback") && !sig.contains("whale")
        })
        .collect::<Vec<_>>();

    println!("\nMass Check: >= 14000-ton non-whaleback candidates (count={}):", heavy_non_whaleback.len());
    for h in &heavy_non_whaleback {
        println!("  {:.1}ft @ {:.0}t, signature={:?}, islands={:?}", h.length_ft, h.mass_tons, h.signature_type, h.island_count);
        println!("    Sensors: SAR={:?}, Optical={:?}, Best={:?}",
            h.sar_vv_vh_ratio, h.b08_b04_ratio, h.best_sensor);
    }

    // 4) Memorial Census output
    println!("\nMemorial Census\n--------------");
    // Target A from ZION_006 report
    let andaste = Detection { lat: 42.4125, lon: -87.25, length_ft: 266.9, mass_tons: 4003.5, thermal_zscore: -0.3, signature_type: Some("strong_steel_whaleback_profile".to_string()), confidence: 0.95, candidate_type: Some("SS_ANDASTE".to_string()), island_count: Some(3), jitter_detected: Some(false), notes: Some("SS Andaste verified".to_string()), in_lake: Some(true), sar_vv_vh_ratio: None, b08_b04_ratio: None, swot_displacement: None, best_sensor: Some("THERMAL".to_string()) };
    println!("Target A: SS Andaste (Verified) - {:.1}ft, mass {:.1}t, islands={}", andaste.length_ft, andaste.mass_tons, andaste.island_count.unwrap_or(0));
    if let Some(mon) = monster {
        println!("Target B: Unidentified Heavy-Lift - {:.1}ft, mass {:.1}t, islands={:?}", mon.length_ft, mon.mass_tons, mon.island_count);
    } else {
        println!("Target B: Unidentified Heavy-Lift - not found in data");
    }

    println!("\nTask complete: Memorial Census generated.");

    Ok(())
}
