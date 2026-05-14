use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HealingDiscovery {
    pub id: String,
    pub discovered_at: String,
    pub app_version: String,
    pub magic: u32,
    pub firmware_version: Option<u32>,
    pub generation: String,
    pub channel_ids: Vec<u32>,
    pub correction_type: String,
    pub original_interpretation: String,
    pub corrected_interpretation: String,
    pub records_parsed: usize,
    pub confidence: f32,
    pub file_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HealingCache {
    pub discoveries: Vec<HealingDiscovery>,
}

/// Compute a sha256-based fingerprint of the first N bytes of a file.
pub fn compute_file_fingerprint(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let preview = &data[..data.len().min(4096)];
    let mut hasher = DefaultHasher::new();
    preview.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Compute a deterministic discovery ID from key fields.
pub fn compute_discovery_id(d: &HealingDiscovery) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    d.magic.hash(&mut hasher);
    d.generation.hash(&mut hasher);
    d.correction_type.hash(&mut hasher);
    d.file_fingerprint.hash(&mut hasher);
    format!("hd-{:016x}", hasher.finish())
}

/// Record a healing discovery to the local cache.
pub fn record_discovery(
    discovery: HealingDiscovery,
    data_dir: Option<&Path>,
) -> Result<(), String> {
    let mut cache = load_cache(data_dir);
    // Deduplicate by id
    if !cache.discoveries.iter().any(|d| d.id == discovery.id) {
        cache.discoveries.push(discovery);
    }
    let path = cache_path(data_dir);
    let json = serde_json::to_string_pretty(&cache).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_cache(data_dir: Option<&Path>) -> HealingCache {
    let path = cache_path(data_dir);
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(cache) = serde_json::from_str(&data) {
                return cache;
            }
        }
    }
    HealingCache::default()
}

#[allow(dead_code)]
pub fn merge_community(
    healings: Vec<HealingDiscovery>,
    data_dir: Option<&Path>,
) -> Result<usize, String> {
    let mut cache = load_cache(data_dir);
    let count = healings.len();
    cache.discoveries.extend(healings);
    let path = cache_path(data_dir);
    let json = serde_json::to_string_pretty(&cache).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())?;
    Ok(count)
}

fn cache_path(data_dir: Option<&Path>) -> std::path::PathBuf {
    data_dir
        .map(|d| d.join("healing_cache.json"))
        .unwrap_or_else(|| "healing_cache.json".into())
}
