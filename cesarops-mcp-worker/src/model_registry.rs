use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;
use glob::glob;

/// Represents a discovered GGUF model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub path: String,
    pub size_mb: f64,
    pub specialty: String,
    pub quantization: String,
    pub estimated_vram_mb: u64,
}

impl ModelEntry {
    /// Infer the model's specialty from its filename.
    fn infer_specialty(filename: &str) -> &'static str {
        let lower = filename.to_lowercase();
        if lower.contains("coder") || lower.contains("dev") || lower.contains("rust") {
            return "coder";
        }
        if lower.contains("r1") || lower.contains("deepseek") || lower.contains("reasoner") {
            return "thinker";
        }
        "general"
    }

    /// Infer quantization type from filename.
    fn infer_quantization(filename: &str) -> String {
        let lower = filename.to_lowercase();
        if lower.contains("q2") { return "Q2_K".to_string(); }
        if lower.contains("q3") { return "Q3_K".to_string(); }
        if lower.contains("q4") { return "Q4_K_M".to_string(); }
        if lower.contains("q5") { return "Q5_K_M".to_string(); }
        if lower.contains("q6") { return "Q6_K".to_string(); }
        if lower.contains("q8") { return "Q8_0".to_string(); }
        if lower.contains("f16") || lower.contains("fp16") { return "F16".to_string(); }
        if lower.contains("bf16") { return "BF16".to_string(); }
        "unknown".to_string()
    }

    /// Estimate VRAM needed in MB based on model size and quantization.
    fn estimate_vram_mb(size_mb: f64, quantization: &str) -> u64 {
        // Rough estimate: model weights take ~size_mb * multiplier based on quant
        match quantization {
            "Q2_K" => (size_mb as u64).saturating_mul(1),
            "Q3_K" => (size_mb as u64).saturating_mul(1),
            "Q4_K_M" | "Q4_0" => (size_mb as u64).saturating_mul(1),
            "Q5_K_M" => (size_mb as u64).saturating_mul(1),
            "Q6_K" => (size_mb as u64).saturating_mul(1),
            "Q8_0" => (size_mb as u64).saturating_mul(2),
            "F16" => (size_mb as u64).saturating_mul(2),
            "BF16" => (size_mb as u64).saturating_mul(2),
            _ => (size_mb as u64).saturating_mul(2),
        }
    }
}

/// The model registry — holds all discovered models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRegistry {
    pub models: Vec<ModelEntry>,
}

impl ModelRegistry {
    /// Scan a directory for .gguf models.
    pub fn scan(base_dir: &Path) -> Self {
        Self { models: scan_models(base_dir) }
    }

    /// Select the best model for a task.
    pub fn select_model_for_task(&self, task_type: &str, available_vram_mb: u64) -> Option<&ModelEntry> {
        select_model_for_task(task_type, available_vram_mb, &self.models)
    }
}

/// Scan a directory for .gguf files and build a registry.
pub fn scan_models(base_dir: &Path) -> Vec<ModelEntry> {
    let pattern = format!("{}/**/*.gguf", base_dir.display());
    let mut entries = Vec::new();

    if let Ok(paths) = glob(&pattern) {
        for entry in paths.flatten() {
            if let Ok(metadata) = fs::metadata(&entry) {
                let size_bytes = metadata.len() as f64;
                let size_mb = size_bytes / (1024.0 * 1024.0);
                let filename = entry.file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();

                let quant = ModelEntry::infer_quantization(&filename);
                entries.push(ModelEntry {
                    path: entry.to_string_lossy().into_owned(),
                    size_mb,
                    specialty: ModelEntry::infer_specialty(&filename).to_string(),
                    quantization: quant.clone(),
                    estimated_vram_mb: ModelEntry::estimate_vram_mb(size_mb, &quant),
                });
            }
        }
    }

    entries.sort_by_key(|e| e.estimated_vram_mb);
    entries
}

/// Select the best model for a given task type and available VRAM.
/// Priority: exact specialty match > any match, smallest sufficient model.
pub fn select_model_for_task<'a>(
    task_type: &str,
    available_vram_mb: u64,
    registry: &'a [ModelEntry],
) -> Option<&'a ModelEntry> {
    let mut candidates: Vec<&ModelEntry> = registry.iter()
        .filter(|m| m.estimated_vram_mb <= available_vram_mb)
        .collect();

    // Prefer exact specialty match
    if !candidates.is_empty() {
        candidates.retain(|m| m.specialty == task_type);
    }

    // Pick smallest that fits
    candidates.into_iter().next()
}
