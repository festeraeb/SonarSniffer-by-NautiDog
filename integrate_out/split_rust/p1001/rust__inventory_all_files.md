# integrate/unmapped/laptopdump_wreckhunter_build/inventory_all_files.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/file_inventory.rs

## Rust source
```rust
//! File inventory utility for cesarops-inference
//! Categorizes files into core scripts, data files, archive candidates, and BAG files
//!
//! Usage: cargo run --bin file-inventory
//! Output: outputs/file_inventory.json

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Inventory result structure
#[derive(Debug, Clone, serde::Serialize)]
pub struct InventoryResult {
    pub timestamp: String,
    pub core_scripts: Vec<FileInfo>,
    pub scripts_folder: Vec<FileInfo>,
    pub documentation: Vec<FileInfo>,
    pub geotiffs: Vec<FileInfo>,
    pub bag_files: Vec<FileInfo>,
    pub large_dirs: Vec<DirectoryInfo>,
    pub unknown_py: Vec<FileInfo>,
    pub archive_candidates: Vec<FileInfo>,
}

/// File information
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileInfo {
    pub path: String,
    pub size_kb: f64,
}

/// Directory information
#[derive(Debug, Clone, serde::Serialize)]
pub struct DirectoryInfo {
    pub path: String,
    pub size_mb: f64,
}

/// Core scripts list
const CORE_SCRIPTS: &[&str] = &[
    "database_connector.py",
    "cesarops_engine.py",
    "cuda_test_kmz.py",
    "tpu_server.py",
    "live_feed_server.py",
    "three_tile_offset_analysis.py",
    "validate_detection.py",
    "deep_wreck_validation.py",
    "smart_daily_scan.py",
    "find_swot_dates.py",
    "prioritized_pull_v2.py",
];

/// Scripts folder paths
const SCRIPTS_FOLDER: &[&str] = &[
    "scripts/wipe_database.py",
    "scripts/inventory_geotiffs.py",
    "scripts/process_tiles.py",
    "scripts/check_xenon_cuda.py",
    "scripts/compare_runs.py",
    "scripts/populate_database.py",
    "scripts/validate_database.py",
];

/// Documentation files
const DOCUMENTATION: &[&str] = &[
    "FRESH_START_PLAN.md",
    "TODO_RECOVERY.md",
    "DATABASE_STATUS.md",
    "TOOL_INVENTORY.md",
    "CUDA_READY_TOOLS_INVENTORY.md",
    "MASTER_FORENSIC_LEDGER.md",
    "ASSUMPTIONS_REGISTRY.md",
    "ALTIMETRY_CONSTELLATION.md",
    "FULL_SPECTRUM_STRATEGY.md",
    "PRIORITIZED_PULL_GUIDE.md",
    "FILE_INVENTORY.md",
];

/// Large directory names to check
const LARGE_DIRS: &[&str] = &[
    "target",
    "tauri-app",
    "outputs",
    "wreckhunter2000/cesarops-search",
];

/// Minimum GeoTIFF size in bytes (100KB)
const MIN_GEO_TIFF_SIZE: u64 = 100 * 1024;

/// Minimum directory size in bytes (10MB)
const MIN_DIR_SIZE: u64 = 10 * 1024 * 1024;

/// Output file path
const OUTPUT_FILE: &str = "outputs/file_inventory.json";

/// Inventory all files in the current directory
pub fn inventory_files(root: &Path) -> InventoryResult {
    let mut inventory = InventoryResult {
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
        core_scripts: Vec::new(),
        scripts_folder: Vec::new(),
        documentation: Vec::new(),
        geotiffs: Vec::new(),
        bag_files: Vec::new(),
        large_dirs: Vec::new(),
        unknown_py: Vec::new(),
        archive_candidates: Vec::new(),
    };

    // Check core scripts
    for script in CORE_SCRIPTS {
        let path = root.join(script);
        if path.exists() {
            let size_kb = path.metadata().unwrap_or_default().len() as f64 / 1024.0;
            inventory.core_scripts.push(FileInfo {
                path: path.to_string_lossy().to_string(),
                size_kb: size_kb.round(),
            });
        }
    }

    // Check scripts folder
    let scripts_dir = root.join("scripts");
    if scripts_dir.exists() {
        for entry in fs::read_dir(&scripts_dir).unwrap_or_default() {
            let entry = entry.unwrap_or_default();
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "py") {
                let size_kb = path.metadata().unwrap_or_default().len() as f64 / 1024.0;
                inventory.scripts_folder.push(FileInfo {
                    path: path.to_string_lossy().to_string(),
                    size_kb: size_kb.round(),
                });
            }
        }
    }

    // Check documentation
    for doc in DOCUMENTATION {
        let path = root.join(doc);
        if path.exists() {
            let size_kb = path.metadata().unwrap_or_default().len() as f64 / 1024.0;
            inventory.documentation.push(FileInfo {
                path: path.to_string_lossy().to_string(),
                size_kb: size_kb.round(),
            });
        }
    }

    // Find GeoTIFFs
    for entry in fs::read_dir(root).unwrap_or_default() {
        let entry = entry.unwrap_or_default();
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("tif")) {
            // Skip geojson files
            if path.to_string_lossy().contains(".tif.geojson") {
                continue;
            }
            let metadata = path.metadata().unwrap_or_default();
            if metadata.len() > MIN_GEO_TIFF_SIZE {
                let size_mb = metadata.len() as f64 / (1024.0 * 1024.0);
                inventory.geotiffs.push(FileInfo {
                    path: path.to_string_lossy().to_string(),
                    size_kb: size_mb.round(),
                });
            }
        }
    }

    // Find BAG files
    for entry in fs::read_dir(root).unwrap_or_default() {
        let entry = entry.unwrap_or_default();
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("bag")) {
            let size_mb = path.metadata().unwrap_or_default().len() as f64 / (1024.0 * 1024.0);
            inventory.bag_files.push(FileInfo {
                path: path.to_string_lossy().to_string(),
                size_kb: size_mb.round(),
            });
        }
    }

    // Find large directories
    for dir_name in LARGE_DIRS {
        let dir_path = root.join(dir_name);
        if dir_path.exists() && dir_path.is_dir() {
            let total_size = dir_path
                .read_dir()
                .unwrap_or_default()
                .filter_map(|entry| {
                    let entry = entry.unwrap_or_default();
                    let path = entry.path();
                    if path.is_file() {
                        Some(path.metadata().unwrap_or_default().len())
                    } else {
                        None
                    }
                })
                .sum::<u64>();

            if total_size > MIN_DIR_SIZE {
                let size_mb = total_size as f64 / (1024.0 * 1024.0);
                inventory.large_dirs.push(DirectoryInfo {
                    path: dir_path.to_string_lossy().to_string(),
                    size_mb: size_mb.round(),
                });
            }
        }
    }

    // Find unknown Python files
    let known_py: HashSet<PathBuf> = CORE_SCRIPTS
        .iter()
        .map(|s| root.join(s))
        .chain(
            inventory
                .scripts_folder
                .iter()
                .map(|f| PathBuf::from(f.path.clone())),
        )
        .chain(
            inventory
                .documentation
                .iter()
                .map(|f| PathBuf::from(f.path.clone())),
        )
        .collect();

    for entry in fs::read_dir(root).unwrap_or_default() {
        let entry = entry.unwrap_or_default();
        let path = entry.path();
        if path.is_file()
            && path.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("py"))
            && !path.to_string_lossy().starts_with('.')
            && !known_py.contains(&path)
        {
            let size_kb = path.metadata().unwrap_or_default().len() as f64 / 1024.0;
            inventory.unknown_py.push(FileInfo {
                path: path.to_string_lossy().to_string(),
                size_kb: size_kb.round(),
            });
        }
    }

    // Find archive candidates (large files, old files, etc.)
    for entry in fs::read_dir(root).unwrap_or_default() {
        let entry = entry.unwrap_or_default();
        let path = entry.path();
        if path.is_file() {
            let metadata = path.metadata().unwrap_or_default();
            let size_mb = metadata.len() as f64 / (1024.0 * 1024.0);
            // Archive candidates: files > 100MB
            if size_mb > 100.0 {
                inventory.archive_candidates.push(FileInfo {
                    path: path.to_string_lossy().to_string(),
                    size_kb: size_mb.round(),
                });
            }
        }
    }

    inventory
}

/// Save inventory to JSON file
pub fn save_inventory(inventory: &InventoryResult, root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let output_path = root.join(OUTPUT_FILE);
    let output_dir = output_path.parent().unwrap_or(root);
    fs::create_dir_all(output_dir)?;

    let json = serde_json::to_string_pretty(inventory)?;
    let mut file = File::create(&output_path)?;
    file.write_all(json.as_bytes())?;

    Ok(())
}

/// Main function for CLI usage
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(".");
    let inventory = inventory_files(root);

    println!("=" * 70);
    println!("FILE INVENTORY");
    println!("=" * 70);
    println!();

    println!("Core Scripts: {}", inventory.core_scripts.len());
    for script in &inventory.core_scripts {
        println!("  [OK] {} ({:.1f} KB)", script.path, script.size_kb);
    }

    println!();
    println!("Scripts Folder: {}", inventory.scripts_folder.len());
    for script in &inventory.scripts_folder {
        println!("  [OK] {} ({:.1f} KB)", script.path, script.size_kb);
    }

    println!();
    println!("Documentation: {}", inventory.documentation.len());
    for doc in &inventory.documentation {
        println!("  [OK] {}", doc.path);
    }

    println!();
    println!("GeoTIFFs: {}", inventory.geotiffs.len());
    let total_geotiff_size = inventory
        .geotiffs
        .iter()
        .map(|f| f.size_kb as u64)
        .sum::<u64>();
    println!(
        "  Found: {} GeoTIFFs ({:.1f} MB total)",
        inventory.geotiffs.len(),
        total_geotiff_size as f64 / (1024.0 * 1024.0)
    );

    println!();
    println!("BAG Files: {}", inventory.bag_files.len());
    for bag in &inventory.bag_files {
        println!("  [OK] {} ({:.2} MB)", bag.path, bag.size_kb);
    }

    println!();
    println!("Large Directories: {}", inventory.large_dirs.len());
    for dir in &inventory.large_dirs {
        println!("  {}: {:.1f} MB", dir.path, dir.size_mb);
    }

    println!();
    println!("Unknown Python Files: {}", inventory.unknown_py.len());
    for py in &inventory.unknown_py {
        println!("  {}", py.path);
    }

    println!();
    println!("Archive Candidates: {}", inventory.archive_candidates.len());
    for arc in &inventory.archive_candidates {
        println!("  {}: {:.1f} MB", arc.path, arc.size_kb);
    }

    println!();
    println!("=" * 70);
    println!("INVENTORY SAVED: {}", output_path.display());
    println!("=" * 70);

    save_inventory(&inventory, root)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inventory_files() {
        let root = Path::new(".");
        let inventory = inventory_files(root);
        assert!(inventory.timestamp.len() > 0);
        assert!(inventory.core_scripts.len() >= 0);
    }

    #[test]
    fn test_save_inventory() {
        let root = Path::new(".");
        let inventory = InventoryResult {
            timestamp: "test".to_string(),
            core_scripts: vec![],
            scripts_folder: vec![],
            documentation: vec![],
            geotiffs: vec![],
            bag_files: vec![],
            large_dirs: vec![],
            unknown_py: vec![],
            archive_candidates: vec![],
        };
        let result = save_inventory(&inventory, root);
        assert!(result.is_ok());
    }
}
```

## Forge wire
- **Pipeline diagnostic tool**: Called from `pipeline/run_inventory.sh` when a pipeline run needs to audit file state before/after execution
- **CI/CD integration**: Added to `forge/verify_files.rs` as a pre-flight check to ensure core scripts exist before deployment
- **Debug mode**: Exposed via `forge/debug/inventory` endpoint to fetch current file inventory without running full pipeline

## Risks
- **Path resolution**: Script assumes current directory is root; needs explicit path handling in production
- **Large file scanning**: Reading all files for archive candidates could be slow on large repos; consider sampling
- **JSON serialization**: `serde_json` dependency adds ~10KB binary; acceptable for diagnostic tool but not core inference
- **Error handling**: File metadata failures silently skip files; should log warnings for missing metadata
- **Memory usage**: Collecting all file paths in memory could be problematic on very large repos; consider streaming output
