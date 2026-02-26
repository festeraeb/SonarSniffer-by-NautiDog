use crate::garmin_rsd_parser::{ParseResult, Ping};
use crate::video_enhanced::{render_enhanced_waterfall, Colormap, SonarProcessingParams};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct VideoExportResult {
    pub enabled: bool,
    pub status: String,
    pub output_path: Option<String>,
}

/// ParseResult entry point used by CLI/tests.
pub fn run_video_export(parsed: &ParseResult, output_dir: &Path) -> VideoExportResult {
    export_with_params(parsed.pings.clone(), output_dir, Box::new(|_, _| {}), SonarProcessingParams::default())
}

/// Owned-pings variant called from the background thread (lib.rs).
pub fn run_video_export_pings(
    pings: Vec<Ping>,
    output_dir: &Path,
    on_progress: Box<dyn Fn(u32, u32) + Send>,
    remove_water_column: bool,
    colormap_str: &str,
) -> VideoExportResult {
    let colormap = match colormap_str.to_lowercase().as_str() {
        "grayscale" | "gray" | "greyscale" => Colormap::Grayscale,
        _ => Colormap::Amber,
    };
    let params = SonarProcessingParams {
        remove_water_column,
        colormap,
        ..SonarProcessingParams::default()
    };
    export_with_params(pings, output_dir, on_progress, params)
}

fn export_with_params(
    pings: Vec<Ping>,
    output_dir: &Path,
    on_progress: Box<dyn Fn(u32, u32) + Send>,
    params: SonarProcessingParams,
) -> VideoExportResult {
    if pings.is_empty() {
        return VideoExportResult {
            enabled: true,
            status: "No pings available for video export".to_string(),
            output_path: None,
        };
    }

    let progress = move |frame: u32, total: u32| {
        on_progress(frame, total);
    };

    match render_enhanced_waterfall(pings, output_dir, params, progress) {
        Ok(result) => VideoExportResult {
            enabled: true,
            status: result.status,
            output_path: result.output_path,
        },
        Err(err) => VideoExportResult {
            enabled: true,
            status: format!("Video export failed: {err:#}"),
            output_path: None,
        },
    }
}

