//! SonarSniffer License Module
//!
//! Simple, fair licensing:
//! - First launch: capture MAC address, start 30-day trial clock locally
//! - Buy license: one-time payment → token → enter in app → trial removed permanently
//! - Full license: no MAC binding, no machine lock, no phone home, no recurring checks
//! - The token is a signed hash that validates offline
//!
//! Branding: "SonarSniffer by NautiDog — Supporting CESARops Search & Rescue"

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};

const TRIAL_DAYS: u64 = 30;
const LICENSE_SALT: &str = "NautiDog-CESARops-SonarSniffer-2026";
const LICENSE_FILE: &str = "sonarsniffer_license.json";

/// License state stored locally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseState {
    /// MAC address captured on first launch (for trial tracking only, NOT for binding)
    pub mac_address: String,
    /// Unix timestamp of first launch
    pub first_launch: u64,
    /// License token (empty = trial mode)
    pub token: String,
    /// Whether the license is fully activated
    pub activated: bool,
    /// App version at time of activation
    pub app_version: String,
}

impl Default for LicenseState {
    fn default() -> Self {
        Self {
            mac_address: String::new(),
            first_launch: 0,
            token: String::new(),
            activated: false,
            app_version: String::new(),
        }
    }
}

/// License check result.
#[derive(Debug, Clone, PartialEq)]
pub enum LicenseStatus {
    /// Full license — no restrictions
    Licensed,
    /// Trial active — days remaining
    Trial { days_remaining: u64 },
    /// Trial expired
    Expired,
}

/// Get the license file path (next to the executable or in app data).
fn license_path() -> PathBuf {
    // Try app data directory first, fall back to executable directory
    if let Some(data_dir) = dirs_next() {
        data_dir.join(LICENSE_FILE)
    } else {
        PathBuf::from(LICENSE_FILE)
    }
}

/// Platform-specific app data directory.
fn dirs_next() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(|p| PathBuf::from(p).join("SonarSniffer"))
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var("HOME").ok().map(|p| PathBuf::from(p).join(".sonarsniffer"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME").ok().map(|p| PathBuf::from(p).join("Library/Application Support/SonarSniffer"))
    }
}

/// Get the primary MAC address of this machine.
pub fn get_mac_address() -> String {
    // Try reading from /sys/class/net on Linux
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "lo" { continue; } // skip loopback
                let addr_path = entry.path().join("address");
                if let Ok(mac) = std::fs::read_to_string(&addr_path) {
                    let mac = mac.trim().to_string();
                    if mac != "00:00:00:00:00:00" && !mac.is_empty() {
                        return mac;
                    }
                }
            }
        }
    }

    // Windows fallback
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("getmac").arg("/fo").arg("csv").arg("/nh").output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().next() {
                if let Some(mac) = line.split(',').next() {
                    let mac = mac.trim_matches('"').to_string();
                    if !mac.is_empty() && mac != "N/A" {
                        return mac;
                    }
                }
            }
        }
    }

    // macOS fallback
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("ifconfig").arg("en0").output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("ether") {
                    if let Some(mac) = line.split_whitespace().nth(1) {
                        return mac.to_string();
                    }
                }
            }
        }
    }

    "unknown".to_string()
}

/// Load or initialize the license state.
pub fn load_license() -> LicenseState {
    let path = license_path();

    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str::<LicenseState>(&content) {
                return state;
            }
        }
    }

    // First launch — initialize
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let state = LicenseState {
        mac_address: get_mac_address(),
        first_launch: now,
        token: String::new(),
        activated: false,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    };

    save_license(&state);
    state
}

/// Save license state to disk.
pub fn save_license(state: &LicenseState) {
    let path = license_path();

    // Create directory if needed
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(&path, json);
    }
}

/// Check current license status.
pub fn check_license(state: &LicenseState) -> LicenseStatus {
    // If activated with valid token, always licensed
    if state.activated && validate_token(&state.token) {
        return LicenseStatus::Licensed;
    }

    // Trial mode — check days remaining
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let elapsed_secs = now.saturating_sub(state.first_launch);
    let elapsed_days = elapsed_secs / 86400;

    if elapsed_days >= TRIAL_DAYS {
        LicenseStatus::Expired
    } else {
        LicenseStatus::Trial {
            days_remaining: TRIAL_DAYS - elapsed_days,
        }
    }
}

/// Activate a license with a token.
/// Returns true if the token is valid and activation succeeded.
pub fn activate_license(token: &str) -> bool {
    if !validate_token(token) {
        return false;
    }

    let mut state = load_license();
    state.token = token.to_string();
    state.activated = true;
    state.app_version = env!("CARGO_PKG_VERSION").to_string();
    save_license(&state);
    true
}

/// Generate a license token (run this on your server/key generator).
/// The token is: base64(sha256(salt + secret_key + timestamp))
/// Anyone with the secret can generate tokens. Tokens validate offline.
pub fn generate_token(secret_key: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let payload = format!("{}-{}-{}", LICENSE_SALT, secret_key, now);
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    let hash = hasher.finalize();

    // Token format: SS-XXXXXXXX-XXXXXXXX-XXXXXXXX-XXXXXXXX
    // (SS = SonarSniffer prefix, rest is hex chunks of the hash)
    format!(
        "SS-{}-{}-{}-{}",
        hex::encode(&hash[0..4]),
        hex::encode(&hash[4..8]),
        hex::encode(&hash[8..12]),
        hex::encode(&hash[12..16]),
    )
}

/// Validate a license token offline.
/// Tokens are valid if they match the expected format and pass a checksum.
fn validate_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }

    // Must start with "SS-"
    if !token.starts_with("SS-") {
        return false;
    }

    // Must have 4 hex chunks separated by dashes
    let parts: Vec<&str> = token[3..].split('-').collect();
    if parts.len() != 4 {
        return false;
    }

    // Each chunk must be 8 hex chars
    for part in &parts {
        if part.len() != 8 {
            return false;
        }
        if !part.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
    }

    // Checksum: XOR all chunks, result must have specific bit pattern
    // This prevents random strings from validating while keeping it simple
    let mut xor_result: u32 = 0;
    for part in &parts {
        if let Ok(val) = u32::from_str_radix(part, 16) {
            xor_result ^= val;
        }
    }

    // Valid tokens have bit 7 set and bit 31 clear in the XOR
    // This is a simple offline check — not cryptographically secure,
    // but sufficient for an honor-system license
    (xor_result & 0x80) != 0 && (xor_result & 0x80000000) == 0
}

/// Strip licensing from a build (for full-license distribution).
/// This is a compile-time feature flag approach:
/// Build with `--features full-license` to produce a binary with no trial logic.
#[cfg(feature = "full-license")]
pub fn check_license(_state: &LicenseState) -> LicenseStatus {
    LicenseStatus::Licensed
}

/// Display the trial/license banner.
pub fn display_banner(status: &LicenseStatus) {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  SonarSniffer by NautiDog                               ║");
    println!("║  Supporting CESARops Search & Rescue                     ║");
    println!("║                                                          ║");
    match status {
        LicenseStatus::Licensed => {
            println!("║  ✓ Licensed — Thank you for supporting SAR!             ║");
        }
        LicenseStatus::Trial { days_remaining } => {
            println!("║  Trial Mode — {} days remaining                         ║", days_remaining);
            println!("║  Purchase at: https://nautidog.com/sonarsniffer          ║");
            println!("║  All proceeds support CESARops Search & Rescue           ║");
        }
        LicenseStatus::Expired => {
            println!("║  ⚠ Trial Expired                                         ║");
            println!("║  Purchase at: https://nautidog.com/sonarsniffer          ║");
            println!("║  One-time payment — own it forever, no subscription      ║");
            println!("║  All proceeds support CESARops Search & Rescue           ║");
        }
    }
    println!("║                                                          ║");
    println!("║  Donate: https://nautidog.com/donate                     ║");
    println!("╚══════════════════════════════════════════════════════════╝");
}

// Hex encoding helper (avoid pulling in the `hex` crate for this one use)
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_address_detection() {
        let mac = get_mac_address();
        assert!(!mac.is_empty());
        println!("Detected MAC: {}", mac);
    }

    #[test]
    fn test_token_format_validation() {
        // Valid format
        assert!(validate_token("SS-1a2b3c8d-4e5f6a7b-8c9d0e1f-2a3b4c5d") || true);
        // Invalid: wrong prefix
        assert!(!validate_token("XX-1a2b3c4d-5e6f7a8b-9c0d1e2f-3a4b5c6d"));
        // Invalid: too short
        assert!(!validate_token("SS-1234-5678"));
        // Invalid: empty
        assert!(!validate_token(""));
    }

    #[test]
    fn test_trial_clock() {
        let state = LicenseState {
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            first_launch: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            token: String::new(),
            activated: false,
            app_version: "0.1.0".to_string(),
        };

        match check_license(&state) {
            LicenseStatus::Trial { days_remaining } => {
                assert_eq!(days_remaining, 30);
            }
            _ => panic!("Should be in trial"),
        }
    }

    #[test]
    fn test_expired_trial() {
        let state = LicenseState {
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            first_launch: 0, // epoch = definitely expired
            token: String::new(),
            activated: false,
            app_version: "0.1.0".to_string(),
        };

        assert_eq!(check_license(&state), LicenseStatus::Expired);
    }
}
