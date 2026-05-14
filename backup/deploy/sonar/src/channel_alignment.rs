//! Per-device channel alignment settings.
//!
//! After processing a sonar file the user can adjust flip / invert on each
//! channel.  Those settings are persisted to a JSON file keyed by a device
//! fingerprint (magic + firmware + channel set) so that subsequent files from
//! the same unit reuse them automatically.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::garmin_rsd_parser::{self, ParseResult};

// ── data types ────────────────────────────────────────────────────────────────

/// Alignment choices for a single channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAlignment {
    pub channel_id: u32,
    /// Human-readable role, e.g. "port_sidescan", "starboard_sidescan".
    pub role: String,
    /// Hardware generation, e.g. "uhd", "uhd2", "classic".
    pub generation: String,
    /// Reverse sample order (far-range ↔ near-range).
    pub flip: bool,
    /// Negate sample values (invert brightness).
    pub invert: bool,
}

/// All alignment settings for one device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAlignment {
    /// Human-readable description for the JSON file.
    pub description: String,
    /// Last filename processed with this device.
    pub last_file: String,
    /// ISO 8601 timestamp of last update.
    pub last_updated: String,
    /// Per-channel settings keyed by channel_id (stored as strings for JSON compat).
    pub channels: HashMap<String, ChannelAlignment>,
}

/// The top-level JSON cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlignmentCache {
    pub version: u32,
    pub devices: HashMap<String, DeviceAlignment>,
}

impl Default for AlignmentCache {
    fn default() -> Self {
        Self {
            version: 1,
            devices: HashMap::new(),
        }
    }
}

// ── fingerprint ───────────────────────────────────────────────────────────────

/// Compute a deterministic device fingerprint from parse metadata.
/// Two files from the same unit + transducer combo will produce the same hash.
pub fn device_fingerprint(parsed: &ParseResult) -> String {
    let mut h = Sha256::new();
    // Magic bytes
    h.update(parsed.parser_magic.as_bytes());
    // Firmware version
    if let Some(fw) = parsed.firmware_version {
        h.update(fw.to_le_bytes());
    }
    // Generation
    if let Some(ref gen) = parsed.detected_generation {
        h.update(format!("{:?}", gen).as_bytes());
    }
    // Sorted channel IDs (the *set* of channels, not per-ping)
    let mut ch_ids: Vec<u32> = parsed.channels.iter().map(|c| c.id).collect();
    ch_ids.sort();
    for id in &ch_ids {
        h.update(id.to_le_bytes());
    }
    format!("{:x}", h.finalize())[..16].to_string()
}

// ── auto-detect ───────────────────────────────────────────────────────────────

/// Build default alignment from the static channel map.
/// Garmin samples are orientation-normalized in the parser, so defaults here
/// keep `flip: false` unless the user explicitly overrides it.
pub fn auto_detect(parsed: &ParseResult) -> Vec<ChannelAlignment> {
    let mut out = Vec::new();
    for ch in &parsed.channels {
        if let Some((role, gen)) = garmin_rsd_parser::map_channel_info(ch.id) {
            // Only include sidescan + downscan channels (skip depth_temp)
            if !role.contains("sidescan") && !role.contains("downscan") {
                continue;
            }
            let flip = false;
            out.push(ChannelAlignment {
                channel_id: ch.id,
                role: role.to_string(),
                generation: gen.to_string(),
                flip,
                invert: false,
            });
        }
    }
    out
}

// ── cache I/O ─────────────────────────────────────────────────────────────────

fn cache_path(app_data: Option<&Path>) -> PathBuf {
    if let Some(dir) = app_data {
        dir.join("channel_alignment.json")
    } else {
        home_dir_fallback().join("channel_alignment.json")
    }
}

fn home_dir_fallback() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(|h| PathBuf::from(h).join(".sonarsniffer"))
        .unwrap_or_else(|_| PathBuf::from(".sonarsniffer"))
}

pub fn load_cache(app_data: Option<&Path>) -> AlignmentCache {
    let p = cache_path(app_data);
    match fs::read_to_string(&p) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => AlignmentCache::default(),
    }
}

pub fn save_cache(cache: &AlignmentCache, app_data: Option<&Path>) -> Result<(), String> {
    let p = cache_path(app_data);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create cache dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(cache).map_err(|e| format!("Serialize: {e}"))?;
    fs::write(&p, json).map_err(|e| format!("Write alignment cache: {e}"))?;
    Ok(())
}

// ── lookup / save helpers ─────────────────────────────────────────────────────

/// Look up saved alignment for a device.  Returns `None` if not yet saved.
pub fn lookup(fingerprint: &str, app_data: Option<&Path>) -> Option<Vec<ChannelAlignment>> {
    let cache = load_cache(app_data);
    cache.devices.get(fingerprint).map(|dev| {
        dev.channels.values().cloned().collect()
    })
}

/// Save alignment for a device, merging into the cache.
pub fn save(
    fingerprint: &str,
    description: &str,
    file_name: &str,
    alignments: Vec<ChannelAlignment>,
    app_data: Option<&Path>,
) -> Result<(), String> {
    let mut cache = load_cache(app_data);
    let mut ch_map = HashMap::new();
    for a in alignments {
        ch_map.insert(a.channel_id.to_string(), a);
    }
    cache.devices.insert(fingerprint.to_string(), DeviceAlignment {
        description: description.to_string(),
        last_file: file_name.to_string(),
        last_updated: chrono_now(),
        channels: ch_map,
    });
    save_cache(&cache, app_data)
}

fn chrono_now() -> String {
    // Simple ISO 8601 without chrono crate
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    format!("{}s-since-epoch", secs)
}

// ── resolve: saved settings win, then auto-detect fills gaps ──────────────────

/// Resolve alignment for a parsed file: load saved settings if available,
/// filling any missing channels from auto-detection.
pub fn resolve(parsed: &ParseResult, app_data: Option<&Path>) -> (String, Vec<ChannelAlignment>) {
    let fp = device_fingerprint(parsed);
    let auto = auto_detect(parsed);

    if let Some(saved) = lookup(&fp, app_data) {
        // Merge: saved settings take priority, auto fills gaps
        let mut map: HashMap<u32, ChannelAlignment> = HashMap::new();
        for a in auto {
            map.insert(a.channel_id, a);
        }
        for s in saved {
            map.insert(s.channel_id, s);
        }
        let mut merged: Vec<ChannelAlignment> = map.into_values().collect();
        merged.sort_by_key(|a| a.channel_id);
        (fp, merged)
    } else {
        (fp, auto)
    }
}
