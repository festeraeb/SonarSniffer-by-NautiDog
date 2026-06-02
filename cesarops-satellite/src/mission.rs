//! Pipeline stage runner — orchestrates all stages for a MissionSpec.
//!
//! Ports `run_mission()` from sat_mission_orchestrator.py with full stage list:
//!   download → target_known → poc_aoi → temporal_stack → validate_gt → report

use crate::{
    concept::score_wreck_all_concepts,
    downloads::preflight_sources,
    fusion::{fuse_candidates, validate_against_gt},
    stac::{search_scenes_post, StacQuery},
    temporal::run_temporal_stack_mission,
    types::{
        BBox, Knobs, MissionReport, MissionSpec, Stage, StageResults,
        WreckTarget,
    },
};
use anyhow::{Context, Result};
use chrono::Local;
use reqwest::Client;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Instant,
};
use tracing::{info, warn};

// ── Paths helper ──────────────────────────────────────────────────────────────

pub struct MissionPaths {
    pub output_dir: PathBuf,
    pub chip_cache_dir: PathBuf,
    pub known_wrecks_json: Option<PathBuf>,
    pub download_dir: PathBuf,
}

impl MissionPaths {
    fn from_spec(spec: &MissionSpec, default_root: &Path) -> Self {
        let out = spec
            .paths
            .get("output_dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| default_root.join("detection_runs").join(&spec.mission_id));
        let chips = spec
            .paths
            .get("chip_cache_dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| out.join("chip_cache"));
        let dl = spec
            .paths
            .get("download_dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| default_root.join("downloads").join(&spec.mission_id));
        let kw = spec
            .paths
            .get("known_wrecks_json")
            .map(PathBuf::from);

        Self { output_dir: out, chip_cache_dir: chips, known_wrecks_json: kw, download_dir: dl }
    }
}

// ── Knobs resolver ────────────────────────────────────────────────────────────

/// Deep-merge spec.knobs (JSON map) onto Knobs::default().
pub fn resolve_knobs(spec: &MissionSpec, overrides: Option<HashMap<String, Value>>) -> Knobs {
    let mut base = serde_json::to_value(Knobs::default()).unwrap();
    // Apply spec.knobs
    if let Value::Object(ref mut map) = base {
        for (k, v) in &spec.knobs {
            map.insert(k.clone(), v.clone());
        }
        if let Some(ov) = overrides {
            for (k, v) in ov {
                map.insert(k, v);
            }
        }
    }
    serde_json::from_value(base).unwrap_or_default()
}

// ── Known-wreck loader ────────────────────────────────────────────────────────

/// Map a known-wreck `confidence` JSON value to a 0–1 score for the
/// `gt_min_confidence` knob filter.  Accepts numbers ("0.75") and the common
/// category strings used in known_wrecks.json.
fn confidence_to_f64(v: Option<&serde_json::Value>) -> f64 {
    match v {
        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(serde_json::Value::String(s)) => {
            // Try a numeric string first, then fall back to categories.
            if let Ok(f) = s.trim().parse::<f64>() {
                return f;
            }
            match s.trim().to_lowercase().as_str() {
                "high" | "verified" | "dive_verified" | "confirmed" => 1.0,
                "medium" | "moderate" | "probable" => 0.6,
                "low" | "possible" | "unverified" => 0.3,
                _ => 0.0,
            }
        }
        _ => 0.0,
    }
}

fn load_known_wrecks(
    paths: &MissionPaths,
    bbox: BBox,
    knobs: &Knobs,
    gt_wreck_names: &Option<Vec<String>>,
) -> Vec<WreckTarget> {
    let json_path = match paths.known_wrecks_json.as_deref() {
        Some(p) if p.is_file() => p.to_path_buf(),
        _ => {
            // Common fallback locations
            let candidates = [
                PathBuf::from("/codebase/repos/wreckhunter2000-1/data/known_wrecks_straits.json"),
                PathBuf::from("/codebase/repos/wreckhunter2000-1/data/known_wrecks.json"),
                PathBuf::from("/codebase/repos/wreckhunter2000-1/backup/deploy/tools/cesarops-core-github/known_wrecks.json"),
                PathBuf::from("/codebase/repos/wreckhunter2000-1/backup/wreckhunter2000-1/known_wrecks.json"),
            ];
            match candidates.iter().find(|p| p.is_file()) {
                Some(p) => p.clone(),
                None => {
                    warn!("known_wrecks.json not found; no GT wrecks loaded");
                    return vec![];
                }
            }
        }
    };

    let raw = match std::fs::read_to_string(&json_path) {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to read {}: {e}", json_path.display());
            return vec![];
        }
    };

    let map: HashMap<String, serde_json::Value> = match serde_json::from_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            warn!("Failed to parse known_wrecks.json: {e}");
            return vec![];
        }
    };

    let want_names: Vec<String> = gt_wreck_names
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|n| n.to_lowercase())
        .collect();

    let mut out: Vec<WreckTarget> = map
        .into_iter()
        .filter_map(|(id, rec)| {
            let lat = (rec["lat_min"].as_f64()? + rec["lat_max"].as_f64()?) / 2.0;
            let lon = (rec["lon_min"].as_f64()? + rec["lon_max"].as_f64()?) / 2.0;
            let name = rec.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string();

            // Name filter
            if !want_names.is_empty() {
                let nl = name.to_lowercase();
                if !want_names.iter().any(|w| nl.contains(w.as_str()) || w.contains(&nl)) {
                    return None;
                }
            }

            // Bbox filter (skip when using explicit name list)
            if want_names.is_empty() {
                if !(bbox.lat_min <= lat && lat <= bbox.lat_max && bbox.lon_min <= lon && lon <= bbox.lon_max) {
                    return None;
                }
            }

            // Confidence filter (gt_min_confidence knob).  Confidence may be
            // numeric ("0.75") or a category ("high"/"medium"/"low").  Only
            // applied when the knob is > 0 (default 0 → keep everything).
            if knobs.gt_min_confidence > 0.0 {
                let conf_val = confidence_to_f64(rec.get("confidence"));
                if conf_val < knobs.gt_min_confidence {
                    return None;
                }
            }

            let depth_m = rec.get("depth_ft").and_then(|v| v.as_f64()).unwrap_or(0.0) * 0.3048;
            Some(WreckTarget {
                id: id.clone(),
                name,
                lat,
                lon,
                depth_m,
                wreck_type: rec.get("type").and_then(|v| v.as_str()).unwrap_or("").into(),
                confidence: rec.get("confidence").and_then(|v| v.as_str()).unwrap_or("").into(),
            })
        })
        .collect();

    out.sort_by(|a, b| a.name.cmp(&b.name));
    let max_w = knobs.max_wrecks;
    if out.len() > max_w {
        out.truncate(max_w);
    }
    info!("Loaded {} GT wrecks from {}", out.len(), json_path.display());
    out
}

// ── Stage: download ────────────────────────────────────────────────────────────

async fn stage_download(
    spec: &MissionSpec,
    knobs: &Knobs,
    paths: &MissionPaths,
    dry_run: bool,
) -> Value {
    if dry_run || knobs.dry_run_download {
        return serde_json::json!({ "skipped": true, "dry_run": true, "download_dir": paths.download_dir });
    }
    // Delegate to Python universal_downloader via subprocess (mirrors original Python behaviour)
    let bbox = match spec.bbox() {
        Ok(b) => b,
        Err(e) => return serde_json::json!({ "error": e.to_string() }),
    };
    let days_back = spec.days_back();
    let end = Local::now().naive_local().date();
    let start = end - chrono::Duration::days(days_back as i64);

    let http = match Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return serde_json::json!({
                "error": format!("download preflight client init failed: {e}")
            })
        }
    };
    let preflight = preflight_sources(&http, &knobs.sensors, false).await;
    if !preflight.ok {
        return serde_json::json!({
            "rc": 2,
            "error": "download preflight failed",
            "preflight": preflight,
        });
    }

    let sensor_tokens: Vec<String> = knobs
        .sensors
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let native_s2_only = !sensor_tokens.is_empty()
        && sensor_tokens.iter().all(|s| {
            matches!(s.as_str(), "sentinel2" | "optical" | "stac")
        });

    if native_s2_only {
        if let Err(e) = std::fs::create_dir_all(&paths.download_dir) {
            return serde_json::json!({
                "rc": 1,
                "error": format!("failed creating download dir: {e}"),
                "download_dir": paths.download_dir,
                "preflight": preflight,
            });
        }

        let mut scenes: Vec<crate::stac::Scene> = Vec::new();
        let mut strategy = "date_window_cloud".to_string();
        let mut water_years: Vec<i32> = spec
            .knobs
            .get("water_year_priority")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_i64())
                    .map(|x| x as i32)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Auto-rank low-water years when none are pinned. Low water raises wrecks
        // toward the readable zone, so we prefer the lowest-level years the
        // sensor can actually serve. Sentinel-2's archive (2017+) cannot reach
        // the 2012-2013 Great Lakes record lows — for those, set sensors to
        // landsat (lake_levels clamps per sensor so we never request a year the
        // sensor can't serve).
        if water_years.is_empty() && knobs.auto_low_water_years > 0 {
            let (clat, clon) = bbox.center();
            let sensor = crate::lake_levels::SensorFamily::from_str(&knobs.sensors);
            let intent = crate::lake_levels::ScanIntent::from_str(&knobs.scan_intent);
            water_years = crate::lake_levels::select_years(
                clat,
                clon,
                sensor,
                intent,
                knobs.auto_low_water_years,
            );
            if !water_years.is_empty() {
                strategy = format!("auto_{}", knobs.scan_intent);
                if intent == crate::lake_levels::ScanIntent::LowWaterWreck
                    && !crate::lake_levels::sensor_reaches_record_low(clat, clon, sensor)
                {
                    warn!(
                        "auto low-water years {:?} are the best {:?} can serve; the basin's \
                         record-low years predate this sensor's archive — use sensors=landsat \
                         to reach the true lows",
                        water_years, sensor
                    );
                }
            }
        }

        if !water_years.is_empty() {
            if strategy == "date_window_cloud" {
                strategy = "water_year_priority_then_cloud".to_string();
            }
            let mut remaining = knobs.max_download_results;
            // Contiguous low-cloud window per year (the operator's "20-day
            // no-cloud stack"): when stack_window_days > 0 we still query the
            // whole year but the temporal stack stage tightens to the best
            // contiguous window; the download keeps the lowest-cloud scenes.
            for wy in &water_years {
                if remaining == 0 {
                    break;
                }
                let y_start = match chrono::NaiveDate::from_ymd_opt(*wy, 1, 1) {
                    Some(d) => d,
                    None => continue,
                };
                let y_end = match chrono::NaiveDate::from_ymd_opt(*wy, 12, 31) {
                    Some(d) => d,
                    None => continue,
                };
                let q = StacQuery {
                    bbox: bbox.to_stac_array(),
                    date_start: y_start,
                    date_end: y_end,
                    max_cloud: knobs.max_cloud,
                    month_filter: &[],
                    limit: remaining,
                };
                match search_scenes_post(&http, &q).await {
                    Ok(mut found) => {
                        found.sort_by(|a, b| a.cloud_cover.partial_cmp(&b.cloud_cover).unwrap_or(std::cmp::Ordering::Equal));
                        if found.len() > remaining {
                            found.truncate(remaining);
                        }
                        remaining = remaining.saturating_sub(found.len());
                        scenes.extend(found);
                    }
                    Err(e) => {
                        warn!("water-year query failed for {}: {e}", wy);
                    }
                }
            }
        } else {
            let q = StacQuery {
                bbox: bbox.to_stac_array(),
                date_start: start,
                date_end: end,
                max_cloud: knobs.max_cloud,
                month_filter: &[],
                limit: knobs.max_download_results,
            };
            match search_scenes_post(&http, &q).await {
                Ok(found) => scenes = found,
                Err(e) => {
                    warn!("native sentinel2 download path failed, falling back to python: {e}");
                    scenes.clear();
                }
            }
        }

        if !scenes.is_empty() {
            let manifest = scenes
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "id": s.id,
                        "datetime": s.datetime,
                        "cloud_cover": s.cloud_cover,
                        "assets": s.assets,
                    })
                })
                .collect::<Vec<_>>();
                let manifest_path = paths.download_dir.join("sentinel2_stac_manifest.json");
                if let Err(e) = std::fs::write(
                    &manifest_path,
                    serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "[]".to_string()),
                ) {
                    return serde_json::json!({
                        "rc": 1,
                        "error": format!("failed writing manifest: {e}"),
                        "download_dir": paths.download_dir,
                        "preflight": preflight,
                    });
                }
                return serde_json::json!({
                    "rc": 0,
                    "mode": "rust_native_stac_manifest",
                    "selection_strategy": strategy,
                    "water_year_priority": water_years,
                    "download_dir": paths.download_dir,
                    "manifest": manifest_path,
                    "n_scenes": scenes.len(),
                    "date_range": [start.to_string(), end.to_string()],
                    "preflight": preflight,
                });
        }

        if !water_years.is_empty() {
            return serde_json::json!({
                "rc": 0,
                "mode": "rust_native_stac_manifest",
                "selection_strategy": strategy,
                "water_year_priority": water_years,
                "download_dir": paths.download_dir,
                "n_scenes": 0,
                "date_range": [start.to_string(), end.to_string()],
                "warning": "no Sentinel-2 scenes matched requested water_year_priority",
                "preflight": preflight,
            });
        }
    }

    let dl_script = std::path::Path::new("/codebase/repos/wreckhunter2000-1/universal_downloader.py");
    if !dl_script.exists() {
        return serde_json::json!({ "skipped": true, "reason": "universal_downloader.py not found", "preflight": preflight });
    }

    let bbox_arg = format!("{},{},{},{}", bbox.lat_min, bbox.lon_min, bbox.lat_max, bbox.lon_max);
    let output = tokio::process::Command::new("python3")
        .arg(dl_script)
        .arg("--bbox").arg(&bbox_arg)
        .arg("--dates").arg(start.to_string()).arg(end.to_string())
        .arg("--sensors").arg(&knobs.sensors)
        .arg("--max-results").arg(knobs.max_download_results.to_string())
        .arg("--output").arg(&paths.download_dir)
        .output()
        .await;

    match output {
        Ok(o) => serde_json::json!({
            "rc": o.status.code(),
            "download_dir": paths.download_dir,
            "date_range": [start.to_string(), end.to_string()],
            "preflight": preflight,
        }),
        Err(e) => serde_json::json!({ "error": e.to_string() }),
    }
}

// ── Stage: target_known ───────────────────────────────────────────────────────

async fn stage_target_known(
    client: &Client,
    wrecks: &[WreckTarget],
    knobs: &Knobs,
    paths: &MissionPaths,
    dry_run: bool,
) -> (Value, Vec<crate::types::ConceptResult>) {
    if dry_run || wrecks.is_empty() {
        return (
            serde_json::json!({ "skipped": dry_run, "n_wrecks": wrecks.len() }),
            vec![],
        );
    }

    let targeting_out = paths.output_dir.join("wreck_targeting");
    std::fs::create_dir_all(&targeting_out).ok();

    let mut all_results: Vec<crate::types::ConceptResult> = Vec::new();
    for wreck in wrecks {
        let results =
            score_wreck_all_concepts(client, wreck, knobs, &paths.chip_cache_dir).await;
        all_results.extend(results);
    }

    // Write CSV
    let csv = crate::concept::results_to_csv(&all_results);
    let csv_path = targeting_out.join("wreck_targets_all.csv");
    if let Err(e) = std::fs::write(&csv_path, &csv) {
        warn!("Failed to write CSV: {e}");
    }

    let rc_val = serde_json::json!({
        "rc": 0,
        "output_dir": targeting_out,
        "n_wrecks": wrecks.len(),
        "n_results": all_results.len()
    });
    (rc_val, all_results)
}

// ── Stage: poc_aoi ────────────────────────────────────────────────────────────

async fn stage_poc_aoi(
    client: &Client,
    spec: &MissionSpec,
    wrecks: &[WreckTarget],
    knobs: &Knobs,
    paths: &MissionPaths,
    dry_run: bool,
) -> Value {
    if dry_run {
        return serde_json::json!({ "skipped": true });
    }
    // Native full-scene optical POC — ports wh2k_sentinel_optical_poc.py
    // (previously this stage shelled out to `python3`).
    let bbox = match spec.bbox() {
        Ok(b) => b,
        Err(e) => return serde_json::json!({ "error": e.to_string() }),
    };
    let days_back = spec.days_back();
    let end = Local::now().naive_local().date();
    let start = end - chrono::Duration::days(days_back as i64);
    let poc_out = paths.output_dir.join("sentinel_optical");
    std::fs::create_dir_all(&poc_out).ok();

    // Known wrecks → (lat, lon) for `_cross_reference`.
    let known: Vec<(f64, f64)> = wrecks.iter().map(|w| (w.lat, w.lon)).collect();

    let outcome = crate::poc::run_poc_aoi(
        client,
        &bbox,
        start,
        end,
        knobs,
        &paths.chip_cache_dir,
        &known,
    )
    .await;

    match outcome {
        Ok(o) => {
            // Persist candidates as JSON (mirrors optical_all_concepts.json).
            let cand_path = poc_out.join("optical_all_concepts.json");
            if let Err(e) = std::fs::write(
                &cand_path,
                serde_json::to_string_pretty(&o.candidates).unwrap_or_else(|_| "[]".into()),
            ) {
                warn!("Failed to write POC candidates: {e}");
            }
            let min_score = knobs.min_score;
            let n_kept = o
                .candidates
                .iter()
                .filter(|c| c.wreck_score as f64 >= min_score)
                .count();
            serde_json::json!({
                "rc": 0,
                "mode": "rust_native_optical_poc",
                "output_dir": poc_out,
                "n_scenes": o.n_scenes,
                "clear_date": o.clear_date,
                "storm_date": o.storm_date,
                "n_candidates": o.candidates.len(),
                "n_candidates_above_min_score": n_kept,
            })
        }
        Err(e) => serde_json::json!({ "error": e.to_string() }),
    }
}

// ── Stage: temporal_stack ─────────────────────────────────────────────────────

async fn stage_temporal_stack(
    client: &Client,
    spec: &MissionSpec,
    wrecks: &[WreckTarget],
    knobs: &Knobs,
    paths: &MissionPaths,
    dry_run: bool,
) -> (Value, HashMap<String, f64>) {
    if dry_run {
        return (serde_json::json!({ "skipped": true }), HashMap::new());
    }
    let bbox = match spec.bbox() {
        Ok(b) => b,
        Err(e) => return (serde_json::json!({ "error": e.to_string() }), HashMap::new()),
    };
    let stack_out = paths.output_dir.join("temporal_stack");
    let report = run_temporal_stack_mission(
        client,
        bbox,
        wrecks,
        &stack_out,
        knobs,
        spec.days_back(),
        &paths.chip_cache_dir,
    )
    .await;

    match report {
        Ok(r) => {
            // Extract per-wreck max persistence z
            let tz_map: HashMap<String, f64> = r
                .wrecks
                .iter()
                .filter_map(|w| {
                    let z = [w.ndwi_persistence_z, w.ndvi_persistence_z]
                        .iter()
                        .filter_map(|&z| z)
                        .map(|z| z.abs())
                        .fold(0.0_f64, f64::max);
                    if z > 0.0 {
                        // Use wreck name as id (same key used in concept results)
                        Some((w.name.clone(), z))
                    } else {
                        None
                    }
                })
                .collect();
            let val = serde_json::json!({
                "rc": 0,
                "output_dir": stack_out,
                "n_scenes": r.n_scenes_catalog,
                "n_anomalies": r.wrecks.iter().filter(|w| w.anomaly).count()
            });
            (val, tz_map)
        }
        Err(e) => (serde_json::json!({ "error": e.to_string() }), HashMap::new()),
    }
}

// ── Main runner ────────────────────────────────────────────────────────────────

pub struct RunOptions {
    pub dry_run: bool,
    pub knob_overrides: Option<HashMap<String, Value>>,
    pub pipeline_root: PathBuf,
}

pub async fn run_mission(spec: MissionSpec, opts: RunOptions) -> Result<MissionReport> {
    let t0 = Instant::now();
    let started_at = Local::now().to_rfc3339();

    let knobs = resolve_knobs(&spec, opts.knob_overrides);
    let paths = MissionPaths::from_spec(&spec, &opts.pipeline_root);
    std::fs::create_dir_all(&paths.output_dir)
        .with_context(|| format!("creating output dir {}", paths.output_dir.display()))?;

    let bbox = spec.bbox()?;
    let stages = spec.effective_stages();
    let dry_run = opts.dry_run;

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let wrecks = load_known_wrecks(&paths, bbox, &knobs, &spec.gt_wreck_names);

    let mut stage_results = StageResults::default();
    let mut all_concept_results: Vec<crate::types::ConceptResult> = Vec::new();
    let mut temporal_zscores: HashMap<String, f64> = HashMap::new();

    for stage in &stages {
        info!("━━━ stage: {stage:?} ━━━");
        match stage {
            Stage::Download => {
                let r = stage_download(&spec, &knobs, &paths, dry_run).await;
                stage_results.download = Some(r);
            }
            Stage::TargetKnown => {
                let (r, results) =
                    stage_target_known(&client, &wrecks, &knobs, &paths, dry_run).await;
                all_concept_results.extend(results);
                stage_results.target_known = Some(r);
            }
            Stage::PocAoi => {
                let r = stage_poc_aoi(&client, &spec, &wrecks, &knobs, &paths, dry_run).await;
                stage_results.poc_aoi = Some(r);
            }
            Stage::TemporalStack => {
                let (r, tz) = stage_temporal_stack(
                    &client, &spec, &wrecks, &knobs, &paths, dry_run,
                )
                .await;
                temporal_zscores.extend(tz);
                stage_results.temporal_stack = Some(r);
            }
            Stage::ValidateGt => {
                let vr = validate_against_gt(
                    &all_concept_results,
                    &wrecks,
                    knobs.min_gt_score,
                    knobs.min_gt_hit_rate,
                    &spec.mission_id,
                );
                let vr_path = paths.output_dir.join("validation_report.json");
                let _ = std::fs::write(&vr_path, serde_json::to_string_pretty(&vr)?);
                info!("Validation: {}/{} pass → {}", vr.n_pass, vr.n_gt, vr_path.display());
                stage_results.validate_gt = Some(serde_json::to_value(&vr)?);
            }
            Stage::Report => {}
        }
    }

    // Fuse candidates
    let candidates = fuse_candidates(
        &all_concept_results,
        if temporal_zscores.is_empty() { None } else { Some(&temporal_zscores) },
        None,
        knobs.min_score,
    );

    // Determine overall status
    let status = if stage_results.download.as_ref()
        .and_then(|r| r.get("rc"))
        .and_then(|v| v.as_i64())
        .map_or(false, |rc| rc != 0)
    {
        "partial"
    } else {
        "ok"
    };

    let report = MissionReport {
        mission_id: spec.mission_id.clone(),
        target_name: spec.target_name.clone().unwrap_or_default(),
        bbox: spec.bbox.clone(),
        stages,
        dry_run,
        started_at,
        runtime_seconds: t0.elapsed().as_secs_f64(),
        status: status.into(),
        stage_results,
        candidates,
    };

    let report_path = paths.output_dir.join("mission_report.json");
    std::fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;
    info!("Wrote {}", report_path.display());
    Ok(report)
}
