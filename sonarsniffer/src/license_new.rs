use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Branding: SonarSniffer by NautiDog
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseStatus {
    Valid,
    Trial { days_remaining: u32 },
    Expired,
    FullLicense,
}

#[derive(Serialize, Deserialize, Debug)]
struct LicenseData {
    first_run_timestamp: u64,
    token: Option<String>,
    is_full_license: bool,
}

const APP_NAME: &str = "sonarsniffer";
const CONFIG_FILE: &str = "license.json";
const TRIAL_DAYS: u64 = 30;

/// Returns the path to ~/.sonarsniffer/license.json
fn get_config_path() -> PathBuf {
    let mut path = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push(APP_NAME);
    if !path.exists() {
        let _ = fs::create_dir_all(&path);
    }
    path.push(CONFIG_FILE);
    path
}

/// Loads the license data from disk or creates a new trial state
fn load_license_data() -> LicenseData {
    let path = get_config_path();
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(data) = serde_json::from_str::<LicenseData>(&content) {
            return data;
        }
    }

    // First run initialization
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let new_data = LicenseData {
        first_run_timestamp: now,
        token: None,
        is_full_license: false,
    };
    
    let _ = save_license_data(&new_data);
    new_data
}

fn save_license_data(data: &LicenseData) -> Result<(), String> {
    let path = get_config_path();
    let json = serde_json::to_string_pretty(data)
        .map_err(|e| format!("Serialization error: {}", e))?;
    fs::write(path, json)
        .map_err(|e| format!("File write error: {}", e))
}

/// Checks the current license status based on time and token
pub fn check_license() -> LicenseStatus {
    let data = load_license_data();

    if data.is_full_license && data.token.is_some() {
        return LicenseStatus::Valid;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let elapsed_secs = now.saturating_sub(data.first_run_timestamp);
    let elapsed_days = elapsed_secs / (24 * 3600);

    if elapsed_days >= TRIAL_DAYS {
        LicenseStatus::Expired
    } else {
        LicenseStatus::Trial {
            days_remaining: (TRIAL_DAYS - elapsed_days) as u32,
        }
    }
}

/// Validates and applies a token in the format: SS-XXXXXXXX-XXXXXXXX-XXXXXXXX-XXXXXXXX
pub fn activate_token(token: &str) -> Result<(), String> {
    // Validate format: SS- followed by 4 groups of 8 hex chars
    let parts: Vec<&str> = token.split('-').collect();
    if parts.len() != 5 || parts[0] != "SS" {
        return Err("Invalid token format. Expected SS-XXXXXXXX-XXXXXXXX-XXXXXXXX-XXXXXXXX".to_string());
    }

    for part in parts.iter().skip(1) {
        if part.len() != 8 || !part.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("Invalid token segment. Must be 8 hex characters.".to_string());
        }
    }

    let mut data = load_license_data();
    data.token = Some(token.to_string());
    data.is_full_license = true;

    save_license_data(&data)?;
    Ok(())
}
