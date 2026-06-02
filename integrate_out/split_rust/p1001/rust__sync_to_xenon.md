# integrate/unmapped/laptopdump_wreckhunter_build/sync_to_xenon.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/sync_xenon.rs

## Rust source
```rust
//! Sync CESAROPS database connector files to Xenon server.
//!
//! This module provides functionality to copy database connector files
//! from the local filesystem to a remote Xenon server using SCP.
//!
//! # Example
//!
//! ```
//! use cesarops_inference::integrate::sync_xenon;
//!
//! let config = sync_xenon::XenonConfig {
//!     user: "cesarops".to_string(),
//!     host: "10.0.0.55".to_string(),
//!     path: "~/cesarops-wreckhunter-build".to_string(),
//! };
//!
//! sync_xenon::sync_to_xenon(&config, &["database_connector.py"])?;
//! ```

use std::process::Command;
use std::path::Path;
use std::io::{self, Write};

/// Configuration for Xenon server connection.
#[derive(Debug, Clone)]
pub struct XenonConfig {
    pub user: String,
    pub host: String,
    pub path: String,
}

impl Default for XenonConfig {
    fn default() -> Self {
        Self {
            user: "cesarops".to_string(),
            host: "10.0.0.55".to_string(),
            path: "~/cesarops-wreckhunter-build".to_string(),
        }
    }
}

/// Expand a path that may start with `~`.
fn expand_path(path: &str) -> String {
    if path.starts_with("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        path.replace("~/", &home)
    } else {
        path.to_string()
    }
}

/// Default files to sync.
pub const DEFAULT_FILES: &[&str] = &[
    "database_connector.py",
    "cesarops_comprehensive_schema.sql",
    "init_database.py",
];

/// Sync database connector files to Xenon server.
///
/// # Arguments
///
/// * `config` - Xenon server configuration.
/// * `files` - List of files to sync. If empty, uses DEFAULT_FILES.
///
/// # Returns
///
/// * `Ok(())` if all files synced successfully.
/// * `Err(String)` if any file failed to sync.
pub fn sync_to_xenon(config: &XenonConfig, files: &[&str]) -> Result<(), String> {
    let expanded_path = expand_path(&config.path);
    let mut errors = Vec::new();

    for file in files {
        let src = Path::new(file);
        if !src.exists() {
            eprintln!("⚠ Skipping {} (not found)", file);
            continue;
        }

        eprintln!("Syncing {}...", file);

        let remote_path = format!("{}/{}", expanded_path, file);
        let cmd = format!(
            "scp \"{}\" {}
