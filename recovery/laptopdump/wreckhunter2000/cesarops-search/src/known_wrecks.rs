// Known wreck database - Historical records for calibration
// NO SIMULATIONS - Only documented wrecks

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownWreck {
    pub id: String,
    pub name: String,
    pub wreck_type: WreckType,
    pub length_ft: f64,
    pub wingspan_ft: Option<f64>,
    pub lost_date: String,
    pub cause: String,
    pub location_note: String,
    pub utm_easting: f64,
    pub utm_northing: f64,
    pub depth_ft: f64,
    pub priority: Priority,
    pub victims: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WreckType {
    SteelFreighter,
    WhalebackFreighter,
    Schooner,
    AircraftAluminum,
    Submarine,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
}

/// Load known wreck database
pub fn load_known_wrecks() -> Vec<KnownWreck> {
    vec![
        KnownWreck {
            id: "SS_CHICORAH".to_string(),
            name: "SS Chicorah".to_string(),
            wreck_type: WreckType::SteelFreighter,
            length_ft: 438.0,
            wingspan_ft: None,
            lost_date: "1913-09-11".to_string(),
            cause: "Collision".to_string(),
            location_note: "Southern Lake Michigan".to_string(),
            utm_easting: 445000.0,
            utm_northing: 4695000.0,
            depth_ft: 280.0,
            priority: Priority::High,
            victims: None,
        },
        KnownWreck {
            id: "SS_ANCASTE".to_string(),
            name: "SS Andaste".to_string(),
            wreck_type: WreckType::WhalebackFreighter,
            length_ft: 310.0,
            wingspan_ft: None,
            lost_date: "1907-08-17".to_string(),
            cause: "Collision with steamer Cuba".to_string(),
            location_note: "Zion Trench".to_string(),
            utm_easting: 457990.7,
            utm_northing: 4702720.4,
            depth_ft: 180.0,
            priority: Priority::High,
            victims: None,
        },
        KnownWreck {
            id: "FLIGHT_2501".to_string(),
            name: "Flight 2501 (DC-4)".to_string(),
            wreck_type: WreckType::AircraftAluminum,
            length_ft: 113.0,
            wingspan_ft: Some(117.0),
            lost_date: "1959-09-21".to_string(),
            cause: "Storm/Unknown".to_string(),
            location_note: "Last radar 42.99N, 88.12W (93.5 miles from shore)".to_string(),
            utm_easting: 408500.0,
            utm_northing: 4760050.0,
            depth_ft: 300.0,
            priority: Priority::Critical,
            victims: Some(58),
        },
        KnownWreck {
            id: "SS_WISCONSIN".to_string(),
            name: "SS Wisconsin".to_string(),
            wreck_type: WreckType::SteelFreighter,
            length_ft: 438.0,
            wingspan_ft: None,
            lost_date: "1913-09-11".to_string(),
            cause: "Collision".to_string(),
            location_note: "Near Chicorah".to_string(),
            utm_easting: 428500.0,
            utm_northing: 4735000.0,
            depth_ft: 250.0,
            priority: Priority::Medium,
            victims: None,
        },
        KnownWreck {
            id: "MV_PRINS_WILLEM_V".to_string(),
            name: "MV Prins Willem V".to_string(),
            wreck_type: WreckType::SteelFreighter,
            length_ft: 390.0,
            wingspan_ft: None,
            lost_date: "1968-11-25".to_string(),
            cause: "Storm".to_string(),
            location_note: "Southern Lake Michigan".to_string(),
            utm_easting: 431200.0,
            utm_northing: 4712000.0,
            depth_ft: 300.0,
            priority: Priority::Medium,
            victims: None,
        },
        KnownWreck {
            id: "UC_97".to_string(),
            name: "UC-97 (German U-boat)".to_string(),
            wreck_type: WreckType::Submarine,
            length_ft: 185.0,
            wingspan_ft: None,
            lost_date: "1921".to_string(),
            cause: "Target practice (war prize)".to_string(),
            location_note: "20 miles off Chicago".to_string(),
            utm_easting: 420000.0,
            utm_northing: 4680000.0,
            depth_ft: 350.0,
            priority: Priority::High,
            victims: None,
        },
    ]
}

/// Get steel reference values from known vessels
pub fn get_steel_reference_values() -> SteelReference {
    SteelReference {
        thermal_sink_typical: 0.75,
        sar_vv_vh_typical: 0.65,
        b08_b04_typical: 1.15,
        calibration_source: "SS Wisconsin + MV Prins Willem V".to_string(),
        tolerance: 0.15,
    }
}

#[derive(Debug, Clone)]
pub struct SteelReference {
    pub thermal_sink_typical: f64,
    pub sar_vv_vh_typical: f64,
    pub b08_b04_typical: f64,
    pub calibration_source: String,
    pub tolerance: f64,
}
