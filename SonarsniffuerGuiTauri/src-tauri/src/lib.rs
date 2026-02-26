mod corpus_scan;
pub mod garmin_rsd_parser;
mod firmware_lookup;
pub mod outputs;
mod video;
mod video_enhanced;
pub mod lowrance_parser;
pub mod humminbird_parser;
pub mod xtf_parser;
pub mod cerulean_parser;
pub mod jsf_parser;
pub mod format_detector;
mod license;

use corpus_scan::CorpusScanResult;
use garmin_rsd_parser::GarminRSDParser;
use outputs::{build_outputs, PipelineOptions};
#[allow(unused_imports)]
use format_detector::detect_and_parse;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};
use video::VideoExportResult;

#[cfg(feature = "video-gstreamer")]
const VIDEO_FILENAME: &str = "sonar_waterfall.mp4";
#[cfg(not(feature = "video-gstreamer"))]
const VIDEO_FILENAME: &str = "sonar_waterfall.gif";

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
fn run_sonar_pipeline(file_name: &str, options: Option<PipelineOptions>, app: tauri::AppHandle) -> PipelineResponse {
    run_pipeline_internal(file_name, options, Some(app))
}

pub fn run_pipeline_internal(file_name: &str, options: Option<PipelineOptions>, app: Option<tauri::AppHandle>) -> PipelineResponse {
    let options = options.unwrap_or_default();
    let path = Path::new(file_name);

    // ── Detect format and parse ───────────────────────────────────────────
    let detected = format_detector::detect_and_parse(path);
    let probe = detected.probe;
    let mut parse = detected.parse;

    let mut status_notes: Vec<String> = Vec::new();
    let outputs = if parse.error_message.is_none() {
        match build_outputs(path, &parse, &options) {
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
    let depth_stats = {
        let depths: Vec<f32> = parse.pings.iter()
            .map(|p| p.depth_ft)
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
            let min = temps.iter().cloned().fold(f32::MAX, f32::min);
            let max = temps.iter().cloned().fold(f32::MIN, f32::max);
            let avg = temps.iter().sum::<f32>() / temps.len() as f32;
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
        let app_done     = app;
        let remove_water_column = options.remove_water_column;
        let colormap = options.colormap.clone();
        std::thread::spawn(move || {
            let on_progress: Box<dyn Fn(u32, u32) + Send> = Box::new(move |frame, total| {
                if let Some(ref h) = app_progress {
                    let pct = if total > 0 { frame.saturating_mul(100) / total } else { 0 };
                    let _ = h.emit("video-progress", serde_json::json!({
                        "frame": frame, "total": total, "pct": pct
                    }));
                }
            });
            let result = video::run_video_export_pings(pings_for_video, &vid_dir_clone, on_progress, remove_water_column, &colormap);
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
            output_path: Some(vid_dir.join(VIDEO_FILENAME).display().to_string()),
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
    // Append probe warnings so they're always visible in the UI status bar.
    if !probe.header_crc_ok || !probe.body_crc_ok {
        status = format!("{status} — Probe: CRC mismatch (hdr={}, body={})",
            if probe.header_crc_ok { "OK" } else { "FAIL" },
            if probe.body_crc_ok   { "OK" } else { "FAIL" });
    }

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
    .invoke_handler(tauri::generate_handler![
            check_license,
            activate_license,
            pick_input_file,
            pick_any_file,
            pick_folder,
            reveal_path,
            run_sonar_pipeline,
            analyze_firmware,
            scan_corpus_directory
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
