mod corpus_scan;
pub mod garmin_rsd_parser;
pub mod firmware_lookup;
pub mod healing_api;
pub mod channel_alignment;
pub mod channel_discovery;
pub mod probing;
pub mod egn;
pub mod outputs;
pub mod mosaic;
pub mod curvelet_diag;
mod video;
mod video_enhanced;
pub mod lowrance_parser;
pub mod humminbird_parser;
pub mod xtf_parser;
pub mod cerulean_parser;
pub mod jsf_parser;
pub mod format_detector;
mod license;
mod deps;
mod target_detection;
mod static_server;

use corpus_scan::CorpusScanResult;
use outputs::{build_outputs, PipelineOptions, estimate_curvelet_threshold};
#[allow(unused_imports)]
use format_detector::detect_and_parse;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use video::VideoExportResult;

/// Expected video filename — MP4 when GStreamer is compiled in, GIF otherwise.
/// The actual file produced may differ (GStreamer can fall back to GIF at runtime);
/// the authoritative path comes from the `video-complete` event.
fn video_filename() -> &'static str {
    if cfg!(feature = "video-gstreamer") {
        "sonar_waterfall_enhanced.mp4"
    } else {
        "sonar_waterfall.gif"
    }
}

/// Set up environment so bundled GStreamer DLLs are found at runtime.
/// Called once during app setup — safe to call even when GStreamer isn't bundled.
///
/// Search order (Windows):
///   1. `<exe_dir>/gstreamer/` — bundled copy shipped alongside the executable
///   2. `GSTREAMER_1_0_ROOT_MSVC_X86_64` env var (set by official SDK installer)
///   3. `C:\gstreamer\1.0\msvc_x86_64` — official installer default path
///   4. `C:\Program Files\gstreamer\1.0\msvc_x86_64` — alternate installer path
#[cfg(target_os = "windows")]
fn setup_bundled_gstreamer() {
    let exe_dir = match std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        Some(d) => d,
        None => return,
    };

    // Candidate search order
    let mut candidates = vec![exe_dir.join("gstreamer")];
    if let Ok(env_root) = std::env::var("GSTREAMER_1_0_ROOT_MSVC_X86_64") {
        candidates.push(std::path::PathBuf::from(env_root));
    }
    if let Ok(env_root) = std::env::var("GSTREAMER_1_0_ROOT_MINGW_X86_64") {
        candidates.push(std::path::PathBuf::from(env_root));
    }
    
    // Hardcoded absolute paths (Common defaults)
    let common_bases = [
        r"C:\gstreamer\1.0",
        r"D:\gstreamer\1.0",
        r"C:\Program Files\gstreamer\1.0",
        r"C:\Program Files (x86)\gstreamer\1.0",
    ];
    let common_archs = [
        "msvc_x86_64",
        "mingw_x86_64",
        "x86_64",
        "msvc_x86",
        "mingw_x86",
        "x86"
    ];
    for base in &common_bases {
        for arch in &common_archs {
            candidates.push(std::path::PathBuf::from(base).join(arch));
        }
    }

    for gst_dir in candidates {
        let gst_bin = gst_dir.join("bin");
        if !gst_bin.exists() {
            continue;
        }
        // Prepend bin/ to PATH so Windows finds the core DLLs
        if let Ok(path) = std::env::var("PATH") {
            std::env::set_var("PATH", format!("{};{}", gst_bin.display(), path));
        }
        // Tell GStreamer exactly where plugins live
        let plugin_dir = gst_dir.join("lib").join("gstreamer-1.0");
        if plugin_dir.exists() {
            let pd = plugin_dir.display().to_string();
            std::env::set_var("GST_PLUGIN_PATH", &pd);
            std::env::set_var("GST_PLUGIN_SYSTEM_PATH", &pd);
        }
        eprintln!("[gstreamer] GStreamer found at {}", gst_dir.display());
        return;
    }
    eprintln!("[gstreamer] GStreamer not found — video will fall back to GIF");
}

/// On macOS, bundled GStreamer dylibs live in the .app's Resources/ directory.
/// Core dylibs are found via @rpath (set in build.rs).
/// Plugins need GST_PLUGIN_PATH set at runtime.
#[cfg(target_os = "macos")]
fn setup_bundled_gstreamer() {
    let exe_dir = match std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        Some(d) => d,
        None => return,
    };
    // In a .app bundle: exe is at Contents/MacOS/SonarSniffer
    // Resources are at Contents/Resources/
    let resources_dir = match exe_dir.parent() {
        Some(contents) => contents.join("Resources"),
        None => return,
    };
    let gst_dir = resources_dir.join("gstreamer");
    let dylibs_dir = gst_dir.join("dylibs");
    if !dylibs_dir.exists() {
        return;
    }
    // Tell GStreamer where plugins live
    let plugin_dir = gst_dir.join("lib").join("gstreamer-1.0");
    if plugin_dir.exists() {
        let pd = plugin_dir.display().to_string();
        std::env::set_var("GST_PLUGIN_PATH", &pd);
        std::env::set_var("GST_PLUGIN_SYSTEM_PATH", &pd);
    }
    eprintln!("[gstreamer] Bundled GStreamer detected at {}", gst_dir.display());
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn setup_bundled_gstreamer() { /* no-op on Linux — system GStreamer used */ }

#[derive(Debug, Clone, Serialize)]
pub struct PipelineResponse {
    pub input_file: String,
    pub parse: garmin_rsd_parser::ParseResult,
    pub outputs: Option<outputs::OutputSummary>,
    pub video: Option<VideoExportResult>,
    /// [min_ft, max_ft, avg_ft] computed before pings are cleared.
    pub depth_stats: [f32; 3],
    /// [min_c, max_c, avg_c] if water temp is present; otherwise [0,0,0].
    pub temp_stats: [f32; 3],
    /// `true` when a video export thread was spawned; frontend should listen for
    /// `video-progress` and `video-complete` Tauri events.
    pub video_rendering: bool,
    pub status: String,
    /// Quick pre-parse varstruct probe (magic, CRC, first channel, field layout).
    pub probe: garmin_rsd_parser::FileProbe,
    /// Target detection results (if detection mode was enabled).
    pub detections: Option<target_detection::DetectionSummary>,
    /// Device fingerprint for alignment cache lookup.
    pub device_fingerprint: String,
    /// Resolved channel alignment (saved + auto-detected).
    pub channel_alignment: Vec<channel_alignment::ChannelAlignment>,
    /// Effective curvelet threshold used for this run.  0.0 if denoising was off.
    /// When auto-mode is on this is the MAD-estimated value so the user can see
    /// what the pipeline computed and optionally pin it as a manual override.
    pub curvelet_threshold_used: f32,
    /// Data-driven channel discovery and signal profiling results.
    /// Contains archetype classifications, sidescan pairs, frequency tiers,
    /// and composite scanline groupings.
    pub channel_discovery: Option<channel_discovery::DiscoveryResult>,
}

#[derive(Debug, Clone, Serialize)]
struct FirmwareLookupResponse {
    analysis: firmware_lookup::FirmwareLookupResult,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
struct CorpusScanResponse {
    scan: CorpusScanResult,
    status: String,
}

#[tauri::command]
fn check_license(app: tauri::AppHandle) -> license::LicenseStatus {
    let data_dir = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
    license::check_license(data_dir)
}

#[tauri::command]
fn check_dependencies() -> deps::DependencyStatus {
    deps::check_gstreamer()
}

#[tauri::command]
fn install_gstreamer() -> Result<String, String> {
    deps::install_gstreamer_runtime()
}

#[tauri::command]
fn activate_license(key: String, app: tauri::AppHandle) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
    license::activate_license(key, data_dir)
}

#[tauri::command]
fn pick_input_file() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Sonar logs", &[
            "rsd", "RSD",
            "sl2", "SL2",
            "sl3", "SL3",
            "dat", "DAT",
            "son", "SON",
            "xtf", "XTF",
            "jsf", "JSF",
            "svlog", "SVLOG",
            "bin",
        ])
        .pick_file()
        .map(|path| path.display().to_string())
}

#[tauri::command]
fn pick_any_file() -> Option<String> {
    rfd::FileDialog::new()
        .pick_file()
        .map(|path| path.display().to_string())
}

#[tauri::command]
fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .map(|path| path.display().to_string())
}

#[tauri::command]
async fn run_sonar_pipeline(file_name: String, options: Option<PipelineOptions>, app: tauri::AppHandle) -> Result<PipelineResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_pipeline_internal(&file_name, options, Some(app))
    }).await.map_err(|e| e.to_string())
}

#[derive(Clone, serde::Serialize)]
struct PipelineProgress {
    step: String,
    pct: u8,
}

pub fn run_pipeline_internal(file_name: &str, options: Option<PipelineOptions>, app: Option<tauri::AppHandle>) -> PipelineResponse {
    let mut options = options.unwrap_or_default();
    let path = Path::new(file_name);

    if let Some(a) = &app {
        let _ = a.emit("pipeline-progress", PipelineProgress { step: "Parsing file...".into(), pct: 5 });
    }

    // ── Detect format and parse ───────────────────────────────────────────
    let detected = format_detector::detect_and_parse(path);
    let probe = detected.probe;
    let mut parse = detected.parse;

    // ── Heuristic probe: hardware identification + alignment detection ────
    // Reads at most 10 MB of the raw file. The resulting ProbeReport drives:
    //   • record_alignment_bytes (4 for GT56/UHD2, 2 for GT51/GT54)
    //   • hardware_gain_raw per channel (Gen1Classic TVG byte)
    // Any sonar_size ↔ sample mismatch is reported by the parser itself.
    if detected.format == format_detector::SonarFormat::GarminRSD {
        if let Ok(raw) = std::fs::read(path) {
            let hprobe = probing::probe_file_bytes(&raw);
            eprintln!(
                "[heuristic_probe] hardware={} gen={:?} alignment={}B confidence={:.2} records={}",
                hprobe.hardware, hprobe.generation, hprobe.record_alignment_bytes,
                hprobe.confidence, hprobe.records_decoded,
            );
            for cp in &hprobe.channels {
                let gain = cp.hardware_gain_raw.map(|g| format!(" gain_raw={}", g)).unwrap_or_default();
                eprintln!(
                    "[heuristic_probe]   ch{} bit={:?} nadir={:?} role={:?} flip={:?}{}",
                    cp.channel_id, cp.bit_depth, cp.nadir_edge, cp.suggested_role,
                    cp.flip_status, gain,
                );
            }
        }
    }

    let mut status_notes: Vec<String> = Vec::new();

    if let Some(a) = &app {
        let _ = a.emit("pipeline-progress", PipelineProgress { step: "Detecting targets...".into(), pct: 40 });
    }

    // ── Target detection (runs before outputs so results can be included) ──
    let detections = if parse.error_message.is_none() {
        let clutter = options.detection_clutter.clamp(0.0, 1.0);
        // Map clutter slider (0..1) -> sensitivity multiplier (1.0..2.5).
        // Higher clutter means stronger suppression (fewer false positives).
        let effective_sensitivity = options.detection_sensitivity * (1.0 + 1.5 * clutter);
        if clutter > 0.0 {
            status_notes.push(format!(
                "Detection clutter {:.0}% applied (sensitivity {:.2} -> {:.2})",
                clutter * 100.0,
                options.detection_sensitivity,
                effective_sensitivity
            ));
        }
        let det_settings = target_detection::DetectionSettings {
            mode: target_detection::DetectionMode::from_str(&options.detection_mode),
            min_size: options.detection_min_size,
            max_size: options.detection_max_size,
            sensitivity: effective_sensitivity,
        };
        if det_settings.mode != target_detection::DetectionMode::Off {
            Some(target_detection::detect_targets(&parse, &det_settings))
        } else {
            None
        }
    } else {
        None
    };

    let outputs = if parse.error_message.is_none() {
        if let Some(a) = &app {
            let _ = a.emit("pipeline-progress", PipelineProgress { step: "Applying Curvelet filter...".into(), pct: 50 });
        }
        // ── Auto-compute curvelet threshold before building any outputs ─────────
        if options.curvelet_denoise && options.curvelet_auto {
            let t = estimate_curvelet_threshold(&parse);
            options.curvelet_threshold = t;
        }

        if let Some(a) = &app {
            let _ = a.emit("pipeline-progress", PipelineProgress { step: "Generating Geographic Map Data...".into(), pct: 60 });
        }
        
        // Pass app down to build_outputs if we want more granular progress
        match build_outputs(path, &parse, &options, detections.as_ref(), app.clone()) {
            Ok(o) => Some(o),
            Err(e) => {
                status_notes.push(format!("Outputs failed: {e:#}"));
                None
            }
        }
    } else {
        None
    };

    // Compute depth stats BEFORE clearing pings so the frontend can still show them.
    // Returns [min, max, avg] in the user's chosen unit system.
    let use_metric = options.unit_system == "metric";
    let depth_stats = {
        let depths: Vec<f32> = parse.pings.iter()
            .map(|p| if use_metric { p.depth_m } else { p.depth_ft })
            .filter(|&d| d > 0.0)
            .collect();
        if depths.is_empty() {
            [0.0_f32; 3]
        } else {
            let min = depths.iter().cloned().fold(f32::MAX, f32::min);
            let max = depths.iter().cloned().fold(f32::MIN, f32::max);
            let avg = depths.iter().sum::<f32>() / depths.len() as f32;
            [min, max, avg]
        }
    };

    let temp_stats = {
        let temps: Vec<f32> = parse.pings.iter()
            .filter_map(|p| p.temp_c)
            .filter(|t| *t > 0.0)
            .collect();
        if temps.is_empty() {
            [0.0_f32; 3]
        } else {
            let converted: Vec<f32> = if use_metric {
                temps
            } else {
                temps.iter().map(|&c| c * 9.0 / 5.0 + 32.0).collect()
            };
            let min = converted.iter().cloned().fold(f32::MAX, f32::min);
            let max = converted.iter().cloned().fold(f32::MIN, f32::max);
            let avg = converted.iter().sum::<f32>() / converted.len() as f32;
            [min, max, avg]
        }
    };

    // Video handling:
    // - In the Tauri app (app.is_some()): spawn a background thread and emit events, and clear
    //   pings from the IPC payload to avoid 160+ MB serialization.
    // - In CLI / tests (app.is_none()): keep pings in-memory and skip video generation so we
    //   can inspect decoded fields (depth/temp debugging etc.).
    let video_rendering;
    let video = if options.video && app.is_some() {
        let vid_dir: PathBuf = outputs
            .as_ref()
            .map(|o| PathBuf::from(&o.output_dir))
            .unwrap_or_else(|| {
                path.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."))
            });
        let pings_for_video = std::mem::take(&mut parse.pings);
        let vid_dir_clone = vid_dir.clone();
        let app_progress = app.clone();
        let app_done     = app.clone();
        let vid_options = options.clone();
        std::thread::spawn(move || {
            let on_progress: Box<dyn Fn(u32, u32) + Send> = Box::new(move |frame, total| {
                if let Some(ref h) = app_progress {
                    let pct = if total > 0 { frame.saturating_mul(100) / total } else { 0 };
                    let _ = h.emit("video-progress", serde_json::json!({
                        "frame": frame, "total": total, "pct": pct
                    }));
                }
            });
            let result = video::run_video_export_pings(pings_for_video, &vid_dir_clone, on_progress, &vid_options);
            if let Some(ref h) = app_done {
                let _ = h.emit("video-complete", serde_json::json!({
                    "status": result.status,
                    "output_path": result.output_path,
                    "ok": result.output_path.is_some()
                }));
            }
        });
        video_rendering = true;
        Some(VideoExportResult {
            enabled: true,
            status: "Video rendering in background — watch the progress bar".to_string(),
            output_path: Some(vid_dir.join(video_filename()).display().to_string()),
        })
    } else {
        // CLI / tests: keep pings for inspection
        video_rendering = false;
        None
    };

    let mut status = if let Some(err) = &parse.error_message {
        format!("Parsing failed: {err}")
    } else {
        "Pipeline complete".to_string()
    };
    if !status_notes.is_empty() {
        status = format!("{status} — {}", status_notes.join("; "));
    }
    // Garmin RSD CRC fields use an unknown polynomial (header) and a constant
    // sentinel (body) — mismatches are universal and expected.  Don't alarm users.

    // Resolve channel alignment: saved settings win, auto-detect fills gaps.
    let (device_fingerprint, channel_alignment) = channel_alignment::resolve(&parse, None);

    // Run the data-driven channel discovery and signal profiling pipeline.
    // This produces archetype classifications, sidescan pairs, frequency tiers,
    // and composite scanline groupings without relying on hardcoded channel maps.
    let channel_discovery = if parse.error_message.is_none() && !parse.pings.is_empty() {
        if let Some(a) = &app {
            let _ = a.emit("pipeline-progress", PipelineProgress { step: "Discovering channels...".into(), pct: 15 });
        }
        Some(channel_discovery::discover_and_profile(&parse))
    } else {
        None
    };

    PipelineResponse {
        input_file: file_name.to_string(),
        parse,
        outputs,
        video,
        depth_stats,
        temp_stats,
        video_rendering,
        status,
        probe,
        detections,
        device_fingerprint,
        channel_alignment,
        curvelet_threshold_used: if options.curvelet_denoise { options.curvelet_threshold } else { 0.0 },
        channel_discovery,
    }
}

#[tauri::command]
fn analyze_firmware(file_name: &str) -> FirmwareLookupResponse {
    let analysis = firmware_lookup::analyze_firmware_file(Path::new(file_name));
    let status = if let Some(err) = &analysis.error_message {
        format!("Firmware analysis failed: {err}")
    } else {
        format!(
            "Firmware analysis complete ({} float hits, {} XOR blocks)",
            analysis.float_hits.len(),
            analysis.xor_blocks.len()
        )
    };

    FirmwareLookupResponse { analysis, status }
}

/// Open a file or folder in the native file manager (Windows Explorer).
/// If `path` points to a file the containing folder is opened with the file
/// selected; if it points to a directory the directory is opened directly.
#[tauri::command]
fn reveal_path(path: String, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn scan_corpus_directory(root_dir: &str) -> CorpusScanResponse {
    let scan = corpus_scan::scan_corpus_dir(Path::new(root_dir));
    let status = if let Some(err) = &scan.error_message {
        format!("Corpus scan failed: {err}")
    } else if scan.truncated {
        format!(
            "Corpus scan complete ({} matched files, showing first {} hits)",
            scan.matched_files,
            scan.hits.len()
        )
    } else {
        format!("Corpus scan complete ({} matched files)", scan.matched_files)
    };

    CorpusScanResponse { scan, status }
}

/// Result for a single file in a batch run.
#[derive(Debug, Clone, Serialize)]
pub struct BatchJobResult {
    pub file: String,
    pub ok: bool,
    pub error: Option<String>,
    pub output_dir: Option<String>,
}

/// Run the sonar pipeline sequentially over a list of files.
///
/// Emits `batch-progress` events: `{ index: usize, total: usize, file: String }`
/// after each file finishes so the frontend can drive a progress indicator.
#[tauri::command]
async fn run_batch_pipeline(
    app: tauri::AppHandle,
    files: Vec<String>,
    options: PipelineOptions,
) -> Result<Vec<BatchJobResult>, String> {
    use tauri::Emitter;
    let total = files.len();
    let mut results = Vec::with_capacity(total);

    for (i, file) in files.iter().enumerate() {
        // Notify frontend we are starting this file
        let _ = app.emit("batch-progress", serde_json::json!({
            "index": i,
            "total": total,
            "file": file,
            "state": "running",
        }));

        let file_clone = file.clone();
        let opts_clone = options.clone();
        let app_clone  = app.clone();

        let outcome = tauri::async_runtime::spawn_blocking(move || {
            run_pipeline_internal(&file_clone, Some(opts_clone), Some(app_clone))
        }).await;

        match outcome {
            Ok(resp) => {
                let out_dir = resp.outputs.as_ref().map(|o| o.output_dir.clone());
                results.push(BatchJobResult { file: file.clone(), ok: true, error: None, output_dir: out_dir });
            }
            Err(e) => {
                results.push(BatchJobResult { file: file.clone(), ok: false, error: Some(e.to_string()), output_dir: None });
            }
        }

        // Notify done
        let _ = app.emit("batch-progress", serde_json::json!({
            "index": i + 1,
            "total": total,
            "file": file,
            "state": "done",
        }));
    }

    Ok(results)
}

// ── RSD fingerprint command ───────────────────────────────────────────────────

#[tauri::command]
fn rsd_fingerprint(file_name: &str, max_records: Option<usize>) -> firmware_lookup::RsdFingerprint {
    let n = max_records.unwrap_or(20);
    firmware_lookup::fingerprint_rsd(Path::new(file_name), n)
}

// ── healing cache commands ────────────────────────────────────────────────────

#[tauri::command]
fn get_healing_cache(app: tauri::AppHandle) -> healing_api::HealingCache {
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    healing_api::load_cache(Some(&data_dir))
}

#[tauri::command]
fn merge_community_healings(
    healings: Vec<healing_api::HealingDiscovery>,
    app: tauri::AppHandle,
) -> Result<usize, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    healing_api::merge_community(healings, Some(&data_dir))
}

// ── curvelet preview command ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct CurveletPreviewResponse {
    before_b64: String,
    after_b64: String,
    suggested: f32,
}

/// Return base64-encoded PNG data URLs for a before/after curvelet preview.
/// Renders a 512px-wide probe of the primary sidescan channel so the result
/// arrives fast enough for real-time slider feedback.
#[tauri::command]
fn preview_curvelet(file_name: &str, threshold: f32) -> Result<CurveletPreviewResponse, String> {
    eprintln!("[curvelet] preview_curvelet file={file_name} threshold={threshold:.4}");
    let path = Path::new(file_name);
    let detected = format_detector::detect_and_parse(path);
    let parse = detected.parse;
    if parse.pings.is_empty() {
        eprintln!("[curvelet] preview_curvelet: no pings parsed from {file_name}");
        return Err("No pings in file".to_string());
    }
    eprintln!("[curvelet] preview_curvelet: {} pings, calling curvelet_preview_png", parse.pings.len());
    let (before_png, after_png, suggested) = outputs::curvelet_preview_png(&parse, threshold);
    eprintln!("[curvelet] preview_curvelet: done — suggested={suggested:.4} before={}B after={}B",
        before_png.len(), after_png.len());
    use base64::Engine as _;
    let enc = base64::engine::general_purpose::STANDARD;
    Ok(CurveletPreviewResponse {
        before_b64: format!("data:image/png;base64,{}", enc.encode(&before_png)),
        after_b64: format!("data:image/png;base64,{}", enc.encode(&after_png)),
        suggested,
    })
}

/// Drain and return accumulated curvelet diagnostics (timing, errors, thresholds).
/// Call from browser DevTools: `await __TAURI__.core.invoke('get_curvelet_diagnostics')`
#[tauri::command]
fn get_curvelet_diagnostics() -> Vec<curvelet_diag::CurveletDiagEntry> {
    curvelet_diag::drain()
}

#[tauri::command]
fn save_channel_alignment(
    fingerprint: String,
    file_name: String,
    alignments: Vec<channel_alignment::ChannelAlignment>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let desc = format!("User-saved alignment for {}", file_name);
    channel_alignment::save(&fingerprint, &desc, &file_name, alignments, Some(&data_dir))
}

// ── channel discovery command ─────────────────────────────────────────────────

/// Run the exhaustive channel discovery and signal profiling pipeline on a file.
/// Returns the full DiscoveryResult including channel profiles, sidescan pairs,
/// center channels, composite scanlines, and a diagnostic log.
#[tauri::command]
fn discover_channels(file_name: &str) -> Result<channel_discovery::DiscoveryResult, String> {
    let path = Path::new(file_name);
    let detected = format_detector::detect_and_parse(path);
    let parse = detected.parse;
    if parse.pings.is_empty() {
        return Err(parse.error_message.unwrap_or_else(|| "No pings in file".to_string()));
    }
    Ok(channel_discovery::discover_and_profile(&parse))
}

/// Run the full mosaic engine: discovery → georectification → TVG → tiles → KML.
#[tauri::command]
fn render_mosaic(
    file_name: &str,
    output_dir: &str,
    nadir_mode: Option<String>,
    resolution_m: Option<f64>,
    colormap: Option<String>,
) -> Result<mosaic::engine::MosaicOutput, String> {
    let path = Path::new(file_name);
    let detected = format_detector::detect_and_parse(path);
    let parse = detected.parse;
    if parse.pings.is_empty() {
        return Err(parse.error_message.unwrap_or_else(|| "No pings in file".to_string()));
    }
    let discovery = channel_discovery::discover_and_profile(&parse);

    let nadir = match nadir_mode.as_deref() {
        Some("fill") => mosaic::engine::NadirMode::Fill,
        Some("raw") => mosaic::engine::NadirMode::Raw,
        _ => mosaic::engine::NadirMode::Stitch,
    };

    let config = mosaic::engine::MosaicConfig {
        resolution_m: resolution_m.unwrap_or(0.25),
        colormap: colormap.unwrap_or_else(|| "amber".to_string()),
        nadir_mode: nadir,
        output_dir: PathBuf::from(output_dir),
        ..Default::default()
    };

    Ok(mosaic::engine::render_mosaic(&parse, &discovery, &config))
}

// ── viewer server command ─────────────────────────────────────────────────────────────

struct ViewerServerState(Mutex<Option<static_server::StaticServer>>);

/// Start a local HTTP server for the viewer directory.
/// Returns the base URL (e.g. `http://127.0.0.1:54321`).
/// Automatically stops any previously running viewer server.
#[tauri::command]
fn serve_viewer(dir: String, state: tauri::State<'_, ViewerServerState>) -> Result<String, String> {
    let path = Path::new(&dir);
    if !path.is_dir() {
        return Err(format!("Not a directory: {dir}"));
    }
    let mut guard = state.0.lock().map_err(|e| format!("lock: {e}"))?;
    // Stop any existing server
    if let Some(old) = guard.take() {
        old.shutdown();
    }
    let server = static_server::StaticServer::start(path)?;
    let url = server.url.clone();
    *guard = Some(server);
    Ok(url)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Detect and configure bundled GStreamer DLLs before anything else
    setup_bundled_gstreamer();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(ViewerServerState(Mutex::new(None)))
    .invoke_handler(tauri::generate_handler![
            check_license,
            activate_license,
            pick_input_file,
            pick_any_file,
            pick_folder,
            reveal_path,
            run_sonar_pipeline,
            analyze_firmware,
            rsd_fingerprint,
            scan_corpus_directory,
            run_batch_pipeline,
            get_healing_cache,
            merge_community_healings,
            check_dependencies,
            install_gstreamer,
            save_channel_alignment,
            preview_curvelet,
            get_curvelet_diagnostics,
            discover_channels,
            render_mosaic,
            serve_viewer
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

