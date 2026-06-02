use serde::{Deserialize, Serialize};
use std::process::Command;

/// Describes what a GPU worker can do and its current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerManifest {
    pub worker_id: String,
    pub hostname: String,
    pub gpu_name: String,
    pub vram_total_mb: u64,
    pub vram_free_mb: u64,
    pub specialty: String,
    pub model_loaded: Option<String>,
    pub tools: Vec<String>,
    pub safe_mode: bool,
    pub status: String,
    pub capabilities: Vec<String>,
}

impl Default for WorkerManifest {
    fn default() -> Self {
        Self {
            worker_id: uuid::Uuid::new_v4().to_string(),
            hostname: hostname::get()
                .map(|h| h.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "unknown".to_string()),
            gpu_name: "Unknown".to_string(),
            vram_total_mb: 0,
            vram_free_mb: 0,
            specialty: "general".to_string(),
            model_loaded: None,
            tools: vec![
                "read_file".to_string(),
                "write_file".to_string(),
                "run_command".to_string(),
                "think_harder".to_string(),
                "cargo_check".to_string(),
                "remember".to_string(),
                // SymForge-compatible coding surface (proxy/local hybrid)
                "search_symbols".to_string(),
                "get_symbol".to_string(),
                "get_file_context".to_string(),
                "search_text".to_string(),
                "replace_symbol_body".to_string(),
                "edit_within_symbol".to_string(),
                "insert_symbol".to_string(),
                "delete_symbol".to_string(),
                "batch_edit".to_string(),
                "batch_rename".to_string(),
                "scan_region".to_string(),
                "magnetic_dipole_detect".to_string(),
                "download_satellite_window".to_string(),
                "weather_window".to_string(),
                "detection_health".to_string(),
                "detection_scan".to_string(),
                "detection_poll".to_string(),
                "sat_mission".to_string(),
                "sat_read_mission_report".to_string(),
            ],
            safe_mode: false,
            status: "idle".to_string(),
            capabilities: vec![
                "rust_code_generation".to_string(),
                "file_operations".to_string(),
                "shell_execution".to_string(),
                "web_search".to_string(),
                "memory_persistence".to_string(),
                "model_inference".to_string(),
            ],
        }
    }
}

/// Parse nvidia-smi output to extract GPU info.
pub fn parse_nvidia_smi(output: &str) -> (String, u64, u64) {
    let mut gpu_name = "Unknown".to_string();
    let mut total_mb: u64 = 0;
    let mut free_mb: u64 = 0;

    for line in output.lines() {
        // Try to find GPU name from "| 0: Tesla P100-PCIE..."
        if line.contains("Tesla") || line.contains("GeForce") || line.contains("RTX") {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 2 {
                let name_part = parts[1].trim();
                if !name_part.is_empty() && name_part != "--" {
                    gpu_name = name_part.to_string();
                }
            }
        }
        // Look for memory lines like "|   0%  3456M / 16276M |"
        if line.contains("M /") && line.contains("|") {
            let parts: Vec<&str> = line.split('|').collect();
            for part in parts {
                let trimmed = part.trim();
                if trimmed.contains("M /") {
                    let mem_parts: Vec<&str> = trimmed.split('/').collect();
                    if mem_parts.len() == 2 {
                        let used_string = mem_parts[0].trim().replace('M', "");
                        let total_string = mem_parts[1].trim().replace('M', "");
                        let used_str = used_string.trim();
                        let total_str = total_string.trim();
                        if let Ok(used) = used_str.parse::<u64>() {
                            if let Ok(tot) = total_str.parse::<u64>() {
                                free_mb = tot.saturating_sub(used);
                                total_mb = tot;
                            }
                        }
                    }
                }
            }
        }
    }

    (gpu_name, total_mb, free_mb)
}

/// Read nvidia-smi and return the worker manifest.
pub fn discover_self(specialty: &str, model_loaded: Option<String>) -> WorkerManifest {
    let mut manifest = WorkerManifest::default();
    manifest.specialty = specialty.to_string();
    manifest.model_loaded = model_loaded;

    // Try to get GPU info from nvidia-smi
    let output = Command::new("nvidia-smi")
        .arg("--query-gpu=name,memory.total,memory.used")
        .arg("--format=csv,noheader,nounits")
        .output();

    match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let (gpu_name, total_mb, _) = parse_nvidia_smi(&stdout);
            manifest.gpu_name = gpu_name;
            manifest.vram_total_mb = total_mb;
            // Estimate free VRAM as 70% of total when idle
            manifest.vram_free_mb = (total_mb as f64 * 0.7) as u64;
        }
        Err(_) => {
            tracing::warn!("Could not run nvidia-smi, using defaults");
            manifest.vram_total_mb = 16384; // Assume 16GB P100
            manifest.vram_free_mb = 11469; // ~70%
        }
    }

    manifest
}
