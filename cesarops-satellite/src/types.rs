//! Shared types for the satellite pipeline.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Bounding-box ─────────────────────────────────────────────────────────────

/// [lat_min, lon_min, lat_max, lon_max]  (matches mission_control.py convention)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BBox {
    pub lat_min: f64,
    pub lon_min: f64,
    pub lat_max: f64,
    pub lon_max: f64,
}

impl BBox {
    pub fn from_slice(s: &[f64]) -> anyhow::Result<Self> {
        if s.len() != 4 {
            anyhow::bail!("bbox must be [lat_min, lon_min, lat_max, lon_max]");
        }
        Ok(Self { lat_min: s[0], lon_min: s[1], lat_max: s[2], lon_max: s[3] })
    }

    /// STAC-compatible [west, south, east, north]
    pub fn to_stac_array(&self) -> [f64; 4] {
        [self.lon_min, self.lat_min, self.lon_max, self.lat_max]
    }

    pub fn center(&self) -> (f64, f64) {
        (
            (self.lat_min + self.lat_max) / 2.0,
            (self.lon_min + self.lon_max) / 2.0,
        )
    }
}

// ── Pipeline knobs ────────────────────────────────────────────────────────────

/// All tunable parameters with defaults matching DEFAULT_KNOBS in the Python orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Knobs {
    // download
    pub sensors: String,
    pub max_download_results: usize,
    pub dry_run_download: bool,
    // targeting
    pub concepts: String,
    pub max_wrecks: usize,
    pub max_scenes: usize,
    pub min_score: f64,
    pub min_gt_score: f64,
    pub min_gt_hit_rate: f64,
    pub hit_zscore_min: f64,
    // optical POC
    pub max_cloud: f64,
    pub poc_zscore_threshold: f64,
    /// NMS window size (px) for `_find_peak_clusters`. 0 = use per-concept
    /// Python defaults (shadow=15, clarity=10, plume=10).
    pub poc_min_separation_px: usize,
    /// Max candidates returned per concept (`_find_peak_clusters` max_candidates).
    pub poc_max_candidates: usize,
    /// Longest-axis target after block-average `_downsample` (Python max_dim=2000).
    pub poc_downsample_max_dim: usize,
    /// `_cross_reference` nearby_radius_m (flag candidate if known wreck within this).
    pub xref_nearby_radius_m: f64,
    /// Post-storm scene window for the sediment_plume concept (YYYY-MM-DD).
    pub storm_date_start: String,
    pub storm_date_end: String,
    // local-scene offline mode
    /// Use on-disk tiles instead of STAC queries (for offline runs).
    pub use_local_scenes: Option<bool>,
    /// Target chip size for local band decode (pixels per side).
    pub downsample_max_dim: Option<usize>,
    // ground-truth / download selection
    /// Minimum WreckTarget confidence to keep when loading known wrecks.
    pub gt_min_confidence: f64,
    /// Calendar years to prioritise for Sentinel-2 download selection.
    /// When empty AND `auto_low_water_years > 0`, the pipeline auto-ranks the
    /// AOI's lake by historic low water level (see `lake_levels`).
    pub water_year_priority: Vec<i32>,
    /// If > 0 and `water_year_priority` is empty, auto-select this many years
    /// for the AOI using `scan_intent`. Low water raises wrecks toward the
    /// readable zone, but recent sinkings / clarity / spills want other years.
    pub auto_low_water_years: usize,
    /// Year-selection strategy when auto-selecting: "low_water_wreck" (default),
    /// "recent_sinking", "zebra_clarity", "event_response", or "generic".
    /// Low water is one strategy among several, not the only one.
    pub scan_intent: String,
    /// Length (days) of the contiguous low-cloud window pulled per priority
    /// year — the operator's "20-day no-cloud stack" workflow. 0 = whole year.
    pub stack_window_days: u32,
    // concept chip geometry (annular signal / background radii, metres)
    pub chip_signal_m: f64,
    pub chip_bg_inner_m: f64,
    pub chip_bg_outer_m: f64,
    pub chip_scene_radius_m: f64,
    // SAR DBSCAN temporal persistence
    pub dbscan_eps: f64,
    pub dbscan_min_samples: usize,
    // temporal stack
    pub temporal_chip_radius_m: f64,
    pub temporal_max_scenes: usize,
    pub temporal_ratio_channels: Vec<String>,
    pub temporal_persistence_z: f64,
    pub tiles_per_gpu_window: usize,
    pub gpu_windows: usize,
    // curvelet (future)
    pub use_curvelet_rescore: bool,
    pub curvelet_window_px: usize,
    pub curvelet_num_scales: usize,
    pub curvelet_energy_threshold: f64,
    // ── Triple-lock multi-sensor fusion ──────────────────────────────────────
    // Per-sensor anomaly-strength thresholds (z-score). A location must clear
    // the family threshold to contribute a "lock". Defaults are the values the
    // operator hand-tuned in triple_lock_fusion.py (now tunable, not hardcoded).
    /// Thermal cold-sink / heat-sink lock threshold (|z|).
    pub triple_lock_thermal_z: f64,
    /// SAR steel-mass lock threshold (z).
    pub triple_lock_sar_z: f64,
    /// Optical (clarity / glint / shadow) lock threshold (z).
    pub triple_lock_optical_z: f64,
    /// Temporal-persistence lock threshold (z).
    pub triple_lock_temporal_z: f64,
    /// Spatial tolerance (m) for treating hits as the "same location".
    pub triple_lock_tolerance_m: f64,
    /// Minimum distinct sensor families required to emit a lock (3 = full triple).
    pub triple_lock_min_locks: u8,
}

impl Default for Knobs {
    fn default() -> Self {
        Self {
            sensors: "sentinel2".into(),
            max_download_results: 25,
            dry_run_download: false,
            concepts: "all".into(),
            max_wrecks: 20,
            max_scenes: 8,
            min_score: 4.0,
            min_gt_score: 4.0,
            min_gt_hit_rate: 0.25,
            hit_zscore_min: 1.5,
            max_cloud: 20.0,
            poc_zscore_threshold: 2.5,
            poc_min_separation_px: 0, // 0 → per-concept Python defaults
            poc_max_candidates: 25,
            poc_downsample_max_dim: 2000,
            xref_nearby_radius_m: 2000.0,
            storm_date_start: "2024-01-13".into(),
            storm_date_end: "2024-01-20".into(),
            use_local_scenes: None,
            downsample_max_dim: None,
            gt_min_confidence: 0.0,
            water_year_priority: vec![],
            auto_low_water_years: 4,
            scan_intent: "low_water_wreck".into(),
            stack_window_days: 20,
            chip_signal_m: 150.0,
            chip_bg_inner_m: 350.0,
            chip_bg_outer_m: 1200.0,
            chip_scene_radius_m: 1500.0,
            dbscan_eps: 0.5,
            dbscan_min_samples: 5,
            temporal_chip_radius_m: 600.0,
            temporal_max_scenes: 12,
            temporal_ratio_channels: vec!["ndwi".into(), "ndvi".into()],
            temporal_persistence_z: 2.0,
            tiles_per_gpu_window: 10,
            gpu_windows: 2,
            use_curvelet_rescore: false,
            curvelet_window_px: 64,
            curvelet_num_scales: 5,
            curvelet_energy_threshold: 2.5,
            // Triple-lock defaults = operator's hand-tuned values from
            // triple_lock_fusion.py (thermal/sar/optical = 2.5, fuse 300 m).
            triple_lock_thermal_z: 2.5,
            triple_lock_sar_z: 2.5,
            triple_lock_optical_z: 2.5,
            triple_lock_temporal_z: 2.0,
            triple_lock_tolerance_m: 300.0,
            triple_lock_min_locks: 3,
        }
    }
}

// ── Mission spec ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Download,
    TargetKnown,
    PocAoi,
    SarLocal,
    BagLocal,
    BathyMap,
    TemporalStack,
    ValidateGt,
    Report,
}

/// Full mission specification — mirrors missions/*.json structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionSpec {
    pub mission_id: String,
    pub target_name: Option<String>,
    /// [lat_min, lon_min, lat_max, lon_max]
    pub bbox: Vec<f64>,
    pub days_back: Option<u32>,
    pub stages: Option<Vec<Stage>>,
    #[serde(default)]
    pub knobs: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub paths: HashMap<String, String>,
    /// Optional ground-truth wreck name filters
    pub gt_wreck_names: Option<Vec<String>>,
}

impl MissionSpec {
    pub fn bbox(&self) -> anyhow::Result<BBox> {
        BBox::from_slice(&self.bbox)
    }

    pub fn effective_stages(&self) -> Vec<Stage> {
        self.stages.clone().unwrap_or_else(|| {
            vec![Stage::Download, Stage::TargetKnown, Stage::ValidateGt, Stage::Report]
        })
    }

    pub fn days_back(&self) -> u32 {
        self.days_back.unwrap_or(14)
    }
}

// ── Wreck ground-truth ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WreckTarget {
    pub id: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub depth_m: f64,
    #[serde(rename = "type", default)]
    pub wreck_type: String,
    #[serde(default)]
    pub confidence: String,
}

// ── Concept scoring ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptResult {
    pub wreck_id: String,
    pub wreck_name: String,
    pub lat: f64,
    pub lon: f64,
    pub depth_m: f64,
    pub concept: String,
    pub n_scenes: usize,
    pub n_hits: usize,
    pub hit_rate: f64,
    pub mean_zscore: f64,
    pub best_zscore: f64,
    pub best_date: Option<NaiveDate>,
    /// 0–10 composite score
    pub score: f64,
    #[serde(default)]
    pub notes: String,
}

// ── Candidate (fusion output) ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub lat: f64,
    pub lon: f64,
    pub depth_m: f64,
    /// Composite score across all active signals (0–10)
    pub composite_score: f64,
    /// Map from signal name → individual score
    pub signals: HashMap<String, f64>,
    /// Where the signal was strongest
    pub best_concept: Option<String>,
    pub best_date: Option<NaiveDate>,
    #[serde(default)]
    pub notes: String,
}

// ── Validation result ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationEntry {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub score: f64,
    pub hit_rate: f64,
    pub concept: String,
    pub status: String,
    pub pass: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub mission_id: String,
    pub min_gt_score: f64,
    pub min_gt_hit_rate: f64,
    pub n_gt: usize,
    pub n_pass: usize,
    pub pass_rate: f64,
    pub wrecks: Vec<ValidationEntry>,
}

// ── SAR temporal-persistence cluster ──────────────────────────────────────────

/// A spatial cluster of persistent SAR returns.
///
/// Mirrors the `SarCluster` dataclass referenced by `nasa_fusion_test.py`
/// (`SarCluster(lat, lon, persistence, confidence, cluster_id, props)`) and the
/// per-cluster output of `sar_temporal_persistence.py::calculate_persistence`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SarCluster {
    pub cluster_id: i64,
    pub lat: f64,
    pub lon: f64,
    /// Temporal persistence — count / fraction of scenes the cluster recurs in.
    pub persistence: f64,
    /// 0–1 confidence (defaults to persistence-derived when not supplied).
    #[serde(default)]
    pub confidence: f64,
    /// Number of member points in the cluster.
    #[serde(default)]
    pub n_points: usize,
    /// Orbit direction the cluster came from ("ascending" / "descending" / "").
    #[serde(default)]
    pub orbit: String,
    /// Free-form properties (e.g. {"name": "Big Tub Harbor"}).
    #[serde(default)]
    pub props: HashMap<String, serde_json::Value>,
}

// ── Mission report ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StageResults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_known: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poc_aoi: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sar_local: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bag_local: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bathy_map: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal_stack: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validate_gt: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionReport {
    pub mission_id: String,
    pub target_name: String,
    pub bbox: Vec<f64>,
    pub stages: Vec<Stage>,
    pub dry_run: bool,
    pub started_at: String,
    pub runtime_seconds: f64,
    pub status: String,
    pub stage_results: StageResults,
    pub candidates: Vec<Candidate>,
    /// Multi-sensor triple-lock detections (independent sensor-family agreement).
    #[serde(default)]
    pub triple_locks: Vec<crate::triple_lock::TripleLockTarget>,
}
