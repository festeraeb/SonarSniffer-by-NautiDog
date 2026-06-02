//! Mission orchestrator (T9 design / T10+T11 implementation, polished).
//!
//! Pipeline: OperatorScenario → ScenarioClass (heuristic) → MissionPlan
//! → retool cluster (free P100 VRAM) → fan out modules (POST /tool/{name})
//! → restore cluster → MissionReport.
//!
//! v1 is heuristic-driven: the keyword classifier emits one of three known
//! mission templates from T9 (WreckHunt, DownedAircraft, SearchRescue). LLM
//! refinement of the plan is a v2 hook — for now we save credits and stay
//! deterministic. All execution happens through the existing `/tool/{name}`
//! route on the forge itself, which means the orchestrator inherits every
//! detection-service / SAR / aeromagnetic / scan-region tool already wired.

use axum::Json;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Config — keep loosely coupled. Forge serves itself on 9100; cluster_config
// is the source of truth for worker names.
// ---------------------------------------------------------------------------

const MODULE_TIMEOUT_DEFAULT_SECS: u64 = 120;
const PLAN_RETRY_CAP: u32 = 1;

fn forge_base() -> String {
    std::env::var("FORGE_URL").unwrap_or_else(|_| "http://127.0.0.1:9100".to_string())
}

/// Per-tool timeouts — sat_mission can run for hours; detection poll is quick.
fn module_timeout_secs(tool: &str) -> u64 {
    match tool {
        "sat_mission" => 7200,
        "download_satellite_window" => 1800,
        "detection_scan" => 600,
        "detection_poll" => 120,
        "magnetic_dipole_detect" => 600,
        _ => MODULE_TIMEOUT_DEFAULT_SECS,
    }
}

/// Intake brain pool — probed in order from cluster_config [endpoint_pool.intake].
/// Falls back to cesarops2 draft (1070) → thinker (2060) → T440 P100s.
fn load_intake_pool() -> Vec<String> {
    let path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    if let Some(pool) = table.get("endpoint_pool").and_then(|v| v.as_table()) {
        if let Some(intake) = pool.get("intake").and_then(|v| v.as_table()) {
            if let Some(urls) = intake.get("urls").and_then(|v| v.as_array()) {
                let parsed: Vec<String> = urls
                    .iter()
                    .filter_map(|u| u.as_str().map(String::from))
                    .collect();
                if !parsed.is_empty() {
                    return parsed;
                }
            }
        }
    }
    vec![
        "http://10.0.0.201:5571".into(),
        "http://10.0.0.200:5571".into(),
        "http://10.0.0.201:5200".into(),
        "http://10.0.0.200:5200".into(),
        "http://127.0.0.1:5002".into(),
        "http://127.0.0.1:5001".into(),
    ]
}

async fn probe_llama_endpoint(client: &Client, base: &str) -> bool {
    for path in ["/v1/models", "/health", "/api/v1/model"] {
        if client
            .get(format!("{}{}", base.trim_end_matches('/'), path))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

/// P100 endpoints the orchestrator may need to verify-clear before running a
/// tile-compute mission. Names must match cluster_config.toml [[worker]] entries.
const P100_WORKERS: &[(&str, u16)] = &[
    ("GemmaBig", 5001),
    ("QwenBig", 5002),
];

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScenarioClass {
    WreckHunt,
    DownedAircraft,
    SearchRescue,
    EnvironmentalMonitoring,
    OceanographicSurvey,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DelegateType {
    /// Local Rust+wgpu tile compute (P100 preferred).
    VulkanGpu,
    /// Local CPU SIMD path.
    Cpu,
    /// Coral USB TPU int8 inference.
    CoralTpuInt8,
    /// Hybrid path that may switch primary/secondary at runtime.
    Hybrid,
    /// Remote LLM endpoint (1060/1070/P1000).
    LlmRemote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorScenario {
    pub raw_text: String,
    #[serde(default = "default_priority")]
    pub priority: u8,
    /// Optional bbox: [lat_min, lon_min, lat_max, lon_max]. If absent the
    /// planner will mark the mission as "scoping" and skip download stages.
    #[serde(default)]
    pub bbox: Option<[f64; 4]>,
    /// Optional date window in days back from now, for satellite stages.
    #[serde(default)]
    pub days_back: Option<u32>,
    /// JSON mission spec for `sat_mission` (n8n / webhook satellite path).
    #[serde(default)]
    pub spec_path: Option<String>,
    /// Knob overrides merged into the mission spec.
    #[serde(default)]
    pub knobs: Option<serde_json::Value>,
    /// Stage list override for sat_mission_orchestrator.py.
    #[serde(default)]
    pub stages: Option<Vec<String>>,
    /// Skip network downloads in satellite stages.
    #[serde(default)]
    pub dry_run: Option<bool>,
    /// `parallel` (legacy) or `sequential` (default when spec_path set).
    #[serde(default)]
    pub pipeline_mode: Option<String>,
}

fn default_priority() -> u8 { 2 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerState {
    pub name: String,
    pub model_type: String,
    pub current_task: Option<String>,
    pub load: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSnapshot {
    pub workers: Vec<WorkerState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleSpec {
    pub id: String,
    pub name: String,
    pub delegate: DelegateType,
    pub bbox: Option<[f64; 4]>,
    /// Tool name to invoke via /tool/{name}. None means soft-skip.
    pub tool_name: Option<String>,
    /// Pre-built tool args.
    #[serde(default)]
    pub tool_args: serde_json::Value,
    /// Module ids that must complete before this one (sequential pipeline).
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// If true, failures do not abort the sequential pipeline.
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StitchingStrategy {
    pub n_tiles: usize,
    pub stacks_per_p100: usize,
    pub tiles_per_stack: usize,
    pub merge_method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionPlan {
    pub scenario_class: ScenarioClass,
    pub modules: Vec<ModuleSpec>,
    pub stitching: Option<StitchingStrategy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteAssignment {
    pub module_id: String,
    pub endpoint: String,
    pub fallback_endpoints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetoolReceipt {
    /// Workers we stopped, in order. Empty = no retool needed.
    pub stopped_workers: Vec<String>,
}

impl Default for RetoolReceipt {
    fn default() -> Self {
        Self { stopped_workers: Vec::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleResult {
    pub module_id: String,
    pub status: String,
    pub data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionReport {
    pub scenario_class: ScenarioClass,
    pub modules: Vec<ModuleResult>,
    pub stitching_summary: Option<String>,
    pub runtime_seconds: f32,
    pub status: String,
    /// Free-form notes (retool decisions, fallbacks, soft-skips).
    pub notes: Vec<String>,
    /// Optional MTP reviewer summary (polish pass).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
}

// ---------------------------------------------------------------------------
// 1. Cluster probe
// ---------------------------------------------------------------------------

pub async fn probe_cluster() -> Result<ClusterSnapshot, String> {
    // /cluster/discover internally does HTTP probes against N nodes × N ports
    // with 2s budget each, so worst case is ~20s. Give it generous headroom.
    let client = http_client(30);
    let url = format!("{}/cluster/discover", forge_base());
    let resp = client.get(&url).send().await.map_err(|e| format!("probe: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("probe: HTTP {}", resp.status()));
    }
    let raw: serde_json::Value = resp.json().await.map_err(|e| format!("probe parse: {}", e))?;

    // /cluster/discover returns an array of node objects. Flatten into
    // WorkerStates by treating each online port as a worker slot.
    let mut workers = Vec::new();
    if let Some(arr) = raw.as_array() {
        for node in arr {
            let name = node.get("name").and_then(|v| v.as_str()).unwrap_or("?").to_string();
            let gpu = node.get("gpu").and_then(|v| v.as_str()).unwrap_or("cpu").to_string();
            let services_online = node.get("services_online").and_then(|v| v.as_bool()).unwrap_or(false);
            workers.push(WorkerState {
                name,
                model_type: gpu,
                current_task: None,
                load: if services_online { 50.0 } else { 0.0 },
            });
        }
    }
    Ok(ClusterSnapshot { workers })
}

// ---------------------------------------------------------------------------
// 2. Plan from scenario — heuristic v1
// ---------------------------------------------------------------------------

pub async fn plan_from_scenario(scenario: &OperatorScenario) -> Result<MissionPlan, String> {
    if scenario.spec_path.is_some() {
        return Ok(build_satellite_spec_plan(scenario));
    }
    let class = classify_scenario(&scenario.raw_text);
    let plan = build_plan(class, scenario);
    Ok(plan)
}

fn classify_scenario(text: &str) -> ScenarioClass {
    let t = text.to_lowercase();
    let has_any = |needles: &[&str]| needles.iter().any(|n| t.contains(n));

    if has_any(&["downed", "cessna", "aircraft", "plane crash", "missing aircraft"]) {
        ScenarioClass::DownedAircraft
    } else if has_any(&["wreck", "schooner", "shipwreck", "sunken", "freighter", "u-boat"]) {
        ScenarioClass::WreckHunt
    } else if has_any(&["search and rescue", "missing", "fishing boat", "overdue", "sar ", "sar."]) {
        ScenarioClass::SearchRescue
    } else if has_any(&["pollution", "spill", "algae", "turbidity"]) {
        ScenarioClass::EnvironmentalMonitoring
    } else if has_any(&["bathymetry", "current", "salinity", "thermocline"]) {
        ScenarioClass::OceanographicSurvey
    } else {
        ScenarioClass::Unknown
    }
}

fn build_satellite_spec_plan(scenario: &OperatorScenario) -> MissionPlan {
    let bbox = scenario.bbox;
    let bbox_str = bbox.map(fmt_bbox);
    let days = scenario.days_back.unwrap_or(30);
    let spec = scenario.spec_path.clone().unwrap_or_default();
    let class = classify_scenario(&scenario.raw_text);

    MissionPlan {
        scenario_class: class,
        modules: vec![
            weather_module(bbox, days, true),
            sat_mission_module(&spec, scenario),
            sat_read_reports_module(&spec),
            detection_health_module(),
            detection_scan_module(bbox_str.clone(), "wreck"),
            detection_poll_module("det-wreck"),
        ],
        stitching: Some(StitchingStrategy {
            n_tiles: 20,
            stacks_per_p100: 2,
            tiles_per_stack: 10,
            merge_method: "MeanOfMeans".to_string(),
        }),
    }
}

fn build_plan(class: ScenarioClass, scenario: &OperatorScenario) -> MissionPlan {
    let bbox = scenario.bbox;
    let bbox_str = bbox.map(fmt_bbox);
    let days = scenario.days_back.unwrap_or(14);
    let sequential = scenario.pipeline_mode.as_deref() != Some("parallel");

    let (modules, stitching) = match class {
        ScenarioClass::WreckHunt => (
            vec![
                weather_module(bbox, days, sequential),
                satellite_dl_module(bbox_str.clone(), days),
                magnetic_module(),
                detection_scan_module(bbox_str.clone(), "wreck"),
            ],
            Some(StitchingStrategy {
                n_tiles: 20,
                stacks_per_p100: 2,
                tiles_per_stack: 10,
                merge_method: "MeanOfMeans".to_string(),
            }),
        ),
        ScenarioClass::DownedAircraft => (
            vec![
                weather_module(bbox, days, sequential),
                satellite_dl_module(bbox_str.clone(), days),
                detection_scan_module(bbox_str.clone(), "aircraft"),
            ],
            Some(StitchingStrategy {
                n_tiles: 20,
                stacks_per_p100: 2,
                tiles_per_stack: 10,
                merge_method: "MeanOfMeans".to_string(),
            }),
        ),
        ScenarioClass::SearchRescue => (
            vec![
                weather_module(bbox, days, sequential),
                detection_scan_module(bbox_str.clone(), "sar"),
            ],
            None,
        ),
        ScenarioClass::EnvironmentalMonitoring | ScenarioClass::OceanographicSurvey => (
            vec![
                satellite_dl_module(bbox_str.clone(), days),
                detection_scan_module(bbox_str.clone(), "wreck"),
            ],
            None,
        ),
        ScenarioClass::Unknown => (
            vec![
                ModuleSpec {
                    id: "scope-only".to_string(),
                    name: "scenario_health".to_string(),
                    delegate: DelegateType::Cpu,
                    bbox: None,
                    tool_name: Some("detection_health".to_string()),
                    tool_args: serde_json::json!({}),
                    depends_on: Vec::new(),
                    optional: false,
                },
            ],
            None,
        ),
    };

    MissionPlan {
        scenario_class: class,
        modules,
        stitching,
    }
}

fn fmt_bbox(b: [f64; 4]) -> String {
    format!("{},{},{},{}", b[0], b[1], b[2], b[3])
}

fn weather_module(bbox: Option<[f64; 4]>, days: u32, optional: bool) -> ModuleSpec {
    let bbox_str = bbox
        .map(fmt_bbox)
        .unwrap_or_else(|| "44.0,-87.0,45.0,-86.0".to_string());
    ModuleSpec {
        id: "wx-window".to_string(),
        name: "weather_window".to_string(),
        delegate: DelegateType::Cpu,
        bbox,
        tool_name: Some("weather_window".to_string()),
        tool_args: serde_json::json!({
            "bbox": bbox_str,
            "check": "post_storm",
            "days": days,
        }),
        depends_on: Vec::new(),
        optional,
    }
}

fn sat_mission_module(spec_path: &str, scenario: &OperatorScenario) -> ModuleSpec {
    let mut args = serde_json::json!({
        "spec_path": spec_path,
        "dry_run": scenario.dry_run.unwrap_or(false),
    });
    if let Some(k) = &scenario.knobs {
        args["knobs"] = k.clone();
    }
    if let Some(stages) = &scenario.stages {
        args["stages"] = serde_json::json!(stages);
    }
    ModuleSpec {
        id: "sat-mission".to_string(),
        name: "sat_mission".to_string(),
        delegate: DelegateType::Cpu,
        bbox: scenario.bbox,
        tool_name: Some("sat_mission".to_string()),
        tool_args: args,
        depends_on: vec!["wx-window".to_string()],
        optional: false,
    }
}

fn sat_read_reports_module(spec_path: &str) -> ModuleSpec {
    let mut args = serde_json::json!({ "spec_path": spec_path });
    if let Some(dir) = read_spec_output_dir(spec_path) {
        args["output_dir"] = serde_json::json!(dir);
    }
    ModuleSpec {
        id: "sat-reports".to_string(),
        name: "sat_read_mission_report".to_string(),
        delegate: DelegateType::Cpu,
        bbox: None,
        tool_name: Some("sat_read_mission_report".to_string()),
        tool_args: args,
        depends_on: vec!["sat-mission".to_string()],
        optional: true,
    }
}

fn detection_health_module() -> ModuleSpec {
    ModuleSpec {
        id: "det-health".to_string(),
        name: "detection_health".to_string(),
        delegate: DelegateType::Cpu,
        bbox: None,
        tool_name: Some("detection_health".to_string()),
        tool_args: serde_json::json!({}),
        depends_on: vec!["sat-mission".to_string()],
        optional: true,
    }
}

fn satellite_dl_module(bbox: Option<String>, days: u32) -> ModuleSpec {
    ModuleSpec {
        id: "sat-dl".to_string(),
        name: "download_satellite_window".to_string(),
        delegate: DelegateType::Cpu,
        bbox: None,
        tool_name: Some("download_satellite_window".to_string()),
        tool_args: serde_json::json!({
            "bbox": bbox.unwrap_or_else(|| "44.0,-87.0,45.0,-86.0".to_string()),
            "provider": "auto",
            "days": days,
        }),
        depends_on: Vec::new(),
        optional: false,
    }
}

fn magnetic_module() -> ModuleSpec {
    ModuleSpec {
        id: "mag-dipole".to_string(),
        name: "magnetic_dipole_detect".to_string(),
        delegate: DelegateType::VulkanGpu,
        bbox: None,
        tool_name: Some("magnetic_dipole_detect".to_string()),
        // Real grid path injected by the operator via /orchestrator/execute
        // override; v1 keeps a stub that the worker will reject cleanly.
        tool_args: serde_json::json!({
            "grid_path": "/tmp/forge_mag_grid.npy",
            "pixel_size_m": 25.0,
            "inner_radius": 10,
            "outer_radius": 25,
            "min_score": 0.5,
            "top_n": 200,
        }),
        depends_on: Vec::new(),
        optional: true,
    }
}

fn detection_poll_module(scan_module_id: &str) -> ModuleSpec {
    ModuleSpec {
        id: "det-poll".to_string(),
        name: "detection_poll".to_string(),
        delegate: DelegateType::Hybrid,
        bbox: None,
        tool_name: Some("detection_poll".to_string()),
        tool_args: serde_json::json!({}),
        depends_on: vec![scan_module_id.to_string()],
        optional: true,
    }
}

fn detection_scan_module(bbox: Option<String>, mode: &str) -> ModuleSpec {
    // detection_scan expects {region, tiles}. Sequential pipeline fills tiles
    // from wreck_targets_all.csv after sat-mission via PipelineContext.
    let region = match mode {
        "wreck" => "lake_michigan_wreck_scan",
        "aircraft" => "downed_aircraft_scan",
        "sar" => "search_rescue_scan",
        _ => "generic_scan",
    };
    ModuleSpec {
        id: format!("det-{}", mode),
        name: "detection_scan".to_string(),
        delegate: DelegateType::Hybrid,
        bbox: None,
        tool_name: Some("detection_scan".to_string()),
        tool_args: serde_json::json!({
            "region": region,
            "tiles": [],
            "bbox": bbox.unwrap_or_else(|| "44.0,-87.0,45.0,-86.0".to_string()),
            "mode": mode,
        }),
        depends_on: vec!["sat-mission".to_string()],
        optional: true,
    }
}

fn read_spec_output_dir(spec_path: &str) -> Option<String> {
    let content = std::fs::read_to_string(spec_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    v.get("paths")
        .and_then(|p| p.get("output_dir"))
        .and_then(|o| o.as_str())
        .map(|s| s.to_string())
}

fn use_sequential_pipeline(scenario: &OperatorScenario) -> bool {
    if scenario.spec_path.is_some() {
        return true;
    }
    matches!(
        scenario.pipeline_mode.as_deref(),
        Some("sequential") | Some("staged")
    )
}

/// Mutable state passed between sequential module invocations.
#[derive(Default)]
struct PipelineContext {
    output_dir: Option<String>,
    detection_job_id: Option<String>,
}

impl PipelineContext {
    fn from_scenario(scenario: &OperatorScenario) -> Self {
        let mut ctx = Self::default();
        if let Some(spec_path) = &scenario.spec_path {
            ctx.output_dir = read_spec_output_dir(spec_path);
        }
        ctx
    }

    fn absorb_module_result(&mut self, spec: &ModuleSpec, data: &str) {
        if spec.id == "sat-mission" || spec.tool_name.as_deref() == Some("sat_mission") {
            if let Some(dir) = extract_output_dir_line(data) {
                self.output_dir = Some(dir);
            } else if let Some(dir) = extract_output_dir_guess(data) {
                self.output_dir = Some(dir);
            } else if let Some(dir) = extract_output_dir_json(data) {
                self.output_dir = Some(dir);
            }
        }
        if spec.tool_name.as_deref() == Some("detection_scan") {
            if let Some(id) = extract_job_id(data) {
                self.detection_job_id = Some(id);
            }
        }
    }

    fn enrich_tool_args(&self, spec: &mut ModuleSpec) {
        if spec.id == "sat-reports" {
            if let Some(dir) = &self.output_dir {
                spec.tool_args = serde_json::json!({
                    "output_dir": dir,
                    "which": "both",
                });
            }
        }
        if spec.tool_name.as_deref() == Some("detection_scan") {
            if let Some(dir) = &self.output_dir {
                let csv = std::path::Path::new(dir).join("wreck_targeting/wreck_targets_all.csv");
                if let Some(tiles) = load_detection_tiles_from_csv(&csv, 32) {
                    if let Some(obj) = spec.tool_args.as_object_mut() {
                        obj.insert("tiles".to_string(), tiles);
                    }
                }
            }
        }
        if spec.tool_name.as_deref() == Some("magnetic_dipole_detect") {
            if let Some(dir) = &self.output_dir {
                let grid = std::path::Path::new(dir).join("mag_grid.npy");
                if grid.exists() {
                    if let Some(obj) = spec.tool_args.as_object_mut() {
                        obj.insert(
                            "grid_path".to_string(),
                            serde_json::Value::String(grid.display().to_string()),
                        );
                    }
                }
            }
        }
        if spec.tool_name.as_deref() == Some("detection_poll") {
            if let Some(id) = &self.detection_job_id {
                spec.tool_args = serde_json::json!({ "job_id": id });
            }
        }
    }
}

/// 1×1 gray PNG — valid input for triple-lock smoke when chips are not fetched yet.
const PLACEHOLDER_TILE_B64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

/// Build detection_scan tiles from wreck_targets_all.csv (lat/lon per wreck).
fn load_detection_tiles_from_csv(
    csv_path: &std::path::Path,
    max_tiles: usize,
) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(csv_path).ok()?;
    let mut lines = content.lines();
    let header = lines.next()?;
    let cols: Vec<&str> = header.split(',').map(|s| s.trim()).collect();
    let lat_i = cols.iter().position(|c| *c == "lat")?;
    let lon_i = cols.iter().position(|c| *c == "lon")?;
    let name_i = cols.iter().position(|c| *c == "wreck_name");

    let mut tiles = Vec::new();
    for line in lines {
        if tiles.len() >= max_tiles {
            break;
        }
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() <= lat_i.max(lon_i) {
            continue;
        }
        let lat: f64 = fields[lat_i].trim().parse().ok()?;
        let lon: f64 = fields[lon_i].trim().parse().ok()?;
        let tile_id = name_i
            .and_then(|i| fields.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("wreck_{}", tiles.len()));
        tiles.push(serde_json::json!({
            "lat": lat,
            "lon": lon,
            "image_b64": PLACEHOLDER_TILE_B64,
            "tile_id": tile_id,
        }));
    }
    if tiles.is_empty() {
        None
    } else {
        Some(serde_json::Value::Array(tiles))
    }
}

fn extract_output_dir_line(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("output_dir=") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn extract_output_dir_guess(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("output_dir_guess=") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Last JSON object in stdout may contain paths.output_dir or output_dir.
fn extract_output_dir_json(text: &str) -> Option<String> {
    let start = text.rfind('{')?;
    let end = text.rfind('}')?;
    let slice = &text[start..=end];
    let v: serde_json::Value = serde_json::from_str(slice).ok()?;
    v.get("output_dir")
        .and_then(|o| o.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            v.get("paths")
                .and_then(|p| p.get("output_dir"))
                .and_then(|o| o.as_str())
                .map(|s| s.to_string())
        })
}

fn extract_job_id(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        if let Some(rest) = token.strip_prefix("job_id=") {
            return Some(rest.trim_end_matches(|c: char| ",.)".contains(c)).to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 3. Specialist routing
// ---------------------------------------------------------------------------

pub fn assign_specialists(modules: &[ModuleSpec], cluster: &ClusterSnapshot) -> Vec<RouteAssignment> {
    let mut assignments = Vec::with_capacity(modules.len());

    for m in modules {
        let endpoint = format!(
            "{}/tool/{}",
            forge_base(),
            m.tool_name.clone().unwrap_or_else(|| "noop".to_string())
        );

        let fallback_endpoints = match m.delegate {
            DelegateType::CoralTpuInt8 => {
                // Coral isn't installed yet; T9 routing table says fall back to
                // VulkanGpu. Same forge route, different tool_args caller could swap.
                vec![endpoint.clone()]
            }
            DelegateType::LlmRemote => {
                // Pick the lowest-load worker that's not a P100 (those are reserved
                // for tile compute during WreckHunt/DownedAircraft missions).
                pick_llm_fallback(cluster)
                    .map(|name| vec![format!("{}/cluster/worker/{}/apply", forge_base(), name)])
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        };

        assignments.push(RouteAssignment {
            module_id: m.id.clone(),
            endpoint,
            fallback_endpoints,
        });
    }

    assignments
}

fn pick_llm_fallback(cluster: &ClusterSnapshot) -> Option<String> {
    cluster
        .workers
        .iter()
        .filter(|w| {
            let mt = w.model_type.to_lowercase();
            mt.contains("1060") || mt.contains("1070") || mt.contains("p1000")
        })
        .min_by(|a, b| a.load.partial_cmp(&b.load).unwrap_or(std::cmp::Ordering::Equal))
        .map(|w| w.name.clone())
}

// ---------------------------------------------------------------------------
// 4. Cluster retooling
//
// CESAROPS doctrine (May 17, 2026): the **P100s are kept clear by default**.
// The intake brain is Picasso (P1000:5571), which also handles n8n and
// validator/health duty. So "retool" really just means: verify the P100s are
// empty before launching tile/mag compute, and stop anything that's hanging
// around. There is no "restore to LLM mode" because that's coding-mode
// territory, not CESAROPS.
// ---------------------------------------------------------------------------

pub async fn retool_for_mission(plan: &MissionPlan) -> Result<RetoolReceipt, String> {
    // Only WreckHunt and DownedAircraft demand the P100s. Other missions don't
    // touch them at all.
    let needs_p100 = matches!(
        plan.scenario_class,
        ScenarioClass::WreckHunt | ScenarioClass::DownedAircraft
    ) && plan.modules.iter().any(|m| matches!(m.delegate, DelegateType::VulkanGpu));

    if !needs_p100 {
        return Ok(RetoolReceipt::default());
    }

    // Verify-and-clear loop: for each P100 worker, check if its port is
    // bound. If it is, that means coding-mode left a worker running (or a
    // human did). Stop it via the existing worker-control route.
    let client = http_client(5);
    let mut stopped = Vec::new();

    for &(worker_name, port) in P100_WORKERS {
        if !is_port_bound(port).await {
            continue;
        }
        let url = format!("{}/cluster/worker/{}/stop", forge_base(), worker_name);
        match client.post(&url).send().await {
            Ok(r) if r.status().is_success() => {
                stopped.push(worker_name.to_string());
                info!("retool: cleared {} (port {} was bound)", worker_name, port);
            }
            Ok(r) => warn!("retool: {} stop returned HTTP {}", worker_name, r.status()),
            Err(e) => warn!("retool: {} stop failed: {}", worker_name, e),
        }
    }

    Ok(RetoolReceipt { stopped_workers: stopped })
}

/// Bring the **secondary fleet** (1070/2060/P106 on cesarops2) online when needed.
/// back online so the n8n / health / intake / draft / corrector roles are
/// available. Called at the end of every mission — its job is to make sure
/// every box that should be answering is answering, regardless of whether
/// retool was needed.
///
/// In CESAROPS doctrine the **P100s stay clear** (that's what `retool_for_mission`
/// guarantees), so this function explicitly avoids :5001/:5002. If the operator
/// wants the P100 LLM coders back, that's a `/mode/coding` switch, not a
/// mission restore.
///
/// Behavior per worker:
///   1. Probe the worker's port via the same /api/v1/model handshake the
///      cluster discover route uses.
///   2. If responsive: status = "ok".
///   3. If silent and the host is local: hit `/cluster/worker/{name}/start`
///      then wait up to 90s for the port to bind.
///   4. If silent and the host is remote (LAN/Tailscale): SSH into the host
///      and run a stored launch script — `~/start_<worker>.sh`. Best-effort
///      only; if SSH fails or the script is missing, mark "needs_manual".
pub async fn restore_cluster(_receipt: &RetoolReceipt) -> Vec<String> {
    let mut notes = Vec::new();
    let workers = load_secondary_fleet();
    if workers.is_empty() {
        return notes;
    }

    let client = http_client(8);

    for w in workers {
        // Skip explicitly-disabled workers; the operator decided.
        if !w.enabled {
            notes.push(format!("{}: skipped (disabled in cluster_config)", w.name));
            continue;
        }

        // 1. Probe.
        let probe_url = format!("http://{}:{}/api/v1/model", w.host, w.port);
        let online = client
            .get(&probe_url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);

        if online {
            notes.push(format!("{}: ok ({})", w.name, probe_url));
            continue;
        }

        // 2. Silent — try to bring it up.
        let bring_up_result = if w.host == "local" || w.host == "127.0.0.1" {
            bring_up_local(&client, &w).await
        } else {
            bring_up_remote(&w).await
        };

        // 3. Verify post-launch.
        match bring_up_result {
            BringUpOutcome::Started => {
                let online = wait_for_remote_port(&client, &w.host, w.port,
                    Duration::from_secs(90), Duration::from_secs(3)).await;
                if online {
                    notes.push(format!("{}: started ({}:{} up)", w.name, w.host, w.port));
                } else {
                    notes.push(format!("{}: started_no_port ({}:{} silent after 90s)", w.name, w.host, w.port));
                }
            }
            BringUpOutcome::Failed(reason) => {
                notes.push(format!("{}: needs_manual ({}:{} - {})", w.name, w.host, w.port, reason));
            }
            BringUpOutcome::HostUnreachable => {
                notes.push(format!("{}: host_offline ({} not pingable)", w.name, w.host));
            }
        }
    }

    notes
}

#[derive(Debug, Clone)]
struct SecondaryWorker {
    name: String,
    host: String,
    port: u16,
    enabled: bool,
}

fn load_secondary_fleet() -> Vec<SecondaryWorker> {
    let path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let table: toml::Table = match content.parse() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    if let Some(workers) = table.get("worker").and_then(|v| v.as_array()) {
        for w in workers {
            let name = w.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let port = w.get("port").and_then(|v| v.as_integer()).unwrap_or(0) as u16;
            let host = w.get("host").and_then(|v| v.as_str()).unwrap_or("local").to_string();
            let enabled = w.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);

            if name.is_empty() || port == 0 {
                continue;
            }
            // Skip the P100 workers — they belong to coding mode, not the
            // CESAROPS-mode secondary fleet.
            if P100_WORKERS.iter().any(|(n, _)| *n == name) {
                continue;
            }
            // Skip the local Thinker — it's a coding-mode preflight worker
            // sharing GPU 0 with GemmaBig, gets in the way of CESAROPS.
            if name == "Thinker" {
                continue;
            }

            out.push(SecondaryWorker { name, host, port, enabled });
        }
    }
    out
}

enum BringUpOutcome {
    Started,
    Failed(String),
    HostUnreachable,
}

async fn bring_up_local(client: &Client, w: &SecondaryWorker) -> BringUpOutcome {
    let url = format!("{}/cluster/worker/{}/start", forge_base(), w.name);
    match client.post(&url).send().await {
        Ok(r) if r.status().is_success() => BringUpOutcome::Started,
        Ok(r) => BringUpOutcome::Failed(format!("HTTP {}", r.status())),
        Err(e) => BringUpOutcome::Failed(format!("transport: {}", e)),
    }
}

struct SpawnConfig {
    model_path: String,
    gpu_layers: u32,
    context_size: u32,
}

fn load_worker_spawn_config(name: &str) -> Option<SpawnConfig> {
    let path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    let content = std::fs::read_to_string(path).ok()?;
    let table: toml::Table = content.parse().ok()?;
    let workers = table.get("worker")?.as_array()?;
    let worker = workers.iter().find(|w| w.get("name").and_then(|v| v.as_str()) == Some(name))?;
    Some(SpawnConfig {
        model_path: worker.get("model").and_then(|v| v.as_str())?.to_string(),
        gpu_layers: worker.get("gpulayers").and_then(|v| v.as_integer()).unwrap_or(999) as u32,
        context_size: worker.get("contextsize").and_then(|v| v.as_integer()).unwrap_or(8192) as u32,
    })
}

async fn bring_up_remote(w: &SecondaryWorker) -> BringUpOutcome {
    let client = reqwest::Client::new();
    let base_url = format!("http://{}:5500", w.host);

    // 1. Check if node daemon is reachable.
    let status_res = timeout(
        Duration::from_secs(3),
        client.get(format!("{}/status", base_url)).send(),
    )
    .await;

    let status_val = match status_res {
        Ok(Ok(r)) if r.status().is_success() => {
            r.json::<serde_json::Value>().await.unwrap_or_default()
        }
        _ => return BringUpOutcome::HostUnreachable,
    };

    // 2. If already serving, nothing to do.
    if status_val.get("state").and_then(|s| s.as_str()) == Some("serving") {
        return BringUpOutcome::Started;
    }

    // 3. Load spawn config from cluster_config.toml.
    let cfg = match load_worker_spawn_config(&w.name) {
        Some(c) => c,
        None => return BringUpOutcome::Failed(format!("no config for worker '{}'", w.name)),
    };

    // 4. POST /spawn to the node daemon.
    let payload = serde_json::json!({
        "model_path": cfg.model_path,
        "port": w.port,
        "gpu_layers": cfg.gpu_layers,
        "context_size": cfg.context_size,
    });

    let spawn_res = timeout(
        Duration::from_secs(130),
        client.post(format!("{}/spawn", base_url)).json(&payload).send(),
    )
    .await;

    match spawn_res {
        Ok(Ok(r)) if r.status().is_success() => {
            info!("bring_up_remote: {} spawned on {}:{}", w.name, w.host, w.port);
            BringUpOutcome::Started
        }
        Ok(Ok(r)) => {
            let body = r.json::<serde_json::Value>().await.unwrap_or_default();
            let err = body.get("error").and_then(|e| e.as_str()).unwrap_or("unknown");
            BringUpOutcome::Failed(err.to_string())
        }
        Ok(Err(e)) => BringUpOutcome::Failed(format!("spawn transport: {}", e)),
        Err(_) => BringUpOutcome::Failed("spawn timeout (130s)".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Model swap — stop current model, load a different one
// ---------------------------------------------------------------------------

/// Swap the model on a node. Stops whatever is running, loads the requested model.
/// Used by the orchestrator when a mission step needs a specific model type.
///
/// `host`: node IP (e.g. "10.0.0.201")
/// `model_path`: full path to the .gguf on that node
/// `port`: inference port to serve on
/// `gpu_layers`: layers to offload
/// `context_size`: context window
///
/// Returns the model name if successful.
pub async fn swap_model_on_node(
    host: &str,
    model_path: &str,
    port: u16,
    gpu_layers: u32,
    context_size: u32,
) -> Result<String, String> {
    let client = http_client(10);
    let base_url = format!("http://{}:5500", host);

    // 1. Check if node daemon is reachable
    let status = timeout(Duration::from_secs(3), client.get(format!("{}/status", base_url)).send())
        .await
        .map_err(|_| "node unreachable (timeout)".to_string())?
        .map_err(|e| format!("node unreachable: {}", e))?;

    if !status.status().is_success() {
        return Err(format!("node status HTTP {}", status.status()));
    }

    let status_json: serde_json::Value = status.json().await.unwrap_or_default();
    let current_state = status_json.get("state").and_then(|s| s.as_str()).unwrap_or("unknown");

    // 2. If currently serving, stop it first
    if current_state == "serving" || current_state == "loading" {
        info!("swap_model: stopping current model on {}", host);
        let stop_res = timeout(
            Duration::from_secs(10),
            client.post(format!("{}/stop", base_url)).send(),
        ).await;

        match stop_res {
            Ok(Ok(r)) if r.status().is_success() => {
                info!("swap_model: stopped on {}", host);
            }
            Ok(Ok(r)) => warn!("swap_model: stop returned HTTP {}", r.status()),
            Ok(Err(e)) => warn!("swap_model: stop failed: {}", e),
            Err(_) => warn!("swap_model: stop timed out"),
        }

        // Brief pause for VRAM to free
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // 3. Spawn the new model
    let payload = serde_json::json!({
        "model_path": model_path,
        "port": port,
        "gpu_layers": gpu_layers,
        "context_size": context_size,
    });

    info!("swap_model: spawning {} on {}:{}", model_path, host, port);

    let spawn_res = timeout(
        Duration::from_secs(130),
        client.post(format!("{}/spawn", base_url)).json(&payload).send(),
    ).await;

    match spawn_res {
        Ok(Ok(r)) if r.status().is_success() => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            let model = body.get("model").and_then(|m| m.as_str()).unwrap_or(model_path);
            info!("swap_model: {} now serving on {}:{}", model, host, port);
            Ok(model.to_string())
        }
        Ok(Ok(r)) => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            let err = body.get("error").and_then(|e| e.as_str()).unwrap_or("spawn failed");
            Err(err.to_string())
        }
        Ok(Err(e)) => Err(format!("spawn transport: {}", e)),
        Err(_) => Err("spawn timeout (130s)".to_string()),
    }
}

/// Swap a model on a LOCAL worker (P100s on the T440).
/// Uses the forge's own /cluster/worker/{name}/stop + /start routes.
pub async fn swap_local_worker(worker_name: &str, model_path: &str) -> Result<String, String> {
    let client = http_client(10);

    // Stop current
    let stop_url = format!("{}/cluster/worker/{}/stop", forge_base(), worker_name);
    let _ = client.post(&stop_url).send().await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // The start route uses cluster_config.toml — but we want a DIFFERENT model.
    // For now, we'll use the node daemon's /spawn if running locally.
    // The local node daemon is at 127.0.0.1:5500.
    let payload = serde_json::json!({
        "model_path": model_path,
        "port": match worker_name {
            "GemmaBig" => 5001,
            "QwenBig" => 5002,
            _ => 5010,
        },
        "gpu_layers": 999,
        "context_size": 8192,
    });

    let spawn_url = format!("http://127.0.0.1:5500/spawn");
    let spawn_res = timeout(
        Duration::from_secs(130),
        client.post(&spawn_url).json(&payload).send(),
    ).await;

    match spawn_res {
        Ok(Ok(r)) if r.status().is_success() => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            let model = body.get("model").and_then(|m| m.as_str()).unwrap_or(model_path);
            Ok(model.to_string())
        }
        Ok(Ok(r)) => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            Err(body.get("error").and_then(|e| e.as_str()).unwrap_or("failed").to_string())
        }
        Ok(Err(e)) => Err(format!("spawn: {}", e)),
        Err(_) => Err("spawn timeout".to_string()),
    }
}

async fn wait_for_remote_port(
    client: &Client,
    host: &str,
    port: u16,
    total: Duration,
    interval: Duration,
) -> bool {
    let url = format!("http://{}:{}/api/v1/model", host, port);
    let start = std::time::Instant::now();
    while start.elapsed() < total {
        if let Ok(r) = client.get(&url).send().await {
            if r.status().is_success() {
                return true;
            }
        }
        tokio::time::sleep(interval).await;
    }
    false
}

/// Legacy local-only restore path that re-launches every worker in the
/// receipt. Kept for hybrid/coding-mode flows where the orchestrator's
/// retool stop step DID need a reverse. Not called from execute_mission.
#[allow(dead_code)]
pub async fn restore_cluster_legacy(receipt: &RetoolReceipt) -> Vec<String> {
    let mut notes = Vec::new();
    if receipt.stopped_workers.is_empty() {
        return notes;
    }

    let client = http_client(10);
    let port_by_name = load_worker_ports();

    for worker in &receipt.stopped_workers {
        let url = format!("{}/cluster/worker/{}/start", forge_base(), worker);
        match client.post(&url).send().await {
            Ok(r) if r.status().is_success() => {
                info!("restore: requested start for {}", worker);
            }
            Ok(r) => {
                let msg = format!("{}: start HTTP {}", worker, r.status());
                warn!("{}", msg);
                notes.push(format!("{}: start_failed", worker));
                continue;
            }
            Err(e) => {
                warn!("restore: {} start failed: {}", worker, e);
                notes.push(format!("{}: start_failed (transport {})", worker, e));
                continue;
            }
        }

        match port_by_name.get(worker.as_str()) {
            Some(&port) => {
                let online = wait_for_port(port, Duration::from_secs(120), Duration::from_secs(2)).await;
                if online {
                    notes.push(format!("{}: started (port {} up)", worker, port));
                } else {
                    notes.push(format!("{}: started_no_port (port {} silent after 120s)", worker, port));
                }
            }
            None => {
                notes.push(format!("{}: started (port unknown — verify manually)", worker));
            }
        }
    }
    notes
}

async fn is_port_bound(port: u16) -> bool {
    let addr = format!("127.0.0.1:{}", port);
    matches!(
        timeout(Duration::from_millis(500), tokio::net::TcpStream::connect(&addr)).await,
        Ok(Ok(_))
    )
}

/// Lightweight intake-brain availability check. Returns which endpoint (if any)
/// is responsive. Used by execute_mission to surface intake state in the
/// mission report — the orchestrator itself doesn't currently need an LLM
/// call for planning (heuristic classifier), but knowing whether Picasso is
/// up matters because it's also serving n8n + cesarops.com webhook intake.
struct IntakeStatus {
    primary_online: bool,
    primary_url: Option<String>,
    fallback_online: bool,
    fallback_url: Option<String>,
}

impl IntakeStatus {
    fn note(&self) -> String {
        match (&self.primary_url, self.primary_online, self.fallback_online) {
            (Some(url), true, _) => format!("intake brain: online ({})", url),
            (_, false, true) => format!(
                "intake brain: primary down, fallback online ({})",
                self.fallback_url.as_deref().unwrap_or("?")
            ),
            _ => "intake brain: all nodes offline — heuristic planner only".to_string(),
        }
    }
}

async fn probe_intake_brain() -> IntakeStatus {
    let client = http_client(3);
    let pool = load_intake_pool();
    let mut primary: Option<String> = None;
    let mut fallback: Option<String> = None;

    for url in &pool {
        if !probe_llama_endpoint(&client, url).await {
            continue;
        }
        if primary.is_none() {
            primary = Some(url.clone());
        } else if fallback.is_none() {
            fallback = Some(url.clone());
            break;
        }
    }

    IntakeStatus {
        primary_online: primary.is_some(),
        primary_url: primary,
        fallback_online: fallback.is_some(),
        fallback_url: fallback,
    }
}

fn load_worker_ports() -> std::collections::HashMap<String, u16> {
    use std::collections::HashMap;
    let path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    let mut out = HashMap::new();
    if let Some(workers) = table.get("worker").and_then(|v| v.as_array()) {
        for w in workers {
            let name = w.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let port = w.get("port").and_then(|v| v.as_integer()).unwrap_or(0) as u16;
            if !name.is_empty() && port != 0 {
                out.insert(name, port);
            }
        }
    }
    out
}

async fn wait_for_port(port: u16, total: Duration, interval: Duration) -> bool {
    let start = std::time::Instant::now();
    let addr = format!("127.0.0.1:{}", port);
    while start.elapsed() < total {
        if let Ok(Ok(_)) = timeout(
            Duration::from_millis(500),
            tokio::net::TcpStream::connect(&addr),
        )
        .await
        {
            return true;
        }
        tokio::time::sleep(interval).await;
    }
    false
}

// ---------------------------------------------------------------------------
// 5. Dispatch
// ---------------------------------------------------------------------------

pub async fn dispatch_modules(
    plan: &MissionPlan,
    routing: &[RouteAssignment],
) -> Vec<ModuleResult> {
    let mut set: JoinSet<ModuleResult> = JoinSet::new();

    // Index modules by id for fast lookup (avoid O(n*m) in routing loop).
    let mut spec_by_id = std::collections::HashMap::new();
    for m in &plan.modules {
        spec_by_id.insert(m.id.clone(), m.clone());
    }

    for route in routing {
        let route = route.clone();
        let spec = match spec_by_id.get(&route.module_id) {
            Some(s) => s.clone(),
            None => {
                set.spawn(async move {
                    ModuleResult {
                        module_id: route.module_id,
                        status: "skipped".to_string(),
                        data: Some("no matching ModuleSpec".to_string()),
                    }
                });
                continue;
            }
        };

        let tool_for_client = spec.tool_name.clone().unwrap_or_else(|| "noop".to_string());
        let client = http_client(module_timeout_secs(&tool_for_client) + 5);
        set.spawn(async move {
            let task = async {
                // /tool/{name} expects {"arguments": {...}} envelope.
                let body = serde_json::json!({ "arguments": spec.tool_args.clone() });
                match client.post(&route.endpoint).json(&body).send().await {
                    Ok(r) if r.status().is_success() => {
                        let txt = r.text().await.unwrap_or_default();
                        ModuleResult {
                            module_id: route.module_id.clone(),
                            status: "ok".to_string(),
                            data: Some(truncate(&txt, 4000)),
                        }
                    }
                    Ok(r) => {
                        let code = r.status();
                        let txt = r.text().await.unwrap_or_default();
                        ModuleResult {
                            module_id: route.module_id.clone(),
                            status: format!("http_{}", code.as_u16()),
                            data: Some(truncate(&txt, 1500)),
                        }
                    }
                    Err(e) => ModuleResult {
                        module_id: route.module_id.clone(),
                        status: "failed".to_string(),
                        data: Some(format!("transport: {}", e)),
                    },
                }
            };

            let secs = module_timeout_secs(
                spec.tool_name.as_deref().unwrap_or("noop"),
            );
            match timeout(Duration::from_secs(secs), task).await {
                Ok(r) => r,
                Err(_) => ModuleResult {
                    module_id: route.module_id,
                    status: "timeout".to_string(),
                    data: Some(format!("exceeded {}s", secs)),
                },
            }
        });
    }

    let mut out = Vec::new();
    while let Some(r) = set.join_next().await {
        match r {
            Ok(m) => out.push(m),
            Err(e) => warn!("dispatch: join error: {}", e),
        }
    }
    out
}

/// Run modules in plan order; optional modules may fail without aborting.
pub async fn dispatch_modules_sequential(
    plan: &MissionPlan,
    scenario: &OperatorScenario,
) -> Vec<ModuleResult> {
    let mut ctx = PipelineContext::from_scenario(scenario);
    let mut results = Vec::new();
    let mut completed: std::collections::HashSet<String> = std::collections::HashSet::new();

    for spec in &plan.modules {
        let tool = match &spec.tool_name {
            Some(t) => t.clone(),
            None => {
                results.push(ModuleResult {
                    module_id: spec.id.clone(),
                    status: "skipped".to_string(),
                    data: Some("no tool_name".to_string()),
                });
                continue;
            }
        };

        if !spec.depends_on.is_empty()
            && !spec.depends_on.iter().all(|d| completed.contains(d))
        {
            results.push(ModuleResult {
                module_id: spec.id.clone(),
                status: "skipped".to_string(),
                data: Some(format!("deps not met: {:?}", spec.depends_on)),
            });
            continue;
        }

        let mut spec_mut = spec.clone();
        ctx.enrich_tool_args(&mut spec_mut);

        let endpoint = format!("{}/tool/{}", forge_base(), tool);
        let body = serde_json::json!({ "arguments": spec_mut.tool_args });
        let client = http_client(module_timeout_secs(&tool) + 5);

        let task = async {
            match client.post(&endpoint).json(&body).send().await {
                Ok(r) if r.status().is_success() => {
                    let txt = r.text().await.unwrap_or_default();
                    ModuleResult {
                        module_id: spec.id.clone(),
                        status: "ok".to_string(),
                        data: Some(truncate(&txt, 4000)),
                    }
                }
                Ok(r) => {
                    let code = r.status();
                    let txt = r.text().await.unwrap_or_default();
                    ModuleResult {
                        module_id: spec.id.clone(),
                        status: format!("http_{}", code.as_u16()),
                        data: Some(truncate(&txt, 1500)),
                    }
                }
                Err(e) => ModuleResult {
                    module_id: spec.id.clone(),
                    status: "failed".to_string(),
                    data: Some(format!("transport: {}", e)),
                },
            }
        };

        let secs = module_timeout_secs(&tool);
        let result = match timeout(Duration::from_secs(secs), task).await {
            Ok(r) => r,
            Err(_) => ModuleResult {
                module_id: spec.id.clone(),
                status: "timeout".to_string(),
                data: Some(format!("exceeded {}s", secs)),
            },
        };

        if let Some(ref data) = result.data {
            ctx.absorb_module_result(spec, data);
        }

        let failed = result.status != "ok";
        if result.status == "ok" {
            completed.insert(spec.id.clone());
        }
        results.push(result);

        if failed && !spec.optional {
            warn!(
                "sequential pipeline: abort after required module {} failed",
                spec.id
            );
            break;
        }
    }

    results
}

// ---------------------------------------------------------------------------
// 5b. LLM plan refinement (optional, graceful degradation)
// ---------------------------------------------------------------------------

/// Ask the intake brain (Picasso/scout) to refine the heuristic plan.
/// If the LLM is unreachable or returns garbage, the heuristic plan is
/// returned unchanged — this is a best-effort enhancement, not a gate.
async fn llm_refine_plan(
    heuristic_plan: MissionPlan,
    scenario: &OperatorScenario,
    notes: &mut Vec<String>,
) -> MissionPlan {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(_) => return heuristic_plan,
    };

    let module_names: Vec<&str> = heuristic_plan.modules.iter()
        .map(|m| m.name.as_str())
        .collect();

    let prompt = format!(
        "You are a SAR mission planner. Given this scenario and initial plan, output ONLY valid JSON.\n\n\
         Scenario: {}\nBBox: {:?}\n\n\
         Current plan:\n- Class: {:?}\n- Modules: {:?}\n\n\
         If the plan looks correct, output: {{\"action\": \"accept\"}}\n\
         If you want to add a module, output: {{\"action\": \"add_module\", \"module\": {{\"id\": \"llm-added\", \"name\": \"tool_name\", \"tool_name\": \"tool_name\", \"tool_args\": {{}}}}}}\n\
         If you want to change the scenario class, output: {{\"action\": \"reclassify\", \"class\": \"WreckHunt\"}}\n\n\
         Output ONLY valid JSON:",
        scenario.raw_text, scenario.bbox, heuristic_plan.scenario_class, module_names
    );

    for endpoint in load_intake_pool() {
        let text = match crate::inference_client::complete_prompt(
            &client,
            &endpoint,
            &prompt,
            256,
            0.1,
            vec!["\n\n".to_string()],
            None,
        )
        .await
        {
            Ok(t) => t.trim().to_string(),
            Err(_) => continue,
        };
        // Try to parse the LLM output as JSON action.
        let action: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => {
                // Try extracting JSON from within the text (LLM may add preamble).
                let start = text.find('{');
                let end = text.rfind('}');
                if let (Some(s), Some(e)) = (start, end) {
                    match serde_json::from_str(&text[s..=e]) {
                        Ok(v) => v,
                        Err(_) => { notes.push("LLM refinement: parse failed".to_string()); return heuristic_plan; }
                    }
                } else {
                    notes.push("LLM refinement: no JSON in response".to_string());
                    return heuristic_plan;
                }
            }
        };

        let mut plan = heuristic_plan.clone();
        match action.get("action").and_then(|a| a.as_str()) {
            Some("accept") => {
                notes.push("LLM refinement: accepted heuristic plan".to_string());
                return plan;
            }
            Some("add_module") => {
                if let Some(m) = action.get("module") {
                    let module = ModuleSpec {
                        id: m.get("id").and_then(|v| v.as_str()).unwrap_or("llm-added").to_string(),
                        name: m.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
                        delegate: DelegateType::Cpu,
                        bbox: None,
                        tool_name: m.get("tool_name").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        tool_args: m.get("tool_args").cloned().unwrap_or(serde_json::json!({})),
                        depends_on: Vec::new(),
                        optional: true,
                    };
                    plan.modules.push(module);
                    notes.push("LLM refinement: added module".to_string());
                }
                return plan;
            }
            Some("reclassify") => {
                if let Some(cls) = action.get("class").and_then(|c| c.as_str()) {
                    let new_class = match cls {
                        "WreckHunt" => ScenarioClass::WreckHunt,
                        "DownedAircraft" => ScenarioClass::DownedAircraft,
                        "SearchRescue" => ScenarioClass::SearchRescue,
                        _ => plan.scenario_class.clone(),
                    };
                    if new_class != plan.scenario_class {
                        notes.push(format!("LLM refinement: reclassified to {:?}", new_class));
                        return build_plan(new_class, scenario);
                    }
                }
                return plan;
            }
            _ => {
                notes.push("LLM refinement: unknown action".to_string());
                return heuristic_plan;
            }
        }
    }

    notes.push("LLM refinement: intake brain unreachable, using heuristic".to_string());
    heuristic_plan
}

// ---------------------------------------------------------------------------
// 6. Top-level mission execution
// ---------------------------------------------------------------------------

pub async fn execute_mission(scenario: OperatorScenario) -> MissionReport {
    let start = std::time::Instant::now();
    let mut notes = Vec::new();

    // 1. Verify the intake brain. In CESAROPS doctrine the P100s do NOT
    //    serve LLMs — Picasso (P1000:5571) does. If she's offline we fall
    //    back to scout (cesarops3:5570). We don't actually need an LLM call
    //    for the heuristic planner, but we want the operator to see the
    //    intake-brain status in the report so they know what's responsive.
    let intake_status = probe_intake_brain().await;
    notes.push(intake_status.note());

    let cluster = match probe_cluster().await {
        Ok(c) => {
            notes.push(format!("probed {} worker(s)", c.workers.len()));
            c
        }
        Err(e) => {
            notes.push(format!("probe failed: {}", e));
            ClusterSnapshot { workers: Vec::new() }
        }
    };

    let plan = match retry_plan(&scenario).await {
        Ok(p) => p,
        Err(e) => {
            return MissionReport {
                scenario_class: ScenarioClass::Unknown,
                modules: Vec::new(),
                stitching_summary: None,
                runtime_seconds: start.elapsed().as_secs_f32(),
                status: "failed".to_string(),
                notes: vec![format!("planner: {}", e)],
                review: None,
            };
        }
    };

    // LLM refinement pass — ask intake brain to validate/enhance the plan.
    let plan = llm_refine_plan(plan, &scenario, &mut notes).await;

    let class = plan.scenario_class.clone();
    notes.push(format!("classified as {:?}", class));

    let routing = assign_specialists(&plan.modules, &cluster);
    let receipt = retool_for_mission(&plan).await.unwrap_or_default();
    if !receipt.stopped_workers.is_empty() {
        notes.push(format!("retooled: cleared P100s {:?}", receipt.stopped_workers));
    } else if matches!(class, ScenarioClass::WreckHunt | ScenarioClass::DownedAircraft) {
        notes.push("retool: P100s already clear".to_string());
    }

    let sequential = use_sequential_pipeline(&scenario);
    notes.push(format!(
        "dispatch: {}",
        if sequential { "sequential" } else { "parallel" }
    ));

    let results = if sequential {
        dispatch_modules_sequential(&plan, &scenario).await
    } else {
        dispatch_modules(&plan, &routing).await
    };

    // Restore is unconditional: every mission ends with a secondary-fleet
    // health pass. Bring back any 1060/1070/P1000 worker that's silent.
    let restore_notes = restore_cluster(&receipt).await;
    if !restore_notes.is_empty() {
        notes.push(format!("secondary fleet: {}", restore_notes.join("; ")));
    }

    let ok_count = results.iter().filter(|r| r.status == "ok").count();
    let total = results.len();
    let status = if total == 0 {
        "empty"
    } else if ok_count == total {
        "ok"
    } else if ok_count > 0 {
        "partial"
    } else {
        "failed"
    };

    let stitching_summary = plan.stitching.as_ref().map(|s| {
        format!(
            "{} tiles, {} stacks of {} via {}",
            s.n_tiles, s.stacks_per_p100, s.tiles_per_stack, s.merge_method
        )
    });

    let review = polish_mission_with_reviewer(&scenario, &class, &results, &status, &notes).await;
    if review.is_some() {
        notes.push("MTP reviewer polish: completed".to_string());
    }

    MissionReport {
        scenario_class: class,
        modules: results,
        stitching_summary,
        runtime_seconds: start.elapsed().as_secs_f32(),
        status: status.to_string(),
        notes,
        review,
    }
}

/// Ask the MTP reviewer pool for a short post-mission polish summary.
async fn polish_mission_with_reviewer(
    scenario: &OperatorScenario,
    class: &ScenarioClass,
    modules: &[ModuleResult],
    status: &str,
    notes: &[String],
) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .ok()?;
    let endpoint = crate::routing::first_online_llama(&client, &crate::routing::load_mtp_pool())
        .await?;
    let module_lines: Vec<String> = modules
        .iter()
        .map(|m| {
            format!(
                "- {}: {} ({})",
                m.module_id,
                m.status,
                m.data.as_deref().unwrap_or("").chars().take(120).collect::<String>()
            )
        })
        .collect();
    let prompt = format!(
        "You are the CESAROPS mission reviewer. In 5-8 bullet points:\n\
         1) Was the pipeline successful?\n\
         2) What failed or was skipped?\n\
         3) Next concrete steps for the operator.\n\n\
         Scenario: {:?}\nSpec: {:?}\nOverall status: {}\n\nModules:\n{}\n\nNotes:\n{}\n",
        class,
        scenario.spec_path,
        status,
        module_lines.join("\n"),
        notes.join("\n")
    );
    match crate::inference_client::complete_prompt(
        &client,
        &endpoint,
        &prompt,
        512,
        0.2,
        vec!["\n\n".to_string()],
        None,
    )
    .await
    {
        Ok(text) if !text.trim().is_empty() => Some(truncate(&text, 2500)),
        _ => None,
    }
}

async fn retry_plan(scenario: &OperatorScenario) -> Result<MissionPlan, String> {
    let mut last = String::new();
    for _ in 0..=PLAN_RETRY_CAP {
        match plan_from_scenario(scenario).await {
            Ok(p) => return Ok(p),
            Err(e) => last = e,
        }
    }
    Err(last)
}

// ---------------------------------------------------------------------------
// Axum handlers
// ---------------------------------------------------------------------------

pub async fn orchestrator_probe() -> Json<ClusterSnapshot> {
    match probe_cluster().await {
        Ok(s) => Json(s),
        Err(_) => Json(ClusterSnapshot { workers: Vec::new() }),
    }
}

pub async fn orchestrator_plan(Json(scenario): Json<OperatorScenario>) -> Json<serde_json::Value> {
    match plan_from_scenario(&scenario).await {
        Ok(plan) => Json(serde_json::json!({ "plan": plan })),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

pub async fn orchestrator_execute(Json(scenario): Json<OperatorScenario>) -> Json<MissionReport> {
    Json(execute_mission(scenario).await)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn http_client(secs: u64) -> Client {
    Client::builder()
        .timeout(Duration::from_secs(secs))
        .build()
        .unwrap_or_else(|_| Client::new())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…[+{} bytes]", &s[..max], s.len() - max)
    }
}
