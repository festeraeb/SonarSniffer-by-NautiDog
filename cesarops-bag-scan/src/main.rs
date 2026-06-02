//! CESAROPS BAG scanner CLI.
//!
//! Replaces the old monolithic stub. Orchestrates stages A->G via
//! [`cesarops_bag_scan::pipeline`] and prints a [`MissionReport`] as JSON whose
//! detections carry the `signature_type` contract consumed by
//! `pipelines/bag/wreckhunter2000/validate_geo*.py`.
//!
//! CLI compatibility: the old `--threshold` and `--redaction_sensitivity` flags
//! are preserved (they override the matching knobs). New flags `--knobs` (JSON
//! overlay) and `--stages` (subset selection) are added.

use clap::Parser;
use cesarops_bag_scan::pipeline;
use cesarops_bag_scan::types::{Knobs, Stage};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "NOAA BAG scanner (GDAL) — wreck + redaction detection"
)]
struct Args {
    /// Path to the .bag file (not required with --describe)
    path: Option<String>,

    /// Emit a machine-readable tool catalog (stages, knobs, defaults) as JSON
    /// for the LLM/n8n orchestrator, then exit.
    #[arg(long)]
    describe: bool,

    /// Z-score / height threshold (legacy). Overrides knobs.anomaly_threshold.
    #[arg(short, long)]
    threshold: Option<f64>,

    /// Sensitivity for redaction detection 0..1 (legacy).
    /// Overrides knobs.redaction_sensitivity.
    #[arg(short, long)]
    redaction_sensitivity: Option<f64>,

    /// Enable the heavy elevation-based redaction signature detectors.
    #[arg(long)]
    enable_redaction_signatures: bool,

    /// JSON object overlaying any subset of the knobs onto the defaults,
    /// e.g. --knobs '{"min_height_m": 1.0, "merge_radius_m": 150}'
    #[arg(long)]
    knobs: Option<String>,

    /// Comma-separated stage subset to run (a..g or names), e.g.
    /// --stages a,c,d,g . Defaults to all stages A->G.
    #[arg(long)]
    stages: Option<String>,

    /// Compact (non-pretty) JSON output.
    #[arg(long)]
    compact: bool,

    /// Reconstruct + export georeferenced rasters (recon/diff/hillshade GeoTIFF)
    /// for each masked region into this directory. Implies the redaction stage.
    #[arg(long, value_name = "DIR")]
    unmask: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    if args.describe {
        println!("{}", describe_catalog());
        return Ok(());
    }

    // Build knobs: defaults <- --knobs JSON overlay <- legacy flag overrides.
    let mut knobs: Knobs = match &args.knobs {
        Some(json) => Knobs::from_json_overlay(json)?,
        None => Knobs::default(),
    };
    if let Some(t) = args.threshold {
        knobs.anomaly_threshold = t;
    }
    if let Some(rs) = args.redaction_sensitivity {
        knobs.redaction_sensitivity = rs;
    }
    if args.enable_redaction_signatures {
        knobs.enable_redaction_signatures = true;
    }

    // Resolve stages.
    let mut stages: Vec<Stage> = match &args.stages {
        Some(s) => {
            let parsed: Vec<Stage> = s
                .split(',')
                .filter_map(Stage::parse_token)
                .collect();
            if parsed.is_empty() {
                eprintln!("warning: no valid stages in '{s}', running all stages");
                Stage::all()
            } else {
                parsed
            }
        }
        None => Stage::all(),
    };

    // --unmask implies the redaction stage (it needs masked regions to rebuild).
    if args.unmask.is_some() && !stages.contains(&Stage::Redaction) {
        stages.push(Stage::Redaction);
    }

    let path = match &args.path {
        Some(p) => p,
        None => {
            eprintln!("error: <PATH> to a .bag file is required (or use --describe)");
            std::process::exit(2);
        }
    };
    let report = pipeline::run_with_unmask(path, &knobs, &stages, args.unmask.as_deref())?;

    let json = if args.compact {
        serde_json::to_string(&report)?
    } else {
        serde_json::to_string_pretty(&report)?
    };
    println!("{json}");
    Ok(())
}

/// Build the machine-readable tool catalog for the LLM / n8n orchestrator.
///
/// Emits the pipeline identity, selectable stages (A→G), and the full `Knobs`
/// set with defaults (introspected from `Knobs::default()`). The output
/// detections carry the `signature_type` contract (physical_wreck /
/// masked_redaction_flat). See docs/PIPELINE_UNIFICATION_ARCHITECTURE.md.
fn describe_catalog() -> String {
    let knob_defaults = serde_json::to_value(Knobs::default()).unwrap_or(serde_json::Value::Null);
    let catalog = serde_json::json!({
        "pipeline": "bag",
        "binary": "cesarops-bag-scan",
        "description": "NOAA BAG bathymetry wreck + redaction/unmask detection",
        "version": env!("CARGO_PKG_VERSION"),
        "stages": [
            { "name": "a_read",        "desc": "Read BAG elevation + uncertainty, nodata->NaN, downsample" },
            { "name": "b_geo",         "desc": "Grid->projected->WGS84 reprojection (GDAL OSR)" },
            { "name": "c_anomaly",     "desc": "Seafloor background + height-above-floor anomaly clustering" },
            { "name": "d_redaction",   "desc": "Redaction/unmask signature suite + masked-region scan" },
            { "name": "e_orientation", "desc": "PCA heading/length/width + compass bearing" },
            { "name": "f_dedup",       "desc": "Spatial dedup merge within merge_radius_m" },
            { "name": "g_report",      "desc": "Emit MissionReport with physical_wreck/masked_redaction_flat detections" }
        ],
        "knobs": knob_defaults,
        "inputs": {
            "path": "Path to a .bag file",
            "knobs": "JSON object overlaying any subset of the knobs above",
            "stages": "Comma-separated stage subset (a..g or names)",
            "threshold": "Legacy: overrides knobs.anomaly_threshold",
            "redaction_sensitivity": "Legacy: overrides knobs.redaction_sensitivity"
        },
        "output": {
            "format": "MissionReport JSON",
            "fields": ["file", "grid_size", "epsg_code", "detections[]", "physical_wreck_count", "masked_redaction_count"],
            "contract": "detections[].signature_type in {physical_wreck, masked_redaction_flat}"
        }
    });
    serde_json::to_string_pretty(&catalog).unwrap_or_default()
}
