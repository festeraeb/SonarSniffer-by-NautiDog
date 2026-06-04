//! `sat-run` — unified satellite pipeline CLI
//!
//! Usage:
//!   sat-run --spec missions/straits_known_wreck.json
//!   sat-run --spec missions/...json --knobs '{"max_scenes":12}'
//!   sat-run --spec missions/...json --dry-run
//!   sat-run --spec missions/...json --stages download target_known validate_gt report

use anyhow::Result;
use clap::Parser;
use cesarops_satellite::downloads::preflight_sources;
use cesarops_satellite::mission::{run_mission, RunOptions};
use cesarops_satellite::types::MissionSpec;
use std::{collections::HashMap, path::PathBuf};
use tracing_subscriber::EnvFilter;

// High-performance global allocator (opt-in via `jemalloc` feature). Avoids
// cross-socket allocator contention on the dual-socket Xeon fleet. Unix-only.
#[cfg(all(unix, feature = "jemalloc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[derive(Parser, Debug)]
#[command(
    name = "sat-run",
    about = "Unified satellite pipeline — wreck hunting & search/rescue (water + land)",
    version
)]
struct Args {
    /// Emit a machine-readable tool catalog (stages, knobs, defaults, inputs)
    /// as JSON for the LLM/n8n orchestrator, then exit.
    #[arg(long)]
    describe: bool,

    /// Run only downloader/source preflight and exit
    #[arg(long)]
    preflight_downloads: bool,

    /// Sensor classes for preflight (comma-separated), e.g. sentinel2,sar,hls,landsat
    #[arg(long, default_value = "all")]
    preflight_sensors: String,

    /// Require auth env vars for auth-protected sources during preflight
    #[arg(long)]
    strict_auth: bool,

    /// Path to mission spec JSON (MissionSpec format)
    #[arg(long, short)]
    spec: Option<PathBuf>,

    /// JSON object of knob overrides, e.g. '{"max_scenes":12}'
    #[arg(long, short)]
    knobs: Option<String>,

    /// Run without downloading or issuing real STAC queries
    #[arg(long)]
    dry_run: bool,

    /// Override stages (space-separated): download target_known poc_aoi temporal_stack validate_gt report
    #[arg(long, num_args = 1..)]
    stages: Option<Vec<String>>,

    /// Root directory for output files (default: cwd)
    #[arg(long, default_value = ".")]
    root: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise tracing — RUST_LOG=info default
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Size the rayon pool to all logical cores across sockets, and report which
    // runtime SIMD pipeline this host resolved to (AVX-512 / AVX / scalar).
    let n_threads = cesarops_satellite::simd_dispatch::init_thread_pool();
    tracing::info!(
        "compute: {} rayon threads, {} vector pipeline, allocator={}",
        n_threads,
        cesarops_satellite::simd_dispatch::active_pipeline(),
        if cfg!(feature = "jemalloc") { "jemalloc" } else { "system" },
    );

    let args = Args::parse();

    if args.describe {
        println!("{}", describe_catalog()?);
        return Ok(());
    }

    if args.preflight_downloads {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()?;
        let report = preflight_sources(&client, &args.preflight_sensors, args.strict_auth).await;
        println!("{}", serde_json::to_string_pretty(&report)?);
        if !report.ok {
            anyhow::bail!("download preflight failed");
        }
        return Ok(());
    }

    let spec_path = match args.spec {
        Some(p) => p,
        None => {
            anyhow::bail!("--spec is required unless --preflight-downloads is used");
        }
    };

    // Load and optionally patch spec
    let raw = std::fs::read_to_string(&spec_path)
        .map_err(|e| anyhow::anyhow!("Cannot read spec {}: {e}", spec_path.display()))?;
    let mut spec: MissionSpec = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("Invalid mission spec JSON: {e}"))?;

    // Stage override from CLI
    if let Some(stage_names) = args.stages {
        use cesarops_satellite::types::Stage;
        let stages: Vec<Stage> = stage_names
            .iter()
            .filter_map(|s| match s.as_str() {
                "download" => Some(Stage::Download),
                "target_known" => Some(Stage::TargetKnown),
                "poc_aoi" => Some(Stage::PocAoi),
                "sar_local" => Some(Stage::SarLocal),
                "bathy_map" => Some(Stage::BathyMap),
                "bag_local" => Some(Stage::BagLocal),
                "temporal_stack" => Some(Stage::TemporalStack),
                "validate_gt" => Some(Stage::ValidateGt),
                "report" => Some(Stage::Report),
                other => {
                    eprintln!("Unknown stage '{other}', skipping");
                    None
                }
            })
            .collect();
        if !stages.is_empty() {
            spec.stages = Some(stages);
        }
    }

    // Parse knob overrides
    let knob_overrides: Option<HashMap<String, serde_json::Value>> = args
        .knobs
        .as_deref()
        .map(|s| serde_json::from_str(s))
        .transpose()
        .map_err(|e| anyhow::anyhow!("--knobs must be a JSON object: {e}"))?;

    let opts = RunOptions {
        dry_run: args.dry_run,
        knob_overrides,
        pipeline_root: args.root,
    };

    let report = run_mission(spec, opts).await?;

    println!("\n=== MISSION REPORT ===");
    println!("mission_id   : {}", report.mission_id);
    println!("status       : {}", report.status);
    println!("runtime      : {:.1}s", report.runtime_seconds);
    println!("candidates   : {} above threshold", report.candidates.len());
    println!("triple-locks : {} multi-sensor", report.triple_locks.len());

    if !report.candidates.is_empty() {
        println!("\nTop candidates:");
        for (i, c) in report.candidates.iter().take(10).enumerate() {
            println!(
                "  {:2}. {:>8.4}°N {:>9.4}°E  score={:.2}  concept={}  {}",
                i + 1,
                c.lat,
                c.lon,
                c.composite_score,
                c.best_concept.as_deref().unwrap_or("—"),
                c.notes,
            );
        }
    }

    if !report.triple_locks.is_empty() {
        println!("\nTriple-locks (independent sensor agreement):");
        for (i, t) in report.triple_locks.iter().take(10).enumerate() {
            println!(
                "  {:2}. {:>8.4}°N {:>9.4}°E  {}-LOCK [{}]  conf={:.2}  maxZ={:.2}",
                i + 1,
                t.lat,
                t.lon,
                t.lock_level,
                t.families.join("+"),
                t.confidence,
                t.max_zscore,
            );
        }
    }

    Ok(())
}

/// Build the machine-readable tool catalog for the LLM / n8n orchestrator.
///
/// Emits the pipeline identity, the selectable stages, the full tunable knob
/// set with their default values (introspected from `Knobs::default()`), and
/// the required mission-spec inputs. This is the contract an LLM reads to know
/// what it can tune and which stages it can run — see
/// docs/PIPELINE_UNIFICATION_ARCHITECTURE.md.
fn describe_catalog() -> Result<String> {
    use cesarops_satellite::types::Knobs;

    // Knob defaults, introspected by serializing the default struct.
    let knob_defaults = serde_json::to_value(Knobs::default())?;

    let catalog = serde_json::json!({
        "pipeline": "satellite",
        "binary": "sat-run",
        "description": "Optical/SAR satellite wreck + search-and-rescue detection (water + land)",
        "version": env!("CARGO_PKG_VERSION"),
        "stages": [
            { "name": "download",       "desc": "Acquire Sentinel-2 / multi-sensor scenes for the AOI" },
            { "name": "target_known",   "desc": "Score known-wreck coordinates across the optical archive" },
            { "name": "poc_aoi",        "desc": "Full-scene AOI anomaly discovery (Sobel/Secchi/NDTI + NMS)" },
            { "name": "temporal_stack", "desc": "Multi-date NDWI/NDVI persistence z-scoring" },
            { "name": "validate_gt",    "desc": "Validate detections against ground-truth wrecks" },
            { "name": "report",         "desc": "Fuse signals and emit the ranked candidate report" }
        ],
        "knobs": knob_defaults,
        "inputs": {
            "spec": "Path to a MissionSpec JSON (mission_id, bbox [lat_min,lon_min,lat_max,lon_max], days_back, stages, knobs, paths, gt_wreck_names)",
            "knobs": "JSON object overlaying any subset of the knobs above",
            "stages": "Optional subset/ordering of the stage names above",
            "dry_run": "Run wiring without network/downloads"
        },
        "output": {
            "format": "MissionReport JSON",
            "fields": ["mission_id", "status", "runtime_seconds", "stage_results", "candidates[]"]
        }
    });
    Ok(serde_json::to_string_pretty(&catalog)?)
}
