# integrate/unmapped/laptopdump_wreckhunter_build/anchor_lock_display.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/anchor_lock_display.rs

## Rust source
```rust
//! Anchor-Lock Harbor Light Network + Zion Trench Milled Numbers
//! Real data from 2021 Landsat-8 B10 thermal tile analysis
//!
//! This module provides anchor light network data and anomaly detection
//! parameters for the cesarops-inference pipeline.

use std::fmt;

/// Anchor light network data for Lake Michigan regions
#[derive(Debug, Clone)]
pub struct AnchorNetwork {
    pub region: String,
    pub lights: Vec<AnchorLight>,
}

/// Individual anchor light with metadata
#[derive(Debug, Clone)]
pub struct AnchorLight {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub tower_type: String,
    pub notes: String,
}

impl fmt::Display for AnchorLight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:30s} {:7.4f}N {:8.4f}W  {:15s} {}",
            self.name, self.latitude, self.longitude, self.tower_type, self.notes
        )
    }
}

/// Zion Trench target site data
#[derive(Debug, Clone)]
pub struct TargetSite {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub length_ft: u32,
    pub typ: String,
    pub notes: String,
}

impl fmt::Display for TargetSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}:", self.name)?;
        writeln!(f, "  Coordinates:  {:.4f}N, {:.4f}W", self.latitude, self.longitude)?;
        writeln!(f, "  Length:       {} ft", self.length_ft)?;
        writeln!(f, "  Type:         {}", self.typ)?;
        writeln!(f, "  Notes:        {}", self.notes)?;
        Ok(())
    }
}

/// Anomaly detection result from thermal analysis
#[derive(Debug, Clone)]
pub struct Anomaly {
    pub anomaly_number: u32,
    pub row: u32,
    pub col: u32,
    pub pixel_count: u32,
    pub z_score: f64,
    pub notes: String,
}

impl fmt::Display for Anomaly {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let z_display = format!("{:+.2}", self.z_score);
        write!(
            f,
            "  {:2d}  {:3d}  {:4d}  {:5d}  {:6d}  {:>7s}  {}",
            self.anomaly_number, self.row, self.col, self.pixel_count, self.pixel_count, z_display, self.notes
        )
    }
}

/// Zion Trench analysis configuration
#[derive(Debug, Clone)]
pub struct ZionTrenchConfig {
    pub tile_id: String,
    pub sensor: String,
    pub date: String,
    pub resolution_m: u32,
    pub dimensions: (u32, u32),
    pub coverage: String,
    pub z_threshold: f64,
    pub zion_constant: f64,
    pub depth_threshold_ft: u32,
    pub two_date_tolerance_m: u32,
}

impl fmt::Display for ZionTrenchConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "TILE: {}", self.tile_id)?;
        writeln!(f, "  Sensor:       {}", self.sensor)?;
        writeln!(f, "  Date:         {}", self.date)?;
        writeln!(f, "  Resolution:   {}m/pixel", self.resolution_m)?;
        writeln!(f, "  Dimensions:   {} x {} pixels", self.dimensions.0, self.dimensions.1)?;
        writeln!(f, "  Coverage:     {}", self.coverage)?;
        writeln!(f, "DETECTION PARAMETERS:")?;
        writeln!(f, "  Thermal Z-Score Threshold:  {:.1} sigma", self.z_threshold)?;
        writeln!(f, "  Zion Constant (depth):      {:.2}x", self.zion_constant)?;
        writeln!(f, "  Depth Threshold:            {} ft", self.depth_threshold_ft)?;
        writeln!(f, "  Two-Date Alignment:         {}m tolerance", self.two_date_tolerance_m)?;
        Ok(())
    }
}

/// Main anchor lock display module
pub fn display_anchor_lock_network() -> Result<(), Box<dyn std::error::Error>> {
    let anchors = vec![
        AnchorNetwork {
            region: "WISCONSIN".to_string(),
            lights: vec![
                AnchorLight {
                    name: "North Point Light".to_string(),
                    latitude: 43.0642,
                    longitude: -87.8728,
                    tower_type: "Steel Tower".to_string(),
                    notes: "Milwaukee entrance".to_string(),
                },
                AnchorLight {
                    name: "Wind Point Light".to_string(),
                    latitude: 42.7997,
                    longitude: -87.8181,
                    tower_type: "Brick Tower".to_string(),
                    notes: "Oldest WI 1880".to_string(),
                },
                AnchorLight {
                    name: "Sheboygan Breakwater".to_string(),
                    latitude: 43.7636,
                    longitude: -87.6856,
                    tower_type: "Steel Pierhead".to_string(),
                    notes: "Sheboygan harbor".to_string(),
                },
            ],
        },
        AnchorNetwork {
            region: "MICHIGAN".to_string(),
            lights: vec![
                AnchorLight {
                    name: "Grand Haven Pierhead".to_string(),
                    latitude: 43.0636,
                    longitude: -86.2544,
                    tower_type: "Steel Tower".to_string(),
                    notes: "Coast Guard City".to_string(),
                },
                AnchorLight {
                    name: "Holland Harbor (Big Red)".to_string(),
                    latitude: 42.7786,
                    longitude: -86.2064,
                    tower_type: "Steel Frame".to_string(),
                    notes: "Iconic".to_string(),
                },
                AnchorLight {
                    name: "Muskegon Breakwater".to_string(),
                    latitude: 43.2544,
                    longitude: -86.2706,
                    tower_type: "Steel Tower".to_string(),
                    notes: "Muskegon entrance".to_string(),
                },
                AnchorLight {
                    name: "St. Joseph North Pier".to_string(),
                    latitude: 42.1103,
                    longitude: -86.4864,
                    tower_type: "Steel Tower".to_string(),
                    notes: "Twin lights".to_string(),
                },
            ],
        },
        AnchorNetwork {
            region: "ILLINOIS".to_string(),
            lights: vec![
                AnchorLight {
                    name: "Chicago Harbor Light".to_string(),
                    latitude: 41.8897,
                    longitude: -87.6047,
                    tower_type: "Steel Caisson".to_string(),
                    notes: "Breakwater".to_string(),
                },
                AnchorLight {
                    name: "Waukegan Harbor".to_string(),
                    latitude: 42.3636,
                    longitude: -87.8036,
                    tower_type: "Steel Tower".to_string(),
                    notes: "ZION REFERENCE".to_string(),
                },
                AnchorLight {
                    name: "Evanston Light".to_string(),
                    latitude: 42.0503,
                    longitude: -87.6686,
                    tower_type: "Steel Skeleton".to_string(),
                    notes: "Northwestern".to_string(),
                },
            ],
        },
        AnchorNetwork {
            region: "INDIANA".to_string(),
            lights: vec![
                AnchorLight {
                    name: "Michigan City East Pier".to_string(),
                    latitude: 41.7136,
                    longitude: -86.8864,
                    tower_type: "Steel Tower".to_string(),
                    notes: "Active harbor".to_string(),
                },
                AnchorLight {
                    name: "Gary Breakwater".to_string(),
                    latitude: 41.6136,
                    longitude: -87.3036,
                    tower_type: "Steel Skeleton".to_string(),
                    notes: "Industrial".to_string(),
                },
            ],
        },
    ];

    println!("{}", "=" * 80);
    println!("ANCHOR-LOCK HARBOR LIGHT NETWORK - LAKE MICHIGAN");
    println!("{}", "=" * 80);
    println!();

    let mut total = 0;
    for anchor in &anchors {
        println!("{} ({}) anchors:", anchor.region, anchor.lights.len());
        for light in &anchor.lights {
            let marker = if anchor.lights.iter().any(|l| l.notes.contains("ZION")) {
                " ***"
            } else {
                ""
            };
            println!("  {}{}", light, marker);
            total += 1;
        }
        println!();
    }

    println!("{}", "=" * 80);
    println!("TOTAL ANCHORS: {}", total);
    println!("{}", "=" * 80);
    println!();

    Ok(())
}

/// Display Zion Trench target sites
pub fn display_zion_trench_targets() -> Result<(), Box<dyn std::error::Error>> {
    let targets = vec![
        TargetSite {
            name: "Andaste (SS)".to_string(),
            latitude: 42.4125,
            longitude: -87.2500,
            length_ft: 266,
            typ: "Whaleback".to_string(),
            notes: "1929 storm, 25 casualties".to_string(),
        },
        TargetSite {
            name: "Monster (Unknown)".to_string(),
            latitude: 42.4180,
            longitude: -87.2350,
            length_ft: 343,
            typ: "Steel Freighter".to_string(),
            notes: "14,474 tons, 1929?".to_string(),
        },
        TargetSite {
            name: "Loading Boom (1925)".to_string(),
            latitude: 42.4137,
            longitude: -87.2488,
            length_ft: 117,
            typ: "Steel Structure".to_string(),
            notes: "Andaste refit".to_string(),
        },
    ];

    println!("{}", "=" * 80);
    println!("ZION TRENCH - TARGET SITES");
    println!("{}", "=" * 80);
    println!();

    for target in &targets {
        print!("{}", target);
        println!();
    }

    println!("{}", "=" * 80);
    println!();

    Ok(())
}

/// Display Zion Trench milled numbers analysis
pub fn display_zion_trench_milled_numbers() -> Result<(), Box<dyn std::error::Error>> {
    let config = ZionTrenchConfig {
        tile_id: "HLS.L30.T16TDN.2021182T162824.v2.0.B10.tif".to_string(),
        sensor: "Landsat-8 Thermal Infrared (B10)".to_string(),
        date: "2021-07-01 (Low Water window)".to_string(),
        resolution_m: 30,
        dimensions: (3660, 3660),
        coverage: "UTM Zone 16TDN (Zion Trench)".to_string(),
        z_threshold: 2.5,
        zion_constant: 1.47,
        depth_threshold_ft: 400,
        two_date_tolerance_m: 10,
    };

    println!("{}", "=" * 80);
    println!("ZION TRENCH - MILLED NUMBERS (Real 2021 Landsat B10)");
    println!("{}", "=" * 80);
    println!();

    print!("{}", config);
    println!();

    println!("ANOMALIES DETECTED: 196 total");
    println!();
    println!("TOP 10 BY Z-SCORE:");

    let anomalies = vec![
        Anomaly {
            anomaly_number: 26,
            row: 2241,
            col: 345,
            pixel_count: 58070,
            z_score: 2.81,
            notes: "Land/water boundary".to_string(),
        },
        Anomaly {
            anomaly_number: 190,
            row: 3490,
            col: 178,
            pixel_count: 70,
            z_score: -7.27,
            notes: "Cold water anomaly".to_string(),
        },
        Anomaly {
            anomaly_number: 160,
            row: 3449,
            col: 3604,
            pixel_count: 52018,
            z_score: -7.27,
            notes: "Deep water".to_string(),
        },
        Anomaly {
            anomaly_number: 81,
            row: 2480,
            col: 292,
            pixel_count: 1987,
            z_score: 2.78,
            notes: "Thermal mass".to_string(),
        },
        Anomaly {
            anomaly_number: 194,
            row: 3600,
            col: 3090,
            pixel_count: 190,
            z_score: -7.27,
            notes: "Deep trench".to_string(),
        },
        Anomaly {
            anomaly_number: 148,
            row: 2925,
            col: 243,
            pixel_count: 316,
            z_score: 2.73,
            notes: "Steel signature?".to_string(),
        },
        Anomaly {
            anomaly_number: 99,
            row: 2548,
            col: 463,
            pixel_count: 276,
            z_score: 2.77,
            notes: "Mass anomaly".to_string(),
        },
        Anomaly {
            anomaly_number: 173,
            row: 3391,
            col: 354,
            pixel_count: 153,
            z_score: 2.72,
            notes: "Structure?".to_string(),
        },
        Anomaly {
            anomaly_number: 6,
            row: 1984,
            col: 155,
            pixel_count: 736,
            z_score: 2.71,
            notes: "Thermal
