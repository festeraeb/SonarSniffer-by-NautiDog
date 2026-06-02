# integrate/unmapped/laptopdump_programming_root/cesarops_agent_runner.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/cesarops_agent_runner.rs

## Rust source
```rust
//! CESAROPS Agent Runner
//! Designed for Agent Execution & Parameter Tuning.
//! Fires multiple sensors, fuses results, outputs GeoJSON map, pushes to DB.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

/// Agent configuration for sensor execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Geographic areas to probe
    pub areas: HashMap<String, AreaConfig>,
    /// List of sensors to run
    pub sensors: Vec<String>,
    /// Sensor-specific thresholds
    pub thresholds: Thresholds,
    /// Execution behavior flags
    pub execution: ExecutionConfig,
}

/// Geographic area configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AreaConfig {
    /// Bounding box coordinates [west, south, east, north]
    pub bbox: [f64; 4],
    /// Human-readable label
    pub label: String,
}

/// Sensor-specific threshold values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    pub thermal_zscore: f64,
    pub sar_coherence: f64,
    pub glint_ratio_b08_b04: f64,
    pub swot_ssh_m: f64,
}

/// Execution behavior configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    /// Switch to CPU if GPU fails
    pub cpu_fallback: bool,
    /// Maximum minutes per sensor run
    pub timeout_min: u32,
    /// Stop on first error if true
    pub fail_fast: bool,
}

/// Default agent configuration
const DEFAULT_AGENT_CONFIG: AgentConfig = AgentConfig {
    areas: HashMap::from([
        ("lake_michigan_south".to_string(), AreaConfig {
            bbox: [-88.0, 42.0, -87.0, 43.0],
            label: "Lake MI South (Zion Trench/Andaste)".to_string(),
        }),
        ("lake_superior".to_string(), AreaConfig {
            bbox: [-91.0, 46.5, -84.5, 48.0],
            label: "Lake Superior (Deep Basin)".to_string(),
        }),
        ("lake_erie".to_string(), AreaConfig {
            bbox: [-83.5, 41.5, -82.0, 42.5],
            label: "Lake Erie (Argo/Leak Survey)".to_string(),
        }),
    ]),
    sensors: vec![
        "thermal".to_string(),
        "nir_swir".to_string(),
        "sar".to_string(),
        "swot".to_string(),
    ],
    thresholds: Thresholds {
        thermal_zscore: 2.5,
        sar_coherence: 0.6,
        glint_ratio_b08_b04: 1.5,
        swot_ssh_m: 0.015,
    },
    execution: ExecutionConfig {
        cpu_fallback: true,
        timeout_min: 30,
        fail_fast: false,
    },
};

/// Sensor tool script mapping
const SENSOR_SCRIPTS: &[(&str, &str)] = &[
    ("thermal", "hard_pixel_audit.py"),
    ("nir_swir", "cesarops_engine.py"),
    ("sar", "lake_michigan_scan.py"),
    ("swot", "swot_ssh_extractor.py"),
];

/// Sensor execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorResult {
    pub sensor: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Default for SensorResult {
    fn default() -> Self {
        SensorResult {
            sensor: String::new(),
            status: String::new(),
            output_dir: None,
            reason: None,
            error: None,
        }
    }
}

/// Parse command line arguments
pub fn parse_args() -> (Option<String>, Option<Vec<String>>, bool) {
    let args: Vec<String> = env::args().collect();
    let mut area: Option<String> = None;
    let mut sensors: Option<Vec<String>> = None;
    let mut dry_run = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--area" => {
                i += 1;
                if i < args.len() {
                    area = Some(args[i].clone());
                }
            }
            "--sensors" => {
                i += 1;
                if i < args.len() {
                    let sensor_list: Vec<String> = args[i]
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect();
                    sensors = Some(sensor_list);
                }
            }
            "--dry-run" => {
                dry_run = true;
            }
            _ => {}
        }
        i += 1;
    }

    (area, sensors, dry_run)
}

/// Execute a sensor tool
pub fn run_sensor_tool(
    sensor_name: &str,
    area_cfg: &AreaConfig,
    thresholds: &Thresholds,
    execution_cfg: &ExecutionConfig,
) -> SensorResult {
    let sensor_map = [
        (
            "thermal",
            "hard_pixel_audit.py",
            vec![
                "--area",
                &format!(
                    "{},{},{},{}",
                    area_cfg.bbox[0], area_cfg.bbox[1], area_cfg.bbox[2], area_cfg.bbox[3]
                ),
                "--zscore",
                &thresholds.thermal_zscore.to_string(),
                "--output",
                &format!("outputs/{}", sensor_name),
            ],
        ),
        (
            "nir_swir",
            "cesarops_engine.py",
            vec![
                "--bands",
                "B11,B12,B08A",
                "--bbox",
                &format!(
                    "{},{},{},{}",
                    area_cfg.bbox[0], area_cfg.bbox[1], area_cfg.bbox[2], area_cfg.bbox[3]
                ),
                "--output",
                &format!("outputs/{}", sensor_name),
            ],
        ),
        (
            "sar",
            "lake_michigan_scan.py",
            vec![
                "--mode",
                "sar_only",
                "--bbox",
                &format!(
                    "{},{},{},{}",
                    area_cfg.bbox[0], area_cfg.bbox[1], area_cfg.bbox[2], area_cfg.bbox[3]
                ),
                "--coherence",
                &thresholds.sar_coherence.to_string(),
                "--output",
                "outputs/sar",
            ],
        ),
        (
            "swot",
            "swot_ssh_extractor.py",
            vec![
                "--bbox",
                &format!(
                    "{},{},{},{}",
                    area_cfg.bbox[0], area_cfg.bbox[1], area_cfg.bbox[2], area_cfg.bbox[3]
                ),
                "--threshold",
                &thresholds.swot_ssh_m.to_string(),
                "--output",
                "outputs/swot",
            ],
        ),
    ];

    let (script, args) = match sensor_map.iter().find(|(name, _, _)| name == sensor_name) {
        Some((_, script, args)) => (script, args),
        None => {
            log(&format!("⚠️  Unknown sensor: {}", sensor_name));
            return SensorResult {
                sensor: sensor_name.to_string(),
                status: "skipped".to_string(),
                reason: Some("unknown".to_string()),
                ..Default::default()
            };
        }
    };

    let script_path = Path::new(script);

    if !script_path.exists() {
        log(&format!("🔍 Script not found: {}", script_path.display()));
        return SensorResult {
            sensor: sensor_name.to_string(),
            status: "missing".to_string(),
            reason: Some(format!("script not found: {}", script_path.display())),
            ..Default::default()
        };
    }

    // Environment override for CPU fallback
    let mut env = env::vars().collect();
    if execution_cfg.cpu_fallback {
        env.insert("CUPY_CUDA_PATH".to_string(), "".to_string());
    }

    let cmd = Command::new("python3")
        .args(&[script_path.to_str().unwrap()] & args)
        .envs(env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok();

    match cmd {
        Some(mut child) => {
            let output_dir = Path::new(&format!("outputs/{}", sensor_name));

            let result = child.wait_with_output().unwrap();

            if result.status.success() {
                log(&format!("✅ {} SUCCESS", sensor_name));
                SensorResult {
                    sensor: sensor_name.to
