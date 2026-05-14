use crate::garmin_rsd_parser::{ParseResult, Ping};
use crate::outputs::PipelineOptions;
use crate::video_enhanced::{render_enhanced_waterfall, render_enhanced_waterfall_auto, auto_params_from_dataset, Colormap, SonarProcessingParams};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct VideoExportResult {
    pub enabled: bool,
    pub status: String,
    pub output_path: Option<String>,
}

/// ParseResult entry point used by CLI/tests.
#[allow(dead_code)]
pub fn run_video_export(parsed: &ParseResult, output_dir: &Path) -> VideoExportResult {
    match render_enhanced_waterfall_auto(parsed.pings.clone(), output_dir, |_, _| {}) {
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

/// Owned-pings variant called from the background thread (lib.rs).
pub fn run_video_export_pings(
    pings: Vec<Ping>,
    output_dir: &Path,
    on_progress: Box<dyn Fn(u32, u32) + Send>,
    options: &PipelineOptions,
) -> VideoExportResult {
    let colormap = match options.colormap.to_lowercase().as_str() {
        "grayscale" | "gray" | "greyscale" => Colormap::Grayscale,
        "sonar" => Colormap::SonarCustom,
        "plasma" => Colormap::Plasma,
        "ocean" => Colormap::Ocean,
        "inferno" => Colormap::Inferno,
        "iron" => Colormap::Iron,
        "rainbow" => Colormap::Rainbow,
        "viridis" => Colormap::Viridis,
        "magma" => Colormap::Magma,
        "jet" => Colormap::Jet,
        _ => Colormap::Amber,
    };
    let mut params = auto_params_from_dataset(&pings);
    params.remove_water_column = options.remove_water_column;
    params.colormap = colormap;
    params.video_height = options.video_height;
    params.fps = options.video_fps;
    // When curvelet denoising is active on static images, give video a
    // stronger median filter (5x5 kernel) as a fast frame-level substitute.
    // Full 2D curvelet on video frames is too slow per-frame.
    params.median_filter_enabled = true;
    params.median_kernel_size = if options.curvelet_denoise { 5 } else { 3 };

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

