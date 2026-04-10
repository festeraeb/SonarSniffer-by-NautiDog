//! License gating — 30-day trial + permanent unlock key.
//!
//! The permanent key is validated by comparing a SHA-256 hash.  The raw key
//! is never stored in the binary; only the digest is embedded.

use std::path::PathBuf;

use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// SHA-256("sonarsniffer-license-v1:<permanent_key>") — raw key is not in source.
const EXPECTED_KEY_HASH: &str =
    "8df52de3a661c6c96071a26e1b4e44e1d60907b05a6c74d640ef964d9f5b8725";
const LICENSE_SALT: &str = "sonarsniffer-license-v1";
const TRIAL_DAYS: i64 = 30;
const DEFAULT_CONTACT_EMAIL: &str = "support@nautidogsailing.com";

// ── Persisted license file ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct LicenseFile {
    /// ISO date (YYYY-MM-DD) of the very first launch.
    trial_start_utc: String,
    /// Set to true once a valid permanent key has been entered.
    unlocked: bool,
}

// ── Public return type sent to the frontend ───────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct LicenseStatus {
    /// `"trial"` | `"expired"` | `"unlocked"`
    pub state: String,
    /// Days left in trial; -1 when state is `"unlocked"`.
    pub days_remaining: i64,
    /// Email address shown to users for obtaining a permanent key.
    pub contact_email: String,
    /// Public builds require the user to enter a key after the trial expires.
    pub key_required: bool,
    /// Private builds bypass licensing entirely.
    pub private_build: bool,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Read (or initialise) the license file and return the current status.
pub fn check_license(app_data_dir: PathBuf) -> LicenseStatus {
    if is_private_build() {
        return LicenseStatus {
            state: "unlocked".to_string(),
            days_remaining: -1,
            contact_email: contact_email(),
            key_required: false,
            private_build: true,
        };
    }

    let path = license_path(&app_data_dir);

    let lf: LicenseFile = if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(new_trial_file)
    } else {
        // First launch — create license file.
        let f = new_trial_file();
        let _ = std::fs::create_dir_all(&app_data_dir);
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&f).unwrap_or_default());
        f
    };

    if lf.unlocked {
        return LicenseStatus {
            state: "unlocked".to_string(),
            days_remaining: -1,
            contact_email: contact_email(),
            key_required: false,
            private_build: false,
        };
    }

    let today = Utc::now().date_naive();
    let remaining = days_remaining(&lf.trial_start_utc, today);

    if remaining > 0 {
        LicenseStatus {
            state: "trial".to_string(),
            days_remaining: remaining,
            contact_email: contact_email(),
            key_required: false,
            private_build: false,
        }
    } else {
        LicenseStatus {
            state: "expired".to_string(),
            days_remaining: 0,
            contact_email: contact_email(),
            key_required: true,
            private_build: false,
        }
    }
}

/// Validate `key` and, if correct, mark the license as permanently unlocked.
pub fn activate_license(key: String, app_data_dir: PathBuf) -> Result<(), String> {
    if is_private_build() {
        return Ok(());
    }

    let salted = format!("{LICENSE_SALT}:{}", key.trim());
    let mut hasher = Sha256::new();
    hasher.update(salted.as_bytes());
    let digest = format!("{:x}", hasher.finalize());

    if digest != EXPECTED_KEY_HASH {
        return Err(
            "Invalid license key. Please contact NautiDog Sailing to obtain a key.".to_string(),
        );
    }

    let path = license_path(&app_data_dir);

    // Preserve existing trial_start date if the file already exists.
    let mut lf: LicenseFile = if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(new_trial_file)
    } else {
        new_trial_file()
    };

    lf.unlocked = true;
    let _ = std::fs::create_dir_all(&app_data_dir);
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&lf).unwrap_or_default(),
    )
    .map_err(|e| format!("Could not save license: {e}"))?;

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn is_private_build() -> bool {
    matches!(
        option_env!("SONARSNIFFER_PRIVATE_BUILD"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn contact_email() -> String {
    option_env!("SONARSNIFFER_LICENSE_EMAIL")
        .unwrap_or(DEFAULT_CONTACT_EMAIL)
        .to_string()
}

fn license_path(app_data_dir: &PathBuf) -> PathBuf {
    app_data_dir.join("license.json")
}

fn new_trial_file() -> LicenseFile {
    LicenseFile {
        trial_start_utc: Utc::now().date_naive().to_string(),
        unlocked: false,
    }
}

fn days_remaining(trial_start: &str, today: NaiveDate) -> i64 {
    let start = NaiveDate::parse_from_str(trial_start, "%Y-%m-%d").unwrap_or(today);
    let elapsed = (today - start).num_days();
    (TRIAL_DAYS - elapsed).max(0)
}
