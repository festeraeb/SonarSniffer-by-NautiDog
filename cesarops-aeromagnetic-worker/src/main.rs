//! Aeromagnetic detection worker — adaptive, dipole (WGPU), curvelet.
//! Hardware is auto-selected; tune DetectionLevels per area only.

mod adaptive;
mod continuation;
mod curvelet;
mod datum;
mod dipole_analysis;
mod discriminator;
mod geo;
mod gpu;
mod knobs;
mod known_data;
mod pipeline;
mod scoring;
mod well_loader;

use clap::{Parser, Subcommand};
use geo::GridMeta;
use knobs::DetectionLevels;
use pipeline::{run_detection_pipeline, write_candidates_csv};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(name = "cesarops-aeromagnetic-worker")]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Full pipeline: adaptive → dipole → curvelet
    Detect {
        #[arg(long)]
        grid: PathBuf,
        #[arg(long)]
        meta: PathBuf,
        #[arg(long)]
        levels: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        output_csv: Option<PathBuf>,
        #[arg(long, default_value_t = 25.0)]
        pixel_size: f32,
        /// Optional OGSr petroleum-well CSV. When provided, wells are loaded
        /// (Lake Erie–filtered) and fed to the discriminator cross-reference.
        #[arg(long)]
        wells: Option<PathBuf>,
    },
    /// Legacy dipole-only scan
    Dipole {
        #[arg(long)]
        grid: Option<PathBuf>,
        #[arg(long)]
        meta: Option<PathBuf>,
        #[arg(long, default_value_t = 25.0)]
        pixel_size: f32,
        #[arg(long, default_value = "10")]
        inner: u32,
        #[arg(long, default_value = "25")]
        outer: u32,
        #[arg(long)]
        width: Option<u32>,
        #[arg(long)]
        height: Option<u32>,
        #[arg(long, default_value_t = 0.5)]
        min_score: f32,
        #[arg(long, default_value_t = 100)]
        top_n: usize,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        output_csv: Option<PathBuf>,
        #[arg(long)]
        demo: bool,
    },
    /// Run built-in self-test on demo grid
    TestAll {
        #[arg(long)]
        levels: Option<PathBuf>,
    },
    /// Emit a machine-readable tool catalog (stages, knobs, defaults) as JSON
    /// for the LLM/n8n orchestrator.
    Describe,
}

fn load_f32_grid(
    grid_path: &PathBuf,
    meta: &GridMeta,
    width: Option<u32>,
    height: Option<u32>,
) -> (Vec<f32>, u32, u32) {
    let w = meta.width;
    let h = meta.height;
    let w = width.unwrap_or(w);
    let h = height.unwrap_or(h);
    let bytes = std::fs::read(grid_path).expect("read grid");
    let data: Vec<f32> = bytemuck::cast_slice(&bytes).to_vec();
    assert_eq!(data.len(), (w * h) as usize);
    (data, w, h)
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Detect {
            grid,
            meta,
            levels,
            output,
            output_csv,
            pixel_size,
            wells,
        } => {
            let meta = GridMeta::load(&meta).expect("meta json");
            let levels = DetectionLevels::load(&levels).expect("levels json");
            let (data, w, h) = load_f32_grid(&grid, &meta, None, None);
            let mpx = pixel_size;
            // Embedded known wrecks (NIAGARA + ShipwreckWorld, 47 entries) drive
            // the discriminator cross-reference (erie_wellhead_discriminator.py).
            let wrecks = known_data::all_known_wrecks();
            // OGSr wells are loaded (Lake Erie–filtered) only when --wells is
            // provided; otherwise the wreck cross-reference still activates.
            let wells: Vec<discriminator::Wellhead> = match wells {
                Some(path) => well_loader::load_ogsr_wells(&path, true),
                None => Vec::new(),
            };
            let report =
                run_detection_pipeline(&data, w, h, mpx, &meta, &levels, &wells, &wrecks).await;
            let source = meta
                .source_tif
                .as_ref()
                .and_then(|s| PathBuf::from(s).file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "grid".to_string());
            if let Some(p) = output_csv {
                write_candidates_csv(&p, &source, &report.candidates);
            }
            let text = serde_json::to_string_pretty(&report).unwrap();
            if let Some(p) = output {
                std::fs::write(p, &text).expect("write json");
            }
            println!("{text}");
        }
        Commands::Dipole { demo: true, .. } => run_dipole_legacy_demo(),
        Commands::Dipole {
            grid,
            meta,
            pixel_size,
            inner,
            outer,
            width,
            height,
            min_score,
            top_n,
            output,
            output_csv,
            demo,
        } => {
            if demo {
                run_dipole_legacy_demo();
                return;
            }
            dipole_only(grid, meta, pixel_size, inner, outer, width, height, min_score, top_n, output, output_csv).await;
        }
        Commands::TestAll { levels } => {
            let mut lv = levels
                .map(|p| DetectionLevels::load(&p).expect("levels"))
                .unwrap_or_default();
            // Self-test uses permissive gates; production missions use area JSON only.
            lv.z_thresh = lv.z_thresh.min(0.15);
            lv.edge_z_thresh = lv.edge_z_thresh.min(0.15);
            lv.min_pixels = 1;
            lv.dipole_min_score = lv.dipole_min_score.min(0.05);
            lv.require_dipolar_pull = false;
            let t0 = Instant::now();
            let mut grid = vec![50_000.0f32; 256 * 256];
            // Synthetic dipole blob (~9 px) so adaptive + WGPU both see a target.
            for (dy, dx, sign) in [(-6i32, 0, 1.0f32), (6, 0, -1.0)] {
                for r in -4i32..=4 {
                    for c in -4i32..=4 {
                        if r * r + c * c > 16 {
                            continue;
                        }
                        let y = (128 + dy + r) as usize;
                        let x = (128 + dx + c) as usize;
                        if y < 256 && x < 256 {
                            grid[y * 256 + x] += sign * 180.0;
                        }
                    }
                }
            }
            let meta = GridMeta {
                width: 256,
                height: 256,
                transform: [0.01, 0.0, -82.0, 0.0, -0.01, 42.0],
                source_tif: Some("demo.tif".into()),
            };
            let report = run_detection_pipeline(&grid, 256, 256, 25.0, &meta, &lv, &[], &[]).await;
            println!(
                "test-all: {} candidates in {:.1}ms gpu={}",
                report.candidates.len(),
                t0.elapsed().as_secs_f64() * 1000.0,
                report.gpu_available
            );
            if report.candidates.is_empty() {
                eprintln!("TEST ALL FAILED: no candidates");
                std::process::exit(1);
            }
            println!("TEST ALL PASSED");
        }
        Commands::Describe => {
            println!("{}", describe_catalog());
        }
    }
}

/// Build the machine-readable tool catalog for the LLM / n8n orchestrator.
///
/// Emits the pipeline identity, selectable stages, and the full `DetectionLevels`
/// knob set with defaults (introspected from `DetectionLevels::default()`).
/// See docs/PIPELINE_UNIFICATION_ARCHITECTURE.md.
fn describe_catalog() -> String {
    let knob_defaults = serde_json::to_value(DetectionLevels::default())
        .unwrap_or(serde_json::Value::Null);
    let embedded_wreck_count = known_data::all_known_wrecks().len();
    let catalog = serde_json::json!({
        "pipeline": "aeromagnetic",
        "binary": "cesarops-aeromagnetic-worker",
        "description": "Aeromagnetic anomaly wreck detection (adaptive + dipole + curvelet, basin-aware)",
        "version": env!("CARGO_PKG_VERSION"),
        "stages": [
            { "name": "adaptive",       "desc": "Adaptive local z-score + edge screen + connected-component candidates" },
            { "name": "dipole_gpu",     "desc": "WGPU dipole lobe scan (fast lobe score)" },
            { "name": "dipole_cpu",     "desc": "CPU dipole discriminator: flip distance, gradient contrast, aspect, 0-100 man-made score" },
            { "name": "curvelet",       "desc": "Curvelet structural-energy rescore" },
            { "name": "cross_reference","desc": "Loran-C warp + wellhead/known-wreck cross-reference" },
            { "name": "basin_scoring",  "desc": "Lake Erie basin-aware multiplicative score adjustment" },
            { "name": "disposition",    "desc": "Raised/scrapped/geological false-positive down-rank" },
            { "name": "merge",          "desc": "Candidate merge/NMS within dipole_merge_radius_m" }
        ],
        "knobs": knob_defaults,
        "inputs": {
            "grid": "Path to row-major f32 aeromagnetic nT grid (bin)",
            "meta": "GridMeta JSON (width, height, transform, source_tif)",
            "levels": "DetectionLevels JSON overlaying any subset of the knobs above",
            "pixel_size": "Metres per pixel (default 25)",
            "wells": format!(
                "Optional OGSr petroleum-well CSV (well_loader.rs). When provided, wells are loaded (Lake Erie-filtered) and cross-referenced; {} known wrecks are always embedded (known_data.rs) and active",
                embedded_wreck_count
            )
        },
        "output": {
            "format": "PipelineReport JSON",
            "fields": ["width", "height", "pixel_size_m", "gpu_available", "candidates[]"]
        }
    });
    serde_json::to_string_pretty(&catalog).unwrap_or_default()
}

fn run_dipole_legacy_demo() {
    println!("Use `detect` or `test-all` subcommands for full pipeline");
}

async fn dipole_only(
    grid: Option<PathBuf>,
    meta: Option<PathBuf>,
    pixel_size: f32,
    inner: u32,
    outer: u32,
    width: Option<u32>,
    height: Option<u32>,
    min_score: f32,
    top_n: usize,
    output: Option<PathBuf>,
    output_csv: Option<PathBuf>,
) {
    let _ = (
        grid,
        meta,
        pixel_size,
        inner,
        outer,
        width,
        height,
        min_score,
        top_n,
        output,
        output_csv,
    );
    eprintln!("dipole-only: use `detect` subcommand with --levels");
}
