use std::process::Command;
use tracing::{info, warn};
use crate::types::Error;

/// Metrics collected from a single GPU via nvidia-smi
#[derive(Debug, Clone)]
pub struct GpuMetric {
    pub gpu_index: u32,
    pub utilization_pct: u32,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub temperature_c: u32,
    pub power_draw_w: u32,
}

/// Queries GPU metrics by parsing nvidia-smi CSV output.
/// Compatible with NVIDIA driver 580.x (Pascal legacy branch).
pub fn query_gpu_metrics() -> Result<Vec<GpuMetric>, Error> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .map_err(|e| Error::IoError(e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("nvidia-smi failed: {}", stderr);
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut metrics = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() >= 5 {
            let util = parts[0].parse::<u32>().unwrap_or(0);
            let mem_used = parts[1].parse::<u64>().unwrap_or(0);
            let mem_total = parts[2].parse::<u64>().unwrap_or(0);
            let temp = parts[3].parse::<u32>().unwrap_or(0);
            let power = parts[4].parse::<f32>().unwrap_or(0.0) as u32;

            let gpu_index = metrics.len() as u32;
            metrics.push(GpuMetric {
                gpu_index,
                utilization_pct: util,
                memory_used_mb: mem_used,
                memory_total_mb: mem_total,
                temperature_c: temp,
                power_draw_w: power,
            });
        }
    }

    info!("Queried {} GPUs via nvidia-smi", metrics.len());
    Ok(metrics)
}

/// Estimates register pressure by counting variable declarations in WGSL source.
/// P100 rule: >20 vars = warning, >32 = reject (occupancy drops).
pub fn estimate_register_pressure(wgsl_source: &str) -> u32 {
    let count = wgsl_source.lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("var ") || trimmed.starts_with("let ")
        })
        .count() as u32;
    count
}
