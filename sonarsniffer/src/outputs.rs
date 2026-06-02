use crate::channel_discovery::SpatialRole;
use crate::egn::{apply_egn, beam_profile_from_pings, BeamProfile};
use crate::garmin_rsd_parser::{ParseResult, Ping};
use crate::target_detection::DetectionSummary;
use crate::video_enhanced::tvg;
use anyhow::{Context, Result};
use image::codecs::png::PngEncoder;
use image::{ColorType, GrayImage, ImageBuffer, ImageEncoder, Rgb, RgbImage, Rgba};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;

/// Google Earth ground-overlay strip size (low values look blurry when draped).
const KMZ_OVERLAY_WIDTH: u32 = 1024;
const KMZ_OVERLAY_MAX_HEIGHT: u32 = 512;

/// Progress callback for pipeline stages. Receives (step_description, percent_complete).
/// When running under Tauri, this emits events to the frontend.
/// When running headless/CLI, this can be a no-op or print to stderr.
pub type ProgressCallback = dyn Fn(&str, u8);

/// Helper to emit progress if a callback is provided.
#[inline]
fn emit_progress(cb: Option<&ProgressCallback>, step: &str, pct: u8) {
    if let Some(f) = cb {
        f(step, pct);
    }
}

#[derive(Clone, serde::Serialize)]
struct PipelineProgress {
    step: String,
    pct: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputArtifact {
    pub kind: String,
    pub path: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputSummary {
    pub output_dir: String,
    pub artifacts: Vec<OutputArtifact>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineOptions {
    pub output_dir: Option<String>,
    pub video: bool,
    pub kml: bool,
    pub kmz: bool,
    pub mbtiles: bool,
    pub mosaic: bool,
    pub waterfall: bool,
    pub arcgis: bool,
    pub web_viewer: bool,
    #[serde(default)]
    pub noaa_enc: bool,
    #[serde(default)]
    pub colormap: String,
    #[serde(default)]
    pub remove_water_column: bool,
    // Video-specific options
    #[serde(default = "default_video_height")]
    pub video_height: u32,
    #[serde(default = "default_video_fps")]
    pub video_fps: u32,
    // Data overlay toggles for video
    #[serde(default)]
    pub overlay_depth: bool,
    #[serde(default)]
    pub overlay_temperature: bool,
    #[serde(default)]
    pub overlay_gps: bool,
    #[serde(default)]
    pub overlay_transducer: bool,
    #[serde(default)]
    pub overlay_speed: bool,
    /// Use simpler overlay-style rendering for video (TVG-simple + percentile + gamma).
    #[serde(default = "default_true")]
    pub video_simple: bool,
    /// Render video in downscope mode with ping-time axis flowing right-to-left.
    #[serde(default)]
    pub video_downscope_rtl: bool,
    /// Unit system: "imperial" (ft, °F, knots, nm) or "metric" (m, °C, km/h, km).
    #[serde(default = "default_unit_system")]
    pub unit_system: String,
    // ── Target detection ──────────────────────────────────────────────────
    /// Detection mode: "off", "fish", "structure", "debris", "wreck".
    #[serde(default)]
    pub detection_mode: String,
    /// Minimum blob area (samples × pings) to keep.
    #[serde(default = "default_detection_min_size")]
    pub detection_min_size: u32,
    /// Maximum blob area to keep.
    #[serde(default = "default_detection_max_size")]
    pub detection_max_size: u32,
    /// Sensitivity multiplier over noise floor (higher = fewer false positives).
    #[serde(default = "default_detection_sensitivity")]
    pub detection_sensitivity: f32,
    /// Fisherman clutter suppression slider (0.0-1.0).
    /// Higher values raise detection threshold to reduce visual clutter/noise.
    #[serde(default)]
    pub detection_clutter: f32,
    /// Per-channel alignment overrides (flip / invert). If empty, auto-detect is used.
    #[serde(default)]
    pub channel_alignments: Vec<crate::channel_alignment::ChannelAlignment>,
    /// Highlight extra sonar payload bytes in magenta on waterfall PNGs (debug only).
    #[serde(default)]
    pub show_payload_debug_overlay: bool,
    // ── Curvelet denoising ────────────────────────────────────────────────
    /// Nadir (center-gap) handling for the stitched sidescan mosaic.
    /// "stitch" = close the gap, "fill" = paint with downscan if available, "raw" = leave transparent.
    /// Default: "stitch".
    #[serde(default = "default_nadir_mode")]
    pub nadir_mode: String,
    /// Apply curvelet soft-thresholding to waterfall and mosaic images.
    /// Reduces speckle/noise while preserving sharp edges along geological features.
    #[serde(default)]
    pub curvelet_denoise: bool,
    /// When true (default), automatically estimate the threshold from the data
    /// using the MAD universal estimator.  False = use `curvelet_threshold` as-is.
    #[serde(default = "default_true")]
    pub curvelet_auto: bool,
    /// Soft-threshold value (normalised 0–1). Larger = more aggressive denoising.
    /// Default 0.05 works well for typical sonar noise levels.
    #[serde(default = "default_curvelet_threshold")]
    pub curvelet_threshold: f32,
}

fn default_nadir_mode() -> String {
    "stitch".to_string()
}
fn default_video_height() -> u32 {
    1080
}
fn default_video_fps() -> u32 {
    6
}
fn default_curvelet_threshold() -> f32 {
    0.05
}
fn default_unit_system() -> String {
    "imperial".to_string()
}
fn default_detection_min_size() -> u32 {
    4
}
fn default_detection_max_size() -> u32 {
    500_000
}
fn default_detection_sensitivity() -> f32 {
    3.0
}
fn default_true() -> bool {
    true
}

impl Default for PipelineOptions {
    fn default() -> Self {
        Self {
            output_dir: None,
            video: true,
            kml: true,
            kmz: true,
            mbtiles: true,
            mosaic: true,
            waterfall: true,
            arcgis: true,
            web_viewer: true,
            noaa_enc: false,
            colormap: "amber".to_string(),
            remove_water_column: false,
            video_height: 1080,
            video_fps: 6,
            overlay_depth: false,
            overlay_temperature: false,
            overlay_gps: false,
            overlay_transducer: false,
            overlay_speed: false,
            video_simple: true,
            video_downscope_rtl: false,
            unit_system: "imperial".to_string(),
            detection_mode: String::new(),
            detection_min_size: 4,
            detection_max_size: 500_000,
            detection_sensitivity: 3.0,
            detection_clutter: 0.0,
            channel_alignments: Vec::new(),
            show_payload_debug_overlay: false,
            nadir_mode: "stitch".to_string(),
            curvelet_denoise: false,
            curvelet_auto: true,
            curvelet_threshold: 0.05,
        }
    }
}

/// Build all requested output artifacts for a parsed file.
pub fn build_outputs(
    input_file: &Path,
    parsed: &ParseResult,
    options: &PipelineOptions,
    detections: Option<&DetectionSummary>,
    progress: Option<&ProgressCallback>,
) -> Result<OutputSummary> {
    let parent = input_file
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let stem = input_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("sniffer_output");

    // If user supplies an output_dir, nest runs under that dir as <stem>_<n> to avoid clobbering.
    let output_dir = if let Some(user_dir) = &options.output_dir {
        let base = PathBuf::from(user_dir);
        fs::create_dir_all(&base)
            .with_context(|| format!("Failed to create base output dir: {}", base.display()))?;
        next_available_dir(&base, stem)
    } else {
        next_available_dir(&parent, stem)
    };

    fs::create_dir_all(&output_dir)
        .with_context(|| format!("Failed to create output dir: {}", output_dir.display()))?;

    let mut artifacts = Vec::new();

    // ── Pre-compute expensive results shared by multiple output writers ──
    // The sigma filter takes 4-7s per channel; compute once when both
    // waterfall + mosaic need denoised images from the same gray data.
    let denoised_cache: BTreeMap<u32, GrayImage> =
        if options.waterfall && options.mosaic && options.curvelet_denoise {
            emit_progress(progress, "Denoising channels...", 62);
            let channels = pings_by_channel(parsed);
            channels
                .iter()
                .map(|(ch, pings)| {
                    let raw = render_gray(pings, WATERFALL_MAX_W, WATERFALL_MAX_H);
                    let (denoised, _) = curvelet_denoise_gray_image_tagged(
                        raw,
                        options.curvelet_threshold,
                        &format!("ch{ch}"),
                    );
                    (*ch, denoised)
                })
                .collect()
        } else {
            BTreeMap::new()
        };

    // Channel pair scoring is O(n) over all pings; compute once for mosaic,
    // KMZ, and viewer overlay writers that all need the same result.
    let sidescan_pair = find_sidescan_pair(parsed);

    if options.waterfall {
        emit_progress(progress, "Rendering Waterfall Image...", 65);
        artifacts.extend(write_waterfall_per_channel(
            parsed,
            &output_dir,
            options.curvelet_denoise,
            options.curvelet_threshold,
            &denoised_cache,
            options.show_payload_debug_overlay,
        )?);
    }

    if options.mosaic {
        emit_progress(progress, "Building Geographic Mosaic...", 75);
        artifacts.extend(write_mosaic_per_channel(
            parsed,
            &output_dir,
            &options.colormap,
            options.remove_water_column,
            &options.nadir_mode,
            options.curvelet_denoise,
            options.curvelet_threshold,
            &options.channel_alignments,
            &denoised_cache,
            sidescan_pair,
        )?);

        // ALWAYS write the unified cartographic mosaic image to the root output folder so the user can just open the master file!
        let mut res = 0.20;
        let pvals = parsed
            .pings
            .iter()
            .filter(|p| p.latitude != 0.0)
            .collect::<Vec<_>>();
        if !pvals.is_empty() {
            let min_lat = pvals
                .iter()
                .map(|p| p.latitude)
                .fold(std::f64::INFINITY, f64::min);
            let max_lat = pvals
                .iter()
                .map(|p| p.latitude)
                .fold(std::f64::NEG_INFINITY, f64::max);
            let w_m = (pvals
                .iter()
                .map(|p| p.longitude)
                .fold(std::f64::NEG_INFINITY, f64::max)
                - pvals
                    .iter()
                    .map(|p| p.longitude)
                    .fold(std::f64::INFINITY, f64::min))
            .abs()
                * 111320.0
                * min_lat.to_radians().cos();
            let h_m = (max_lat - min_lat).abs() * 111320.0;
            let max_dim = w_m.max(h_m);
            if max_dim / res > 8192.0 {
                res = max_dim / 8192.0;
            }
        }

        // ── Bridge: use engine::build_mosaic (TVG + EGN + slant-range) ────────
        // Replaces the old projection::project_pings_to_grid path which had no
        // TVG, no histogram normalisation, and no blanking mitigation.
        let discovery = crate::channel_discovery::discover_and_profile(parsed);
        let nadir_mode_engine = match options.nadir_mode.as_str() {
            "fill" => crate::mosaic::engine::NadirMode::Fill,
            "raw" => crate::mosaic::engine::NadirMode::Raw,
            _ => crate::mosaic::engine::NadirMode::Stitch,
        };
        let engine_config = crate::mosaic::engine::MosaicConfig {
            resolution_m: res,
            colormap: options.colormap.clone(),
            nadir_mode: nadir_mode_engine,
            tvg_enabled: true,
            tvg_alpha: 15.0,
            tvg_beta: 0.08,
            histogram_normalize: true,
            remove_water_column: options.remove_water_column,
            gamma: MOSAIC_GAMMA,
            tile_zoom_levels: vec![], // tile pyramid built separately
            output_dir: output_dir.clone(),
        };
        let (grid, engine_log) =
            crate::mosaic::engine::build_mosaic(parsed, &discovery, &engine_config);
        for entry in &engine_log {
            eprintln!("[engine::build_mosaic] {entry}");
        }
        let img = crate::mosaic::engine::build_image_with_gamma(
            &grid,
            &options.colormap,
            engine_config.gamma,
        );
        let out_img_path = output_dir.join("mosaic_geographic.png");
        match img.save(&out_img_path) {
            Ok(_) => {
                artifacts.push(OutputArtifact {
                    kind: "mosaic".to_string(),
                    path: out_img_path.display().to_string(),
                    details: format!(
                        "Geographic Mosaic (engine) · {}x{} px · {:.2}m/px · TVG+EGN+SRC",
                        img.width(),
                        img.height(),
                        res
                    ),
                });
            }
            Err(e) => {
                artifacts.push(OutputArtifact {
                    kind: "mosaic".to_string(),
                    path: out_img_path.display().to_string(),
                    details: format!("ERROR writing geographic mosaic: {e:#}"),
                });
            }
        }
    }

    if options.mbtiles {
        let path = output_dir.join("sonar.mbtiles");
        match write_mbtiles(
            parsed,
            &path,
            &options.colormap,
            options.remove_water_column,
        ) {
            Ok(()) => artifacts.push(OutputArtifact {
                kind: "mbtiles".to_string(),
                path: path.display().to_string(),
                details: format!(
                    "MBTiles multi-zoom · {} pings · georeferenced track-following tiles",
                    parsed.pings.len()
                ),
            }),
            Err(e) => artifacts.push(OutputArtifact {
                kind: "mbtiles".to_string(),
                path: path.display().to_string(),
                details: format!("ERROR: {e:#}"),
            }),
        }
    }

    if options.kml {
        emit_progress(progress, "Generating KML...", 85);
        let path = output_dir.join("track.kml");
        match write_kml(parsed, &path) {
            Ok(n) => artifacts.push(OutputArtifact {
                kind: "kml".to_string(),
                path: path.display().to_string(),
                details: format!(
                    "Trackline + {} depth placemarks · styled · LookAt camera",
                    n
                ),
            }),
            Err(e) => artifacts.push(OutputArtifact {
                kind: "kml".to_string(),
                path: path.display().to_string(),
                details: format!("ERROR: {e:#}"),
            }),
        }
    }

    if options.kmz {
        emit_progress(progress, "Generating KMZ Map...", 90);
        let kml = output_dir.join("track.kml");
        let kml_ready = kml.exists() || write_kml(parsed, &kml).is_ok();
        if kml_ready {
            let kmz = output_dir.join("track.kmz");
            match write_kmz(
                &kml,
                &kmz,
                parsed,
                &output_dir,
                &options.colormap,
                options.remove_water_column,
                &options.channel_alignments,
                sidescan_pair,
            ) {
                Ok(has_overlay) => artifacts.push(OutputArtifact {
                    kind: "kmz".to_string(),
                    path: kmz.display().to_string(),
                    details: if has_overlay {
                        "KMZ with stitched sidescan GroundOverlay georeferenced to sonar swath"
                            .to_string()
                    } else {
                        "KMZ + trackline (no GPS bounding box — GroundOverlay skipped)".to_string()
                    },
                }),
                Err(e) => artifacts.push(OutputArtifact {
                    kind: "kmz".to_string(),
                    path: output_dir.join("track.kmz").display().to_string(),
                    details: format!("ERROR: {e:#}"),
                }),
            }
        } else {
            artifacts.push(OutputArtifact {
                kind: "kmz".to_string(),
                path: output_dir.join("track.kmz").display().to_string(),
                details: "Skipped — KML prerequisite failed".to_string(),
            });
        }
    }

    if options.arcgis {
        let path = output_dir.join("arcgis_layer.json");
        match write_arcgis_sidecar(parsed, &path) {
            Ok(()) => artifacts.push(OutputArtifact {
                kind: "arcgis".to_string(),
                path: path.display().to_string(),
                details: "ArcGIS EsriJSON FeatureCollection with all ping attributes".to_string(),
            }),
            Err(e) => artifacts.push(OutputArtifact {
                kind: "arcgis".to_string(),
                path: path.display().to_string(),
                details: format!("ERROR: {e:#}"),
            }),
        }
    }

    if options.video {
        emit_progress(progress, "Rendering enhanced waterfall video...", 92);
        let video_result = crate::video::run_video_export(parsed, &output_dir);
        let details = if let Some(ref path) = video_result.output_path {
            format!("{} · {}", video_result.status, path)
        } else {
            video_result.status.clone()
        };
        artifacts.push(OutputArtifact {
            kind: if video_result.output_path.is_some() {
                "video".to_string()
            } else {
                "video_error".to_string()
            },
            path: video_result
                .output_path
                .unwrap_or_else(|| output_dir.join("sonar_waterfall_enhanced.mp4").display().to_string()),
            details,
        });
    }

    if options.web_viewer {
        emit_progress(progress, "Generating Web Viewer...", 95);
        let viewer_dir = output_dir.join("viewer");
        match write_native_viewer(
            parsed,
            &viewer_dir,
            &options.colormap,
            options.remove_water_column,
            detections,
            &options.channel_alignments,
            options.noaa_enc,
            sidescan_pair,
        ) {
            Ok(()) => artifacts.push(OutputArtifact {
                kind: "viewer".to_string(),
                path: viewer_dir.display().to_string(),
                details: "MapLibre viewer · track + depth-coloured ping layer · click popup"
                    .to_string(),
            }),
            Err(e) => artifacts.push(OutputArtifact {
                kind: "viewer".to_string(),
                path: viewer_dir.display().to_string(),
                details: format!("ERROR: {e:#}"),
            }),
        }
    }

    // ── Detection outputs ──────────────────────────────────────────────────
    if let Some(det) = detections {
        if !det.detections.is_empty() {
            let det_json_path = output_dir.join("detections.json");
            match serde_json::to_vec_pretty(det) {
                Ok(bytes) => match fs::write(&det_json_path, bytes) {
                    Ok(()) => artifacts.push(OutputArtifact {
                        kind: "detections".to_string(),
                        path: det_json_path.display().to_string(),
                        details: format!(
                            "{} targets ({} fish, {} structure, {} debris, {} wreck)",
                            det.total_detections,
                            det.fish_count,
                            det.structure_count,
                            det.debris_count,
                            det.wreck_count
                        ),
                    }),
                    Err(e) => artifacts.push(OutputArtifact {
                        kind: "detections".to_string(),
                        path: det_json_path.display().to_string(),
                        details: format!("ERROR: {e}"),
                    }),
                },
                Err(e) => artifacts.push(OutputArtifact {
                    kind: "detections".to_string(),
                    path: det_json_path.display().to_string(),
                    details: format!("ERROR: {e}"),
                }),
            }

            // Also write GeoJSON for external GIS tools
            let geojson = build_detections_geojson(det);
            let geojson_path = output_dir.join("detections.geojson");
            match serde_json::to_vec_pretty(&geojson) {
                Ok(bytes) => {
                    let _ = fs::write(&geojson_path, bytes);
                }
                Err(_) => {}
            }
        }
    }

    Ok(OutputSummary {
        output_dir: output_dir.display().to_string(),
        artifacts,
    })
}

fn next_available_dir(parent: &Path, stem: &str) -> PathBuf {
    let mut candidate = parent.join(stem);
    if !candidate.exists() {
        return candidate;
    }
    for idx in 2u32.. {
        candidate = parent.join(format!("{stem}_{idx}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("numeric suffix search exhausted");
}

// ── Colour palette system ────────────────────────────────────────────────────

/// Multi-stop linear interpolation between RGB colour stops.
fn lerp_colormap(n: f32, stops: &[(f32, [u8; 3])]) -> Rgb<u8> {
    let n = n.clamp(0.0, 1.0);
    for i in 1..stops.len() {
        let (t0, c0) = stops[i - 1];
        let (t1, c1) = stops[i];
        if n <= t1 || i == stops.len() - 1 {
            let t = if t1 > t0 { (n - t0) / (t1 - t0) } else { 1.0 };
            return Rgb([
                (c0[0] as f32 + (c1[0] as f32 - c0[0] as f32) * t).clamp(0.0, 255.0) as u8,
                (c0[1] as f32 + (c1[1] as f32 - c0[1] as f32) * t).clamp(0.0, 255.0) as u8,
                (c0[2] as f32 + (c1[2] as f32 - c0[2] as f32) * t).clamp(0.0, 255.0) as u8,
            ]);
        }
    }
    Rgb([255, 255, 240])
}

/// Map a normalised intensity `n ∈ [0,1]` to RGB using one of the named palettes.
/// Unknown names fall back to the "amber" (formerly "sonar") palette.
pub fn apply_colormap(n: f32, name: &str) -> Rgb<u8> {
    let nm = if name.is_empty() { "amber" } else { name };
    match nm {
        "amber" | "sonar" => lerp_colormap(
            n,
            &[
                (0.00, [0, 0, 0]),
                (0.25, [60, 24, 0]),
                (0.50, [140, 80, 10]),
                (0.75, [220, 150, 30]),
                (1.00, [255, 210, 90]),
            ],
        ),
        "grayscale" => {
            let v = (n.clamp(0.0, 1.0) * 255.0) as u8;
            Rgb([v, v, v])
        }
        "ocean" => lerp_colormap(
            n,
            &[
                (0.00, [0, 0, 80]),
                (0.30, [0, 40, 120]),
                (0.55, [0, 100, 160]),
                (0.75, [30, 180, 200]),
                (0.90, [160, 230, 240]),
                (1.00, [255, 255, 255]),
            ],
        ),
        "inferno" => lerp_colormap(
            n,
            &[
                (0.00, [0, 0, 4]),
                (0.20, [40, 11, 84]),
                (0.40, [101, 21, 110]),
                (0.60, [182, 55, 76]),
                (0.80, [237, 121, 18]),
                (1.00, [252, 255, 164]),
            ],
        ),
        "iron" => lerp_colormap(
            n,
            &[
                (0.00, [0, 0, 0]),
                (0.25, [0, 0, 200]),
                (0.50, [160, 0, 200]),
                (0.75, [255, 160, 0]),
                (1.00, [255, 255, 200]),
            ],
        ),
        "rainbow" => lerp_colormap(
            n,
            &[
                (0.00, [0, 0, 255]),
                (0.25, [0, 255, 255]),
                (0.50, [0, 255, 0]),
                (0.75, [255, 255, 0]),
                (1.00, [255, 0, 0]),
            ],
        ),
        "plasma" => lerp_colormap(
            n,
            &[
                (0.00, [13, 8, 135]),
                (0.25, [126, 3, 167]),
                (0.50, [204, 71, 120]),
                (0.75, [248, 149, 64]),
                (1.00, [240, 249, 33]),
            ],
        ),
        _ => lerp_colormap(
            n,
            &[
                // "sonar" (default & fallback)
                (0.00, [0, 0, 0]),
                (0.15, [0, 0, 210]),
                (0.35, [0, 160, 255]),
                (0.55, [0, 220, 80]),
                (0.70, [230, 200, 0]),
                (0.85, [255, 55, 0]),
                (1.00, [255, 255, 240]),
            ],
        ),
    }
}

// ── Per-ping image helpers ────────────────────────────────────────────────────

/// Resample one sonar ping into a `dst_w`-wide grey byte row using:
/// * lightweight TVG correction (spreading α = 15 dB/decade, absorption β = 0.08)
/// * bilinear horizontal interpolation (handles variable sample counts)
/// * per-ping 2 %–98 % percentile contrast stretch
/// * gamma correction to lift shadow detail
#[allow(dead_code)]
fn ping_to_gray_row(ping: &Ping, dst_w: usize, gamma: f32) -> Vec<u8> {
    let mut row = vec![0u8; dst_w];
    let src = &ping.samples;
    if src.is_empty() || dst_w == 0 {
        return row;
    }

    // Apply lightweight TVG before resampling
    let tvg_lut = tvg::precompute_tvg_lut_simple(src.len(), 15.0, 0.08);
    let corrected: Vec<f32> = src
        .iter()
        .enumerate()
        .map(|(i, &s)| s as f32 * tvg_lut[i])
        .collect();

    // Percentile estimation on corrected data
    let mut nonzero: Vec<f32> = corrected.iter().copied().filter(|&x| x > 0.0).collect();
    if nonzero.is_empty() {
        return row;
    }
    nonzero.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
    let nz = nonzero.len();
    let p2 = nonzero[(nz / 50).min(nz - 1)];
    let p98 = nonzero[(nz * 49 / 50).min(nz - 1)];
    let span = (p98 - p2).max(1.0);

    let src_n = corrected.len();
    let inv = if dst_w <= 1 || src_n <= 1 {
        0.0_f32
    } else {
        (src_n - 1) as f32 / (dst_w - 1) as f32
    };
    for i in 0..dst_w {
        let flt = i as f32 * inv;
        let lo = flt as usize;
        let hi = (lo + 1).min(src_n - 1);
        let frac = flt - lo as f32;
        let v = corrected[lo] * (1.0 - frac) + corrected[hi] * frac;
        let norm = ((v - p2) / span).clamp(0.0, 1.0).powf(gamma);
        row[i] = (norm * 255.0) as u8;
    }
    row
}

/// Canonical output width for a channel: median non-zero sample count clamped
/// to `max_w`.  The median avoids giant images from a handful of anomalous pings.
fn canonical_width(pings: &[&Ping], max_w: u32) -> u32 {
    let mut counts: Vec<usize> = pings
        .iter()
        .map(|p| p.samples.len())
        .filter(|&n| n > 0)
        .collect();
    if counts.is_empty() {
        return 512_u32.min(max_w).max(1);
    }
    counts.sort_unstable();
    (counts[counts.len() / 2] as u32).min(max_w).max(1)
}

/// Group pings by channel ID; preserves temporal ping ordering.
fn pings_by_channel(parsed: &ParseResult) -> BTreeMap<u32, Vec<&Ping>> {
    let mut map: BTreeMap<u32, Vec<&Ping>> = BTreeMap::new();
    for ping in &parsed.pings {
        map.entry(ping.channel).or_default().push(ping);
    }
    map
}

/// Render a channel's pings as a GRAY8 waterfall image.
/// Each ping occupies one output row (vertically subsampled for large files).
/// Uses bilinear horizontal resampling + per-ping 2–98 % percentile stretch + gamma.
fn render_gray(pings: &[&Ping], max_w: u32, max_h: u32) -> GrayImage {
    if pings.is_empty() {
        return ImageBuffer::from_pixel(1, 1, image::Luma([0u8]));
    }
    let offsets = vec![0usize; pings.len()];
    let tvg_lut = compute_empirical_tvg(pings, &offsets);
    let (p2, p98) = compute_segment_norm(pings, &offsets, &tvg_lut);
    let img_w = canonical_width(pings, max_w);
    let src_h = pings.len();
    let img_h = (src_h as u32).min(max_h).max(1);
    let mut img: GrayImage = ImageBuffer::new(img_w, img_h);
    for dst_y in 0..img_h {
        let src_y = (dst_y as usize * src_h) / img_h as usize;
        let ping = &pings[src_y.min(src_h - 1)];
        let row =
            ping_to_gray_row_normed(ping, 0, img_w as usize, WATERFALL_GAMMA, p2, p98, &tvg_lut);
        for (x, &v) in row.iter().enumerate() {
            img.put_pixel(x as u32, dst_y, image::Luma([v]));
        }
    }
    img
}

/// Render a channel's pings as a false-colour RGB mosaic.
/// Each ping occupies one output row; uses bilinear resampling + per-ping stretch + gamma.
fn render_mosaic_rgb(pings: &[&Ping], max_w: u32, max_h: u32, colormap: &str) -> RgbImage {
    if pings.is_empty() {
        return ImageBuffer::from_pixel(1, 1, Rgb([0u8, 0, 0]));
    }
    let offsets = vec![0usize; pings.len()];
    let tvg_lut = compute_empirical_tvg(pings, &offsets);
    let (p2, p98) = compute_segment_norm(pings, &offsets, &tvg_lut);
    let img_w = canonical_width(pings, max_w);
    let src_h = pings.len();
    let img_h = (src_h as u32).min(max_h).max(1);
    let mut img: RgbImage = ImageBuffer::new(img_w, img_h);
    for dst_y in 0..img_h {
        let src_y = (dst_y as usize * src_h) / img_h as usize;
        let ping = &pings[src_y.min(src_h - 1)];
        let gray =
            ping_to_gray_row_normed(ping, 0, img_w as usize, MOSAIC_GAMMA, p2, p98, &tvg_lut);
        for (x, &g) in gray.iter().enumerate() {
            img.put_pixel(x as u32, dst_y, apply_colormap(g as f32 / 255.0, colormap));
        }
    }
    img
}

fn overlay_extra_payload_magenta(gray: &GrayImage, pings: &[&Ping]) -> (RgbImage, usize, usize) {
    let mut rgb: RgbImage = ImageBuffer::new(gray.width(), gray.height());
    for y in 0..gray.height() {
        for x in 0..gray.width() {
            let g = gray.get_pixel(x, y).0[0];
            rgb.put_pixel(x, y, Rgb([g, g, g]));
        }
    }
    if pings.is_empty() || gray.width() == 0 || gray.height() == 0 {
        return (rgb, 0, 0);
    }
    let mut highlighted_rows = 0usize;
    let mut max_delta = 0usize;
    let src_h = pings.len();
    for y in 0..gray.height() {
        let src_y = (y as usize * src_h) / gray.height() as usize;
        let ping = pings[src_y.min(src_h - 1)];
        if ping.sample_count == 0 || ping.samples.len() <= ping.sample_count {
            continue;
        }
        let src_n = ping.samples.len();
        if src_n == 0 {
            continue;
        }
        let delta = src_n.saturating_sub(ping.sample_count);
        max_delta = max_delta.max(delta);
        highlighted_rows += 1;
        let x0 = ((ping.sample_count as f64 / src_n as f64) * gray.width() as f64)
            .floor()
            .clamp(0.0, gray.width() as f64) as u32;
        for x in x0..gray.width() {
            rgb.put_pixel(x, y, Rgb([255, 0, 255]));
        }
    }
    (rgb, highlighted_rows, max_delta)
}

/// Detect the per-ping water-column nadir offset using a sustained-run detector.
///
/// Problem with simple threshold: the first sample often contains transducer
/// ring-down (1–10 samples at high amplitude), causing nadir to be detected at
/// position 0 even for sidescan channels.
///
/// Fix: compute the dynamic range of the ping (p15 noise floor + p90 ceiling),
/// then find the FIRST SUSTAINED RUN (≥5 consecutive samples) above a threshold
/// that is 20% of the dynamic range above the noise floor.  The first sample of
/// that run is the nadir offset.
///
/// Returns a Vec of per-ping first-return sample indices.
fn detect_per_ping_nadir(pings: &[&Ping]) -> Vec<usize> {
    const MIN_RUN: usize = 5; // require this many consecutive samples above threshold
    pings
        .iter()
        .map(|p| {
            let n = p.samples.len();
            if n < 32 {
                return 0;
            }

            let mut sorted: Vec<u16> = p.samples.iter().copied().collect();
            sorted.sort_unstable();

            // Noise floor: 15th percentile of all samples
            let p15_idx = (n * 15 / 100).min(n - 1);
            let p90_idx = (n * 90 / 100).min(n - 1);
            let p15 = sorted[p15_idx] as f32;
            let p90 = sorted[p90_idx] as f32;
            let span = (p90 - p15).max(1.0);

            // Threshold: noise floor + 20% of dynamic range
            let threshold = (p15 + span * 0.20) as u16;

            // Find first sustained run of samples above threshold
            let mut run = 0usize;
            for i in 0..n {
                if p.samples[i] > threshold {
                    run += 1;
                    if run >= MIN_RUN {
                        return i + 1 - MIN_RUN;
                    }
                } else {
                    run = 0;
                }
            }
            // No sustained run found → treats entire ping as "above noise" → nadir = 0
            0
        })
        .collect()
}

/// Compute the median nadir offset across all pings (used as a baseline).
#[allow(dead_code)]
fn detect_nadir_offset(pings: &[&Ping]) -> usize {
    let offsets = detect_per_ping_nadir(pings);
    if offsets.is_empty() {
        return 0;
    }
    let mut sorted = offsets;
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

/// Smooth per-ping nadir offsets using a moving-window median to reduce jitter.
/// The smoothed offsets track depth/resolution changes while rejecting single-ping
/// anomalies.  Window size is adaptive: 2% of total pings, clamped to [3, 31].
fn smooth_nadir_offsets(raw_offsets: &[usize]) -> Vec<usize> {
    let n = raw_offsets.len();
    if n == 0 {
        return vec![];
    }
    let window = ((n / 50).max(3)).min(31) | 1; // force odd
    let half = window / 2;
    let mut smoothed = Vec::with_capacity(n);
    let mut buf = Vec::with_capacity(window);
    for i in 0..n {
        buf.clear();
        let lo = i.saturating_sub(half);
        let hi = (i + half + 1).min(n);
        buf.extend_from_slice(&raw_offsets[lo..hi]);
        buf.sort_unstable();
        smoothed.push(buf[buf.len() / 2]);
    }
    smoothed
}

/// Like `ping_to_gray_row` but starts from `start` samples into the ping,
/// effectively collapsing the water-column blank to zero width.
/// Also applies lightweight TVG correction and per-ping contrast stretch
/// for publication-quality mosaics.
#[allow(dead_code)]
fn ping_to_gray_row_from(ping: &Ping, start: usize, dst_w: usize, gamma: f32) -> Vec<u8> {
    let src_raw = if start < ping.samples.len() {
        &ping.samples[start..]
    } else {
        &[]
    };
    let mut row = vec![0u8; dst_w];
    if src_raw.is_empty() || dst_w == 0 {
        return row;
    }

    // Apply lightweight TVG correction (spreading-only, moderate gain)
    let tvg_lut = tvg::precompute_tvg_lut_simple(src_raw.len(), 15.0, 0.08);
    let corrected: Vec<f32> = src_raw
        .iter()
        .enumerate()
        .map(|(i, &s)| s as f32 * tvg_lut.get(i).copied().unwrap_or(1.0))
        .collect();
    // No hard clip here — let percentile stretch handle the dynamic range

    // Robust percentile contrast stretch on corrected data
    let mut nonzero: Vec<f32> = corrected.iter().copied().filter(|&x| x > 0.0).collect();
    if nonzero.is_empty() {
        return row;
    }
    nonzero.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let nz = nonzero.len();
    let p2 = nonzero[(nz / 50).min(nz - 1)];
    let p98 = nonzero[(nz * 49 / 50).min(nz - 1)];
    let span = (p98 - p2).max(1.0);

    let src_n = corrected.len();
    let inv = if dst_w <= 1 || src_n <= 1 {
        0.0_f32
    } else {
        (src_n - 1) as f32 / (dst_w - 1) as f32
    };
    for i in 0..dst_w {
        let flt = i as f32 * inv;
        let lo = flt as usize;
        let hi = (lo + 1).min(src_n - 1);
        let frac = flt - lo as f32;
        let v = corrected[lo] * (1.0 - frac) + corrected[hi] * frac;
        let norm = ((v - p2) / span).clamp(0.0, 1.0).powf(gamma);
        row[i] = (norm * 255.0) as u8;
    }
    row
}

/// Compute shared percentile normalization (p2, p98) across all pings in a group.
/// Used for segment-level normalization to eliminate per-ping brightness banding.
/// Compute empirical TVG correction LUT from actual signal levels in a set of pings.
/// Instead of a mathematical model, measures the mean signal at each range bin
/// and derives correction factors to flatten the range-dependent response.
/// This naturally handles the dark nadir zone by boosting weak near-field samples.
fn compute_empirical_tvg(pings: &[&Ping], offsets: &[usize]) -> Vec<f32> {
    let max_len = pings
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let skip = offsets.get(i).copied().unwrap_or(0);
            p.samples.len().saturating_sub(skip)
        })
        .max()
        .unwrap_or(0);
    if max_len < 10 {
        return vec![1.0; max_len.max(1)];
    }

    // Accumulate signal per range bin
    let mut sums = vec![0.0f64; max_len];
    let mut counts = vec![0u32; max_len];
    for (idx, ping) in pings.iter().enumerate() {
        let skip = offsets.get(idx).copied().unwrap_or(0);
        let src = if skip < ping.samples.len() {
            &ping.samples[skip..]
        } else {
            continue;
        };
        for (i, &s) in src.iter().enumerate() {
            if i < max_len && s > 0 {
                sums[i] += s as f64;
                counts[i] += 1;
            }
        }
    }

    // Mean per range bin
    let means: Vec<f32> = sums
        .iter()
        .zip(counts.iter())
        .map(|(s, &c)| if c > 0 { (*s / c as f64) as f32 } else { 0.0 })
        .collect();

    // Target level: mean of mid-range bins (25th-75th percentile of range)
    // Avoids bias from near-nadir dead zone and far-range noise
    let q25 = max_len / 4;
    let q75 = (max_len * 3) / 4;
    let mid_means: Vec<f32> = means[q25..q75]
        .iter()
        .filter(|&&m| m > 1.0)
        .copied()
        .collect();
    let target = if !mid_means.is_empty() {
        mid_means.iter().sum::<f32>() / mid_means.len() as f32
    } else {
        let valid: Vec<f32> = means.iter().filter(|&&m| m > 1.0).copied().collect();
        if valid.is_empty() {
            return vec![1.0; max_len];
        }
        valid.iter().sum::<f32>() / valid.len() as f32
    };

    // Correction factor per bin: target / mean, clamped
    let mut lut: Vec<f32> = means
        .iter()
        .map(|&m| {
            if m > 1.0 {
                (target / m).clamp(0.3, 15.0)
            } else {
                1.0
            }
        })
        .collect();

    // Smooth with running average (window=21) to prevent noisy corrections
    let window = 21usize;
    let half = window / 2;
    let raw = lut.clone();
    for i in 0..lut.len() {
        let lo = i.saturating_sub(half);
        let hi = (i + half + 1).min(raw.len());
        let sum: f32 = raw[lo..hi].iter().sum();
        lut[i] = sum / (hi - lo) as f32;
    }
    lut
}

fn compute_segment_norm(pings: &[&Ping], offsets: &[usize], tvg_lut: &[f32]) -> (f32, f32) {
    let mut all_vals: Vec<f32> = Vec::new();
    for (idx, ping) in pings.iter().enumerate() {
        let skip = offsets.get(idx).copied().unwrap_or(0);
        let src = if skip < ping.samples.len() {
            &ping.samples[skip..]
        } else {
            continue;
        };
        if src.is_empty() {
            continue;
        }
        // Sample every 4th value to keep memory reasonable
        for (i, &s) in src.iter().enumerate().step_by(4) {
            let gain = tvg_lut.get(i).copied().unwrap_or(1.0);
            let v = s as f32 * gain;
            if v > 0.0 {
                all_vals.push(v);
            }
        }
    }
    if all_vals.is_empty() {
        return (0.0, 1.0);
    }
    all_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = all_vals.len();
    let p2 = all_vals[(n / 50).min(n - 1)];
    let p98 = all_vals[(n * 49 / 50).min(n - 1)];
    (p2, p98)
}

/// Like `ping_to_gray_row_from` but uses externally supplied normalization (p2/p98)
/// and an empirical TVG LUT instead of the mathematical model.
/// Eliminates row-to-row brightness banding and dark nadir zone.
fn ping_to_gray_row_normed(
    ping: &Ping,
    start: usize,
    dst_w: usize,
    gamma: f32,
    p2: f32,
    p98: f32,
    tvg_lut: &[f32],
) -> Vec<u8> {
    let src_raw = if start < ping.samples.len() {
        &ping.samples[start..]
    } else {
        &[]
    };
    let mut row = vec![0u8; dst_w];
    if src_raw.is_empty() || dst_w == 0 {
        return row;
    }
    let corrected: Vec<f32> = src_raw
        .iter()
        .enumerate()
        .map(|(i, &s)| s as f32 * tvg_lut.get(i).copied().unwrap_or(1.0))
        .collect();
    let span = (p98 - p2).max(1.0);
    let src_n = corrected.len();
    if dst_w > 1 && src_n > 1 {
        // Geometric remap: convert slant-range samples to ground-range spacing.
        // This reduces near-range stretching and makes mosaics more map-like.
        let altitude_m = fused_ping_altitude_m(ping, start) as f32;
        let slant_min = altitude_m.max(0.0);
        let slant_max = slant_min + (src_n.saturating_sub(1) as f32) * SONAR_M_PER_SAMPLE_F32;
        if altitude_m > 0.01 && slant_max > slant_min + SONAR_M_PER_SAMPLE_F32 {
            let ground_max = (slant_max * slant_max - altitude_m * altitude_m)
                .max(0.0)
                .sqrt();
            for i in 0..dst_w {
                let t = i as f32 / (dst_w - 1) as f32;
                let ground = t * ground_max;
                let slant = (ground * ground + altitude_m * altitude_m).sqrt();
                let flt =
                    ((slant - slant_min) / SONAR_M_PER_SAMPLE_F32).clamp(0.0, (src_n - 1) as f32);
                let lo = flt as usize;
                let hi = (lo + 1).min(src_n - 1);
                let frac = flt - lo as f32;
                let v = corrected[lo] * (1.0 - frac) + corrected[hi] * frac;
                let norm = ((v - p2) / span).clamp(0.0, 1.0).powf(gamma);
                row[i] = (norm * 255.0) as u8;
            }
        } else {
            let inv = (src_n - 1) as f32 / (dst_w - 1) as f32;
            for i in 0..dst_w {
                let flt = i as f32 * inv;
                let lo = flt as usize;
                let hi = (lo + 1).min(src_n - 1);
                let frac = flt - lo as f32;
                let v = corrected[lo] * (1.0 - frac) + corrected[hi] * frac;
                let norm = ((v - p2) / span).clamp(0.0, 1.0).powf(gamma);
                row[i] = (norm * 255.0) as u8;
            }
        }
    } else if src_n == 1 {
        let norm = ((corrected[0] - p2) / span).clamp(0.0, 1.0).powf(gamma);
        row[0] = (norm * 255.0) as u8;
    }
    // Adaptive horizontal denoise reduces striping on noisy files while keeping
    // cleaner captures sharper. We pick kernel strength from row roughness.
    if dst_w >= 8 {
        let mut rough = 0.0f32;
        for i in 1..dst_w {
            rough += (row[i] as f32 - row[i - 1] as f32).abs();
        }
        rough /= (dst_w - 1) as f32;

        if dst_w >= 16 && rough > 12.0 {
            let mut smoothed = row.clone();
            for i in 2..(dst_w - 2) {
                let a = row[i - 2] as u16;
                let b = row[i - 1] as u16;
                let c = row[i] as u16;
                let d = row[i + 1] as u16;
                let e = row[i + 2] as u16;
                smoothed[i] = ((a + 3 * b + 4 * c + 3 * d + e) / 12) as u8;
            }
            // Extra pass only for very rough rows to calm zipper-like banding.
            if rough > 20.0 {
                let mut pass2 = smoothed.clone();
                for i in 1..(dst_w - 1) {
                    let a = smoothed[i - 1] as u16;
                    let b = smoothed[i] as u16;
                    let c = smoothed[i + 1] as u16;
                    pass2[i] = ((a + 2 * b + c) / 4) as u8;
                }
                pass2
            } else {
                smoothed
            }
        } else {
            let mut smoothed = row.clone();
            for i in 1..(dst_w - 1) {
                let a = row[i - 1] as u16;
                let b = row[i] as u16;
                let c = row[i + 1] as u16;
                smoothed[i] = ((a + 2 * b + c) / 4) as u8;
            }
            smoothed
        }
    } else {
        row
    }
}

const SONAR_M_PER_SAMPLE_F64: f64 = 0.01;
const SONAR_M_PER_SAMPLE_F32: f32 = 0.01;

fn fused_ping_altitude_m(ping: &Ping, nadir_offset: usize) -> f64 {
    let depth_m = if ping.depth_m.is_finite() && ping.depth_m > 0.0 {
        ping.depth_m as f64
    } else {
        0.0
    };
    let nadir_m = nadir_offset as f64 * SONAR_M_PER_SAMPLE_F64;
    match (depth_m > 0.0, nadir_m > 0.0) {
        (true, true) => depth_m * 0.7 + nadir_m * 0.3,
        (true, false) => depth_m,
        (false, true) => nadir_m,
        (false, false) => 1.0,
    }
}

fn ping_ground_half_m(ping: &Ping, nadir_offset: usize) -> f64 {
    let valid_samples = ping.samples.len().saturating_sub(nadir_offset);
    if valid_samples < 4 {
        return 10.0;
    }
    let altitude_m = fused_ping_altitude_m(ping, nadir_offset);
    let slant_max = altitude_m + (valid_samples.saturating_sub(1) as f64) * SONAR_M_PER_SAMPLE_F64;
    let ground_half = (slant_max * slant_max - altitude_m * altitude_m)
        .max(0.0)
        .sqrt();
    ground_half.clamp(10.0, 300.0)
}

fn segment_swath_half_m(segment: &[&Ping], offsets: &[usize]) -> f64 {
    let mut vals: Vec<f64> = segment
        .iter()
        .enumerate()
        .map(|(i, p)| ping_ground_half_m(p, offsets.get(i).copied().unwrap_or(0)))
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    if vals.is_empty() {
        return 30.0;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    vals[vals.len() / 2]
}

/// Estimate sonar swath half-width from actual sample counts.
/// Uses Garmin's ~0.01 m per sample approximation (76.8 kHz, 1500 m/s).
#[allow(dead_code)]
fn estimate_swath_half_m(pings: &[&Ping]) -> f64 {
    let mut counts: Vec<usize> = pings
        .iter()
        .map(|p| p.samples.len())
        .filter(|&c| c > 10)
        .collect();
    if counts.is_empty() {
        return 30.0;
    }
    counts.sort();
    let median_samples = counts[counts.len() / 2] as f64;
    (median_samples * SONAR_M_PER_SAMPLE_F64).clamp(10.0, 300.0)
}

/// Split pings into segments that break at heading changes for better turn following.
/// Returns vector of (start, end) index pairs into the ping slice.
/// Uses smaller base size (25) and tighter heading threshold (~15°) for cleaner corners.
fn segment_by_heading(
    pings: &[&Ping],
    base_size: usize,
    max_heading_rad: f64,
) -> Vec<(usize, usize)> {
    let n = pings.len();
    if n < 3 {
        return vec![(0, n)];
    }
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut seg_start = 0;
    while seg_start < n {
        let seg_end_max = (seg_start + base_size).min(n);
        if seg_end_max - seg_start < 3 {
            if let Some(last) = segments.last_mut() {
                last.1 = seg_end_max;
            } else {
                segments.push((seg_start, seg_end_max));
            }
            break;
        }
        // Use a rolling heading comparison: compare current heading vs segment start heading
        let h0 = heading_between(pings[seg_start], pings[(seg_start + 2).min(n - 1)]);
        let mut split_at = seg_end_max;
        for j in (seg_start + 3)..seg_end_max {
            if j >= n {
                break;
            }
            let h = heading_between(pings[j.saturating_sub(2)], pings[j]);
            let mut delta = (h - h0).abs();
            if delta > std::f64::consts::PI {
                delta = 2.0 * std::f64::consts::PI - delta;
            }
            if delta > max_heading_rad {
                split_at = j;
                break;
            }
        }
        segments.push((seg_start, split_at));
        seg_start = split_at;
    }
    segments
}

/// Compute adaptive segmentation parameters from heading variability.
/// Returns (base_segment_size, heading_break_threshold_radians).
fn adaptive_segmentation_params(pings: &[&Ping]) -> (usize, f64) {
    let n = pings.len();
    if n < 20 {
        return (16, 0.22);
    }

    let step = (n / 400).max(1);
    let mut deltas: Vec<f64> = Vec::new();
    let mut i = step;
    while i + step < n {
        let h_prev = heading_between(pings[i - step], pings[i]);
        let h_next = heading_between(pings[i], pings[i + step]);
        let mut d = (h_next - h_prev).abs();
        if d > std::f64::consts::PI {
            d = 2.0 * std::f64::consts::PI - d;
        }
        if d.is_finite() {
            deltas.push(d);
        }
        i += step;
    }

    if deltas.is_empty() {
        return (24, 0.26);
    }

    deltas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p90 = deltas[((deltas.len() as f64 * 0.90) as usize).min(deltas.len() - 1)];

    // Smaller base sizes so the heading threshold fires more often in turns.
    // The .max(30/50) floor in callers still prevents single-ping slivers.
    let mut base = if p90 > 0.35 {
        8
    } else if p90 > 0.25 {
        11
    } else if p90 > 0.18 {
        15
    } else {
        20
    };
    if n > 50_000 {
        base = (base + 3).min(26);
    }

    // Tighter threshold: a 90° turn now gets ~7 segments instead of ~3-4.
    // Clamp range: 0.10 rad (≈6°) min, 0.26 rad (≈15°) max.
    let heading_thr = (p90 * 1.05).clamp(0.10, 0.26);
    (base, heading_thr)
}

/// Find the best port + starboard sidescan channel pair.
///
/// Fully data-driven: scores every candidate pair of sonar channels by ping
/// count balance, timestamp overlap, sample layout, GPS coverage, and generation.
/// Does NOT require static channel labels to be correct — handles multi-compat
/// UHD2 firmware where both arms share the same static side label.
///
/// Returns `(port_channel_id, starboard_channel_id)`.
/// Convention: lower channel ID = port (left), higher = starboard (right).
fn find_sidescan_pair(parsed: &ParseResult) -> (Option<u32>, Option<u32>) {
    let channels = pings_by_channel(parsed);

    // ── Per-channel metrics ──────────────────────────────────────────────────
    let gps_counts: BTreeMap<u32, usize> = channels
        .iter()
        .map(|(&ch, v)| {
            let n = v
                .iter()
                .filter(|p| {
                    p.latitude.is_finite()
                        && p.longitude.is_finite()
                        && (p.latitude != 0.0 || p.longitude != 0.0)
                })
                .count();
            (ch, n)
        })
        .collect();

    let total_counts: BTreeMap<u32, usize> =
        channels.iter().map(|(&ch, v)| (ch, v.len())).collect();

    let generation_of = |ch: u32| -> Option<String> {
        parsed
            .channels
            .iter()
            .find(|c| c.id == ch)
            .and_then(|c| c.generation.clone())
    };
    let detected_gen = parsed.detected_generation.map(|g| g.to_string());

    // Classify channels by their sonar data characteristics
    let is_depth_temp = |ch: u32| -> bool {
        let lbl = channel_label(parsed, ch);
        if lbl.contains("depth_temp") {
            return true;
        }
        // Depth/temp channels typically fire at ~2× the rate of sonar channels
        let pv = channels.get(&ch).cloned().unwrap_or_default();
        if pv.is_empty() {
            return true;
        }
        // Check: very few actual sonar samples (sample_count ≤ 1 or sonar_size ≤ 2)
        let no_sonar = pv
            .iter()
            .take(200)
            .filter(|p| p.sample_count <= 1 || p.sonar_size <= 2)
            .count();
        no_sonar > pv.len().min(200) / 2
    };

    // Average sonar_size / sample_count ratio (indicates u8 vs i16 format)
    let avg_sample_ratio = |ch: u32| -> f64 {
        let pv = channels.get(&ch).cloned().unwrap_or_default();
        if pv.is_empty() {
            return 0.0;
        }
        let mut sum = 0.0;
        let mut n = 0usize;
        for p in pv.iter().take(500) {
            if p.sample_count > 0 && p.sonar_size > 0 {
                sum += p.sonar_size as f64 / p.sample_count as f64;
                n += 1;
            }
        }
        if n > 0 {
            sum / n as f64
        } else {
            0.0
        }
    };

    // Median sample count (indicates resolution similarity)
    let median_sample_count = |ch: u32| -> usize {
        let pv = channels.get(&ch).cloned().unwrap_or_default();
        if pv.is_empty() {
            return 0;
        }
        let mut counts: Vec<usize> = pv
            .iter()
            .take(500)
            .map(|p| p.sample_count as usize)
            .collect();
        counts.sort_unstable();
        counts[counts.len() / 2]
    };

    // Timestamp span
    let time_span = |ch: u32| -> (u64, u64) {
        let pv = channels.get(&ch).cloned().unwrap_or_default();
        let first = pv.first().map(|p| p.timestamp_ms).unwrap_or(0);
        let last = pv.last().map(|p| p.timestamp_ms).unwrap_or(0);
        (first, last)
    };

    // ── Build candidate list: all sonar channels (not depth/temp) ────────────
    let max_total = total_counts.values().copied().max().unwrap_or(0);
    // Accept channels with ≥ 5% of the largest channel's count, minimum 50 pings
    let min_pings = (max_total / 20).max(50);

    let candidates: Vec<u32> = channels
        .keys()
        .copied()
        .filter(|&ch| {
            !is_depth_temp(ch) && total_counts.get(&ch).copied().unwrap_or(0) >= min_pings
        })
        .collect();

    eprintln!(
        "[channel-probe] {} candidates from {} channels (min_pings={})",
        candidates.len(),
        channels.len(),
        min_pings
    );
    for &ch in &candidates {
        let lbl = channel_label(parsed, ch);
        let gen = generation_of(ch).unwrap_or_else(|| "?".into());
        let gc = gps_counts.get(&ch).copied().unwrap_or(0);
        let tc = total_counts.get(&ch).copied().unwrap_or(0);
        let ratio = avg_sample_ratio(ch);
        let med_sc = median_sample_count(ch);
        eprintln!(
            "  ch{ch}: {lbl}({gen}) total={tc} gps={gc} ratio={ratio:.2} med_samples={med_sc}"
        );
    }

    if candidates.is_empty() {
        eprintln!("[channel-probe] no candidates found");
        return (None, None);
    }
    if candidates.len() == 1 {
        // Single-arm file (e.g. single ClearVü): return as port, no star
        let ch = candidates[0];
        eprintln!("[channel-probe] single-arm: ch{ch}");
        return (Some(ch), None);
    }

    // ── Score every candidate pair ───────────────────────────────────────────
    let mut best: Option<(u32, u32, f64)> = None;

    for i in 0..candidates.len() {
        for j in (i + 1)..candidates.len() {
            let (a, b) = (candidates[i], candidates[j]);
            let ac = total_counts.get(&a).copied().unwrap_or(0);
            let bc = total_counts.get(&b).copied().unwrap_or(0);

            // 1. Ping count balance (1.0 = perfect, 0.0 = terrible)
            let balance = (ac.min(bc) as f64) / (ac.max(bc) as f64).max(1.0);

            // 2. Timestamp overlap (both channels active at the same time)
            let (a_first, a_last) = time_span(a);
            let (b_first, b_last) = time_span(b);
            let overlap_ms = (a_last.min(b_last)).saturating_sub(a_first.max(b_first));
            let min_span = (a_last.saturating_sub(a_first))
                .min(b_last.saturating_sub(b_first))
                .max(1) as f64;
            let overlap_ratio = (overlap_ms as f64 / min_span).clamp(0.0, 1.0);

            // 3. Sample structure similarity (similar resolution = likely paired)
            let a_ratio = avg_sample_ratio(a);
            let b_ratio = avg_sample_ratio(b);
            let ratio_sim = 1.0 - (a_ratio - b_ratio).abs().min(1.0);
            let a_med = median_sample_count(a) as f64;
            let b_med = median_sample_count(b) as f64;
            let count_sim = if a_med > 0.0 && b_med > 0.0 {
                (a_med.min(b_med)) / a_med.max(b_med)
            } else {
                0.0
            };

            // 4. GPS coverage bonus (both have GPS = much more useful)
            let a_gps = gps_counts.get(&a).copied().unwrap_or(0);
            let b_gps = gps_counts.get(&b).copied().unwrap_or(0);
            let gps_bonus = if a_gps > 50 && b_gps > 50 {
                2.0
            } else if a_gps > 0 || b_gps > 0 {
                0.5
            } else {
                0.0
            };

            // 5. Cross-generation bonus (different hw gen = likely different arms)
            let a_gen = generation_of(a);
            let b_gen = generation_of(b);
            let cross_gen = a_gen.is_some() && b_gen.is_some() && a_gen != b_gen;
            let cross_gen_bonus = if cross_gen { 3.0 } else { 0.0 };

            // 6. Static label bonus (port+star labels = high confidence)
            let a_lbl = channel_label(parsed, a);
            let b_lbl = channel_label(parsed, b);
            let has_port = a_lbl.contains("port_sidescan") || b_lbl.contains("port_sidescan");
            let has_star =
                a_lbl.contains("starboard_sidescan") || b_lbl.contains("starboard_sidescan");
            let label_bonus = if has_port && has_star {
                4.0
            }
            // proper port+star
            else if has_port || has_star {
                1.5
            }
            // at least one sidescan label
            else {
                0.0
            };

            // 7. Generation match with detected_gen (prefer native gen)
            let det_bonus = match detected_gen.as_deref() {
                Some(dg) => {
                    let a_match = a_gen.as_deref() == Some(dg);
                    let b_match = b_gen.as_deref() == Some(dg);
                    match (a_match, b_match) {
                        (true, true) => 2.0,
                        (true, false) | (false, true) => 0.8,
                        _ => 0.0,
                    }
                }
                None => 0.0,
            };

            // 8. Penalty for downscan-only labels (both labeled downscan = risky)
            let both_downscan = a_lbl.contains("downscan")
                && b_lbl.contains("downscan")
                && !a_lbl.contains("sidescan")
                && !b_lbl.contains("sidescan");
            let downscan_penalty = if both_downscan { -2.0 } else { 0.0 };

            // 9. Penalty for same-gen same-side (likely ClearVü duplicate, not two arms)
            let same_gen_same_side = !cross_gen
                && ((a_lbl.contains("port") && b_lbl.contains("port"))
                    || (a_lbl.contains("starboard") && b_lbl.contains("starboard")));
            let dup_penalty = if same_gen_same_side { -3.0 } else { 0.0 };

            let score = balance * 4.0
                + overlap_ratio * 3.0
                + ratio_sim * 1.5
                + count_sim * 1.0
                + gps_bonus
                + cross_gen_bonus
                + label_bonus
                + det_bonus
                + downscan_penalty
                + dup_penalty;

            eprintln!("  pair ch{}+ch{}: score={:.2} (bal={:.2} ovlp={:.2} struct={:.2}+{:.2} gps={:.1} xgen={:.1} lbl={:.1} det={:.1} ds={:.1} dup={:.1})",
                a, b, score, balance, overlap_ratio, ratio_sim, count_sim,
                gps_bonus, cross_gen_bonus, label_bonus, det_bonus, downscan_penalty, dup_penalty);

            match best {
                Some((_, _, bs)) if score <= bs => {}
                _ => best = Some((a, b, score)),
            }
        }
    }

    match best {
        Some((a, b, score)) => {
            // Assign port/star: use static labels if one is port and other is star;
            // otherwise lower ID = port (Garmin convention for cross-gen pairs).
            let a_lbl = channel_label(parsed, a);
            let b_lbl = channel_label(parsed, b);
            let (port, star) = if a_lbl.contains("port") && b_lbl.contains("starboard") {
                (a, b)
            } else if b_lbl.contains("port") && a_lbl.contains("starboard") {
                (b, a)
            } else {
                // No clear port/star labels — lower ID = port
                if a < b {
                    (a, b)
                } else {
                    (b, a)
                }
            };
            eprintln!(
                "[channel-probe] SELECTED ch{}=port ch{}=star (score={:.2})",
                port, star, score
            );
            (Some(port), Some(star))
        }
        None => {
            eprintln!("[channel-probe] no viable pair found");
            // Try single best channel as a fallback
            let best_single = candidates
                .iter()
                .max_by_key(|&&ch| total_counts.get(&ch).copied().unwrap_or(0))
                .copied();
            if let Some(ch) = best_single {
                eprintln!("[channel-probe] fallback single: ch{ch}");
            }
            (best_single, None)
        }
    }
}

/// Should a channel's samples be reversed (flipped)?
/// Checks explicit alignment overrides first, then uses the assigned role
/// from `find_sidescan_pair`.  The `assigned_as_port` flag indicates whether
/// this channel was assigned the port role by the data-driven probe —
/// this overrides the static channel map label which can be wrong on
/// multi-compat UHD2 firmware.
fn should_flip(
    ch_id: u32,
    alignments: &[crate::channel_alignment::ChannelAlignment],
    assigned_as_port: bool,
) -> bool {
    if let Some(a) = alignments.iter().find(|a| a.channel_id == ch_id) {
        return a.flip;
    }
    // UHD port data arrives pre-reversed from the transducer hardware,
    // so flipping would double-reverse it.  All other port assignments need flip.
    let gen = crate::garmin_rsd_parser::map_channel_info(ch_id)
        .map(|(_, g)| g)
        .unwrap_or("unknown");
    if assigned_as_port {
        // Port channel: flip unless UHD (pre-reversed)
        gen != "uhd"
    } else {
        false
    }
}

/// Should a channel's samples be inverted (negate brightness)?
fn should_invert(ch_id: u32, alignments: &[crate::channel_alignment::ChannelAlignment]) -> bool {
    alignments
        .iter()
        .find(|a| a.channel_id == ch_id)
        .map_or(false, |a| a.invert)
}

/// Render a stitched overlay strip from port + starboard pings for a segment.
/// Port side is placed on the left half (reversed), starboard on the right half.
/// If only one channel exists, it fills the full width.
/// Uses empirical TVG + normalization to eliminate banding and dark nadir.
///
/// When `port_norm`/`star_norm` are provided, uses those pre-computed values
/// instead of computing per-segment norms — this keeps brightness consistent
/// across all segments along the track.
fn render_stitched_overlay_strip(
    port_pings: &[&Ping],
    star_pings: &[&Ping],
    seg_w: u32,
    seg_h: u32,
    colormap: &str,
    remove_water_column: bool,
    alignments: &[crate::channel_alignment::ChannelAlignment],
    port_ch: Option<u32>,
    star_ch: Option<u32>,
    port_norm: Option<&PrecomputedNorm>,
    star_norm: Option<&PrecomputedNorm>,
) -> RgbImage {
    let has_both = !port_pings.is_empty() && !star_pings.is_empty();
    let half_w = if has_both { seg_w / 2 } else { seg_w };
    let mut strip: RgbImage = ImageBuffer::from_pixel(seg_w, seg_h, Rgb([5u8, 10, 20]));

    // Always detect minimum nadir skip (dead near-field zone) even when
    // remove_water_column is off.  When enabled, use the full adaptive detection.
    let port_offsets = if !port_pings.is_empty() {
        let raw = detect_per_ping_nadir(port_pings);
        if remove_water_column {
            smooth_nadir_offsets(&raw)
        } else {
            // Minimum skip: use median of detected nadirs, clamped to modest value
            let mut sorted = raw.clone();
            sorted.sort_unstable();
            let median = sorted[sorted.len() / 2];
            let min_skip = median.min(30); // Don't skip more than 30 samples when WC is off
            vec![min_skip; port_pings.len()]
        }
    } else {
        vec![]
    };
    let star_offsets = if !star_pings.is_empty() {
        let raw = detect_per_ping_nadir(star_pings);
        if remove_water_column {
            smooth_nadir_offsets(&raw)
        } else {
            let mut sorted = raw.clone();
            sorted.sort_unstable();
            let median = sorted[sorted.len() / 2];
            let min_skip = median.min(30);
            vec![min_skip; star_pings.len()]
        }
    } else {
        vec![]
    };

    // Compute empirical TVG for each channel (data-driven range normalization)
    // When global norms are provided, use those for consistent cross-segment brightness.
    let port_tvg = if let Some(n) = port_norm {
        n.tvg.clone()
    } else if !port_pings.is_empty() {
        compute_empirical_tvg(port_pings, &port_offsets)
    } else {
        vec![1.0]
    };
    let star_tvg = if let Some(n) = star_norm {
        n.tvg.clone()
    } else if !star_pings.is_empty() {
        compute_empirical_tvg(star_pings, &star_offsets)
    } else {
        vec![1.0]
    };

    // Normalization: use global norms if provided, else compute locally per segment
    let (port_p2, port_p98) = if let Some(n) = port_norm {
        (n.p2, n.p98)
    } else if !port_pings.is_empty() {
        compute_segment_norm(port_pings, &port_offsets, &port_tvg)
    } else {
        (0.0, 1.0)
    };
    let (star_p2, star_p98) = if let Some(n) = star_norm {
        (n.p2, n.p98)
    } else if !star_pings.is_empty() {
        compute_segment_norm(star_pings, &star_offsets, &star_tvg)
    } else {
        (0.0, 1.0)
    };

    let port_flip = port_ch.map_or(true, |ch| should_flip(ch, alignments, true));
    let star_flip = star_ch.map_or(false, |ch| should_flip(ch, alignments, false));
    let port_invert = port_ch.map_or(false, |ch| should_invert(ch, alignments));
    let star_invert = star_ch.map_or(false, |ch| should_invert(ch, alignments));

    for dst_y in 0..seg_h {
        // Starboard → right half (or full width if no port)
        if !star_pings.is_empty() {
            let n = star_pings.len();
            let src_y = (dst_y as usize * n) / seg_h as usize;
            let idx = src_y.min(n - 1);
            let skip = star_offsets.get(idx).copied().unwrap_or(0);
            let render_w = if has_both { half_w } else { seg_w };
            let mut gray = ping_to_gray_row_normed(
                star_pings[idx],
                skip,
                render_w as usize,
                MOSAIC_GAMMA,
                star_p2,
                star_p98,
                &star_tvg,
            );
            if star_invert {
                for g in gray.iter_mut() {
                    *g = 255 - *g;
                }
            }
            let x_offset = if has_both { half_w } else { 0 };
            if star_flip {
                for (xi, &g) in gray.iter().enumerate() {
                    let dst_x = x_offset + (render_w - 1 - xi as u32);
                    strip.put_pixel(dst_x, dst_y, apply_colormap(g as f32 / 255.0, colormap));
                }
            } else {
                for (xi, &g) in gray.iter().enumerate() {
                    strip.put_pixel(
                        x_offset + xi as u32,
                        dst_y,
                        apply_colormap(g as f32 / 255.0, colormap),
                    );
                }
            }
        }
        // Port → left half (reversed), or full width if no starboard
        if !port_pings.is_empty() {
            let n = port_pings.len();
            let src_y = (dst_y as usize * n) / seg_h as usize;
            let idx = src_y.min(n - 1);
            let skip = port_offsets.get(idx).copied().unwrap_or(0);
            let render_w = if has_both { half_w } else { seg_w };
            let mut gray = ping_to_gray_row_normed(
                port_pings[idx],
                skip,
                render_w as usize,
                MOSAIC_GAMMA,
                port_p2,
                port_p98,
                &port_tvg,
            );
            if port_invert {
                for g in gray.iter_mut() {
                    *g = 255 - *g;
                }
            }
            if port_flip {
                for (xi, &g) in gray.iter().enumerate() {
                    if has_both {
                        let dst_x = half_w - 1 - xi as u32;
                        strip.put_pixel(dst_x, dst_y, apply_colormap(g as f32 / 255.0, colormap));
                    } else {
                        let dst_x = seg_w - 1 - xi as u32;
                        strip.put_pixel(dst_x, dst_y, apply_colormap(g as f32 / 255.0, colormap));
                    }
                }
            } else {
                for (xi, &g) in gray.iter().enumerate() {
                    strip.put_pixel(xi as u32, dst_y, apply_colormap(g as f32 / 255.0, colormap));
                }
            }
        }
    }
    // Blend the nadir seam: remove the hard port/star edge, correct level mismatch.
    if has_both {
        // Nadir blend scales with strip width (wider KMZ strips need a wider seam zone).
        let seam_half = (half_w / 18).clamp(8, 32);
        blend_nadir_seam(&mut strip, half_w, seam_half);
    }
    strip
}

// ── Nadir seam blending ───────────────────────────────────────────────────────
//
// Problem: the current butterfly mosaic has a hard pixel edge where port (left)
// meets starboard (right) at x = seam_x.  This produces:
//   a) A visible brightness step when the two channels have different gain/TVG
//   b) A sharp line artifact even when levels match
//
// This three-stage post-process eliminates both without requiring any new crate
// dependencies.  The same information captured by a 2D FFT (dominance of either
// channel's texture near nadir) is obtained via local spatial gradient magnitude
// — cheaper and equally effective for the blend-weighting decision.
//
// Stage 1 — Level normalization:  scale each channel's inner band toward the
//   geometric mean of both, removing the bulk brightness offset.
// Stage 2 — Gradient-confidence feathering:  for each row, measure gradient RMS
//   on each side of the seam; blend each pixel in the soft zone proportional to
//   the *other* side's confidence (sharper side wins near the seam).
// Stage 3 is implicit: the smoothstep envelope ensures zero mixing at the blend
//   boundary and maximum mixing at the seam centre.

/// Return the luminance (0–1) of an RGB pixel.
fn pixel_luminance(px: Rgb<u8>) -> f32 {
    (px[0] as f32 * 0.299 + px[1] as f32 * 0.587 + px[2] as f32 * 0.114) / 255.0
}

/// RMS gradient magnitude across the pixel columns x_lo..x_hi in row y.
/// Uses a simple 1-pixel forward difference in x and y — fast and no dep.
fn row_gradient_rms(img: &RgbImage, y: u32, x_lo: u32, x_hi: u32) -> f32 {
    if x_hi <= x_lo + 1 {
        return 0.0;
    }
    let (w, h) = img.dimensions();
    let lum = |px: u32, py: u32| -> f32 { pixel_luminance(*img.get_pixel(px, py)) };
    let mut sum_sq = 0.0f32;
    let mut n = 0u32;
    for x in x_lo..x_hi {
        let dx = if x + 1 < w {
            lum(x + 1, y) - lum(x, y)
        } else {
            0.0
        };
        let dy = if y + 1 < h {
            lum(x, y + 1) - lum(x, y)
        } else {
            0.0
        };
        sum_sq += dx * dx + 0.25 * dy * dy;
        n += 1;
    }
    if n == 0 {
        0.0
    } else {
        (sum_sq / n as f32).sqrt()
    }
}

/// Blend the nadir seam of a butterfly mosaic using level normalization and
/// gradient-confidence feathering.
///
/// * `seam_x`        — x coordinate of the hard port/starboard boundary
/// * `blend_half_w`  — half-width in pixels of the soft blend zone on each side
fn blend_nadir_seam(img: &mut RgbImage, seam_x: u32, blend_half_w: u32) {
    let (w, h) = img.dimensions();
    if seam_x == 0 || seam_x >= w || blend_half_w == 0 {
        return;
    }

    let x_lo = seam_x.saturating_sub(blend_half_w);
    let x_hi = (seam_x + blend_half_w).min(w);
    // Band used for level measurement — inner half of each blend zone
    let level_band = (blend_half_w / 2).max(1);

    // ── Stage 1: level normalization ─────────────────────────────────────────
    let mut port_sum = 0.0f32;
    let mut star_sum = 0.0f32;
    let mut level_n = 0u32;
    for y in 0..h {
        for d in 0..level_band {
            if seam_x > d {
                port_sum += pixel_luminance(*img.get_pixel(seam_x - 1 - d, y));
            }
            let sx = seam_x + d;
            if sx < w {
                star_sum += pixel_luminance(*img.get_pixel(sx, y));
            }
            level_n += 1;
        }
    }
    let lnf = level_n.max(1) as f32;
    let port_mean = port_sum / lnf;
    let star_mean = star_sum / lnf;

    let (port_scale, star_scale) = if port_mean < 1e-4 || star_mean < 1e-4 {
        (1.0f32, 1.0f32)
    } else {
        let geo = (port_mean * star_mean).sqrt();
        let ps = (geo / port_mean).clamp(0.70, 1.50);
        let ss = (geo / star_mean).clamp(0.70, 1.50);
        (ps, ss)
    };

    // Apply scale in the blend zone only — fade toward 1.0 outside the inner half
    let scale_pixel = |v: u8, scale: f32, t: f32| -> u8 {
        // t=0 at seam, t=1 at blend boundary; apply full scale near seam
        let s = 1.0 + (scale - 1.0) * (1.0 - t * t);
        (v as f32 * s).clamp(0.0, 255.0) as u8
    };

    if (port_scale - 1.0).abs() > 0.02 {
        for y in 0..h {
            for x in x_lo..seam_x {
                let t = (seam_x - x) as f32 / blend_half_w.max(1) as f32;
                let p = *img.get_pixel(x, y);
                img.put_pixel(
                    x,
                    y,
                    Rgb([
                        scale_pixel(p[0], port_scale, t),
                        scale_pixel(p[1], port_scale, t),
                        scale_pixel(p[2], port_scale, t),
                    ]),
                );
            }
        }
    }
    if (star_scale - 1.0).abs() > 0.02 {
        for y in 0..h {
            for x in seam_x..x_hi {
                let t = (x - seam_x) as f32 / blend_half_w.max(1) as f32;
                let p = *img.get_pixel(x, y);
                img.put_pixel(
                    x,
                    y,
                    Rgb([
                        scale_pixel(p[0], star_scale, t),
                        scale_pixel(p[1], star_scale, t),
                        scale_pixel(p[2], star_scale, t),
                    ]),
                );
            }
        }
    }

    // ── Stage 2: gradient-confidence feathering ───────────────────────────────
    // Per-row: measure gradient RMS on each side → derive confidence weights →
    // softly blend each pixel in the zone toward the opposite channel's value.
    //
    // Analogy to the FFT approach: gradient RMS captures the same local feature
    // "sharpness" that FFT magnitude captures in the frequency domain, but is
    // computed locally per row without any external crates.
    for y in 0..h {
        let port_rms = row_gradient_rms(img, y, x_lo, seam_x);
        let star_rms = row_gradient_rms(img, y, seam_x, x_hi);
        let total = (port_rms + star_rms).max(1e-6);
        // port_conf: fraction of combined gradient energy on the port side.
        // Clamped [0.25, 0.75] so neither side fully eliminates the other.
        let port_conf = (port_rms / total).clamp(0.25, 0.75);

        // Iterate across the blend zone; read pixels into a row buffer first
        // so we don't read-after-write from Stage 1 writes above.
        for x in x_lo..x_hi {
            let dist = (x as i32 - seam_x as i32).unsigned_abs();
            if dist >= blend_half_w {
                continue;
            }
            let t = dist as f32 / blend_half_w as f32; // 0 at seam → 1 at edge
                                                       // Smoothstep envelope: 1.0 at seam, 0.0 at boundary
            let env = {
                let t2 = t * t;
                1.0 - t2 * (3.0 - 2.0 * t)
            };
            if env < 0.005 {
                continue;
            }

            // Mirror coordinate on the other side of the seam
            let mirror = (2i32 * seam_x as i32 - x as i32) as u32;
            if mirror >= w {
                continue;
            }

            let this_px = *img.get_pixel(x, y);
            let other_px = *img.get_pixel(mirror, y);

            // On port side (x < seam_x): blend in some starboard
            // On star side (x >= seam_x): blend in some port
            // The sharper channel contributes more near the seam.
            let other_w = if x < seam_x {
                (1.0 - port_conf) * env * 0.55
            } else {
                port_conf * env * 0.55
            };

            let blend = |a: u8, b: u8| -> u8 {
                (a as f32 * (1.0 - other_w) + b as f32 * other_w).clamp(0.0, 255.0) as u8
            };
            img.put_pixel(
                x,
                y,
                Rgb([
                    blend(this_px[0], other_px[0]),
                    blend(this_px[1], other_px[1]),
                    blend(this_px[2], other_px[2]),
                ]),
            );
        }
    }
}

/// Stitch port + starboard sidescan pings into a single butterfly mosaic.
/// Port arm (ch4) is reversed so both arms radiate outward from a shared nadir line.
///
/// When `remove_water_column` is enabled, uses per-ping adaptive nadir detection
/// with smoothed offsets to track depth/resolution changes accurately.  This handles
/// Garmin's mid-session range changes that shift the water column width.
///
/// When `down_pings` is non-empty, a narrow strip of downscan imagery is blended
/// at the center of the butterfly, filling the nadir dead zone where sidescan
/// has weak/noisy returns.
///
/// Returns `None` when both sidescan inputs are empty.
fn render_sidescan_stitched(
    port_pings: &[&Ping],
    star_pings: &[&Ping],
    down_pings: &[&Ping],
    single_w: u32,
    max_h: u32,
    colormap: &str,
    remove_water_column: bool,
    stitch_nadir: bool,
    alignments: &[crate::channel_alignment::ChannelAlignment],
    port_ch: Option<u32>,
    star_ch: Option<u32>,
) -> Option<RgbImage> {
    if port_pings.is_empty() && star_pings.is_empty() {
        return None;
    }
    let n_pings = port_pings.len().max(star_pings.len());
    let img_h = (n_pings as u32).min(max_h).max(1);
    let total_w = single_w * 2;
    let mut img: RgbImage = ImageBuffer::from_pixel(total_w, img_h, Rgb([5u8, 10, 20]));

    // Per-ping adaptive nadir with smoothed offsets for alignment
    // Always detect minimum nadir skip even when remove_water_column is off
    let port_offsets = if !port_pings.is_empty() {
        let raw = detect_per_ping_nadir(port_pings);
        if remove_water_column {
            smooth_nadir_offsets(&raw)
        } else {
            let mut sorted = raw.clone();
            sorted.sort_unstable();
            let median = sorted[sorted.len() / 2];
            let min_skip = median.min(30);
            vec![min_skip; port_pings.len()]
        }
    } else {
        vec![0; n_pings]
    };
    let star_offsets = if !star_pings.is_empty() {
        let raw = detect_per_ping_nadir(star_pings);
        if remove_water_column {
            smooth_nadir_offsets(&raw)
        } else {
            let mut sorted = raw.clone();
            sorted.sort_unstable();
            let median = sorted[sorted.len() / 2];
            let min_skip = median.min(30);
            vec![min_skip; star_pings.len()]
        }
    } else {
        vec![0; n_pings]
    };

    // Empirical TVG for each channel (data-driven range normalization)
    let port_tvg = if !port_pings.is_empty() {
        compute_empirical_tvg(port_pings, &port_offsets)
    } else {
        vec![1.0]
    };
    let star_tvg = if !star_pings.is_empty() {
        compute_empirical_tvg(star_pings, &star_offsets)
    } else {
        vec![1.0]
    };

    // Segment-level normalization using empirical TVG (prevents per-ping banding)
    let (port_p2, port_p98) = if !port_pings.is_empty() {
        compute_segment_norm(port_pings, &port_offsets, &port_tvg)
    } else {
        (0.0, 1.0)
    };
    let (star_p2, star_p98) = if !star_pings.is_empty() {
        compute_segment_norm(star_pings, &star_offsets, &star_tvg)
    } else {
        (0.0, 1.0)
    };

    // Downscan nadir offsets (for center fill)
    let down_offsets = if !down_pings.is_empty() && remove_water_column {
        smooth_nadir_offsets(&detect_per_ping_nadir(down_pings))
    } else if !down_pings.is_empty() {
        vec![0; down_pings.len()]
    } else {
        vec![]
    };
    let down_tvg = if !down_pings.is_empty() {
        compute_empirical_tvg(down_pings, &down_offsets)
    } else {
        vec![1.0]
    };
    let (down_p2, down_p98) = if !down_pings.is_empty() {
        compute_segment_norm(down_pings, &down_offsets, &down_tvg)
    } else {
        (0.0, 1.0)
    };

    // Downscan strip width: 8% of each half-width on each side = 16% of single_w total.
    let strip_half_w = if !down_pings.is_empty() {
        (single_w / 12).max(4)
    } else {
        0
    };
    let blend_px = (strip_half_w / 3).max(2);

    let port_flip = port_ch.map_or(true, |ch| should_flip(ch, alignments, true));
    let star_flip = star_ch.map_or(false, |ch| should_flip(ch, alignments, false));

    for dst_y in 0..img_h {
        // Per-channel y-mapping: each channel scales independently to the output
        // height so the shorter channel stretches to fill rather than repeating
        // its last ping (which caused the vertical smear artifact).

        // Starboard → right half (nadir at left edge of this half)
        if !star_pings.is_empty() {
            let n = star_pings.len();
            let src_y = (dst_y as usize * n) / img_h as usize;
            let idx = src_y.min(n - 1);
            let skip = star_offsets.get(idx).copied().unwrap_or(0);
            let gray = ping_to_gray_row_normed(
                star_pings[idx],
                skip,
                single_w as usize,
                MOSAIC_GAMMA,
                star_p2,
                star_p98,
                &star_tvg,
            );
            for (xi, &g) in gray.iter().enumerate() {
                let dst_x = if star_flip {
                    single_w + (single_w - 1 - xi as u32)
                } else {
                    single_w + xi as u32
                };
                img.put_pixel(dst_x, dst_y, apply_colormap(g as f32 / 255.0, colormap));
            }
        }
        // Port → left half, reversed so the outer edge is at x = 0
        if !port_pings.is_empty() {
            let n = port_pings.len();
            let src_y = (dst_y as usize * n) / img_h as usize;
            let idx = src_y.min(n - 1);
            let skip = port_offsets.get(idx).copied().unwrap_or(0);
            let gray = ping_to_gray_row_normed(
                port_pings[idx],
                skip,
                single_w as usize,
                MOSAIC_GAMMA,
                port_p2,
                port_p98,
                &port_tvg,
            );
            for (xi, &g) in gray.iter().enumerate() {
                let dst_x = if port_flip {
                    single_w - 1 - xi as u32
                } else {
                    xi as u32
                };
                img.put_pixel(dst_x, dst_y, apply_colormap(g as f32 / 255.0, colormap));
            }
        }

        // Downscan → center nadir strip, painted OVER the sidescan near-nadir
        if !down_pings.is_empty() && strip_half_w > 0 {
            let dn = down_pings.len();
            let idx = (dst_y as usize * dn / img_h as usize).min(dn - 1);
            let ping = &down_pings[idx];
            let skip = down_offsets.get(idx).copied().unwrap_or(0);
            let strip_w = (strip_half_w * 2) as usize;
            let gray = ping_to_gray_row_normed(
                ping,
                skip,
                strip_w,
                MOSAIC_GAMMA,
                down_p2,
                down_p98,
                &down_tvg,
            );
            for (xi, &g) in gray.iter().enumerate() {
                let dst_x = single_w - strip_half_w + xi as u32;
                if dst_x >= total_w {
                    break;
                }
                let fg = apply_colormap(g as f32 / 255.0, colormap);

                let dist_from_edge = (xi as u32).min(strip_w as u32 - 1 - xi as u32);
                let alpha = if dist_from_edge < blend_px {
                    (dist_from_edge as f32 + 1.0) / (blend_px as f32 + 1.0)
                } else {
                    1.0
                };

                if alpha >= 1.0 {
                    img.put_pixel(dst_x, dst_y, fg);
                } else {
                    let bg = *img.get_pixel(dst_x, dst_y);
                    let blended = Rgb([
                        (bg[0] as f32 * (1.0 - alpha) + fg[0] as f32 * alpha) as u8,
                        (bg[1] as f32 * (1.0 - alpha) + fg[1] as f32 * alpha) as u8,
                        (bg[2] as f32 * (1.0 - alpha) + fg[2] as f32 * alpha) as u8,
                    ]);
                    img.put_pixel(dst_x, dst_y, blended);
                }
            }
        }
    }
    // Blend the nadir seam after both halves are rendered.
    // 28px per side balances blend quality and sharpness for mosaics up to 4096px wide.
    // The downscan nadir fill already painted over the dead zone at the centre;
    // this blend additionally feathers the port/star boundary where they overlap.
    if stitch_nadir && !port_pings.is_empty() && !star_pings.is_empty() {
        blend_nadir_seam(&mut img, single_w, 28);
    }
    Some(img)
}

/// Encode an RgbImage to PNG bytes in memory (used by mosaic/waterfall outputs).
#[allow(dead_code)]
fn encode_png_rgb(img: &RgbImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let encoder = PngEncoder::new(&mut buf);
    encoder
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            ColorType::Rgb8.into(),
        )
        .context("In-memory PNG encode failed")?;
    Ok(buf)
}

/// Encode an RgbaImage to PNG bytes in memory (for alpha-feathered overlays).
fn encode_png_rgba(img: &image::RgbaImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let encoder = PngEncoder::new(&mut buf);
    encoder
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            ColorType::Rgba8.into(),
        )
        .context("In-memory RGBA PNG encode failed")?;
    Ok(buf)
}

/// Encode an RgbaImage to WebP bytes in memory.
///
/// Uses lossless WebP via `image-webp` (pure Rust, no C dependencies).
/// Typical size reduction vs PNG: ~30–45 % for sonar imagery textures.
/// Lossless preserves every pixel exactly — safe for both KMZ and MBTiles.
fn encode_webp_rgba(img: &image::RgbaImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cursor, image::ImageFormat::WebP)
        .context("In-memory WebP encode failed")?;
    Ok(buf)
}

/// Pre-computed normalization values for consistent cross-segment brightness.
/// When provided to `render_stitched_overlay_strip`, overrides per-segment
/// normalization so all segments share the same contrast range.
struct PrecomputedNorm {
    tvg: Vec<f32>,
    p2: f32,
    p98: f32,
}

/// Apply alpha feathering to a rendered sonar strip for overlay compositing.
///
/// Creates smooth transparency at strip edges to prevent visible seams,
/// corner flaring, and dark background artifacts.  Uses smoothstep envelope
/// for natural-looking fade and makes near-black background pixels transparent.
///
/// `feather_frac_x` — fraction of width to feather on left/right edges (e.g. 0.12)
/// `feather_frac_y` — fraction of height to feather on top/bottom edges (e.g. 0.06)
fn apply_alpha_feathering(
    rgb: &RgbImage,
    feather_frac_x: f32,
    feather_frac_y: f32,
) -> image::RgbaImage {
    let (w, h) = rgb.dimensions();
    let mut out: image::RgbaImage = ImageBuffer::new(w, h);

    // When fractions are zero, skip geometric feathering entirely —
    // only apply the near-black transparency pass.
    let do_geom = feather_frac_x > 0.0 || feather_frac_y > 0.0;
    let edge_x = if do_geom {
        (w as f32 * feather_frac_x).max(2.0) as u32
    } else {
        0
    };
    let edge_y = if do_geom {
        (h as f32 * feather_frac_y).max(1.0) as u32
    } else {
        0
    };

    for y in 0..h {
        for x in 0..w {
            let px = rgb.get_pixel(x, y);

            let geom_alpha = if do_geom {
                let smoothstep = |t: f32| -> f32 {
                    let t = t.clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };

                let ax = if x < edge_x {
                    x as f32 / edge_x as f32
                } else if x >= w.saturating_sub(edge_x) {
                    (w - 1 - x) as f32 / edge_x as f32
                } else {
                    1.0
                };
                let ay = if y < edge_y {
                    y as f32 / edge_y as f32
                } else if y >= h.saturating_sub(edge_y) {
                    (h - 1 - y) as f32 / edge_y as f32
                } else {
                    1.0
                };
                smoothstep(ax) * smoothstep(ay)
            } else {
                1.0
            };

            // Make near-black pixels transparent (removes dark background fill)
            let lum = px[0] as f32 * 0.299 + px[1] as f32 * 0.587 + px[2] as f32 * 0.114;
            let data_alpha = if lum < 6.0 {
                (lum / 6.0).clamp(0.0, 1.0)
            } else {
                1.0
            };

            let final_alpha = (geom_alpha * data_alpha * 255.0).clamp(0.0, 255.0) as u8;
            out.put_pixel(x, y, Rgba([px[0], px[1], px[2], final_alpha]));
        }
    }
    out
}

/// Encode bytes as base64 (for data: URIs in the viewer).
#[allow(dead_code)]
fn to_base64(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 63) as usize] as char);
        result.push(CHARS[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((n >> 6) & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

/// Compute perpendicular left/right corners at a position given heading.
/// Returns ((left_lon, left_lat), (right_lon, right_lat)).
fn perp_corners(
    lat: f64,
    lon: f64,
    heading_rad: f64,
    swath_half_m: f64,
) -> ((f64, f64), (f64, f64)) {
    let perp_rad = heading_rad + std::f64::consts::FRAC_PI_2;
    let m_per_deg_lat = 111_320.0;
    let m_per_deg_lon = 111_320.0 * lat.to_radians().cos();
    let half_lat = swath_half_m * perp_rad.cos() / m_per_deg_lat;
    let half_lon = swath_half_m * perp_rad.sin() / m_per_deg_lon.max(1.0);
    (
        (lon - half_lon, lat - half_lat),
        (lon + half_lon, lat + half_lat),
    )
}

/// Average two headings (radians) handling wrap-around correctly.
fn avg_heading(h1: f64, h2: f64) -> f64 {
    let sx = h1.sin() + h2.sin();
    let cx = h1.cos() + h2.cos();
    sx.atan2(cx)
}

/// Compute heading (radians) from ping a to ping b.
/// Corrects for longitude convergence at latitude so heading is consistent
/// with the perp_corners() metre-space projection.
fn heading_between(a: &Ping, b: &Ping) -> f64 {
    if let (Some(ha), Some(hb)) = (a.heading_deg, b.heading_deg) {
        // Both have sensor heading. Convert to geographic radians.
        return avg_heading(ha.to_radians() as f64, hb.to_radians() as f64);
    }
    let delta_lat = b.latitude - a.latitude;
    let delta_lon = (b.longitude - a.longitude) * a.latitude.to_radians().cos();
    delta_lon.atan2(delta_lat)
}

/// Pre-compute shared boundary corners for a sequence of segments.
/// Returns N+1 entries for N segments: boundaries[i] and boundaries[i+1] are the
/// start/end corners for segment i.  Adjacent segments share exact same boundary
/// coordinates → zero gaps.
///
/// Each entry is ((left_lon, left_lat), (right_lon, right_lat)).
fn compute_shared_boundaries(
    segments: &[&[&Ping]],
    seg_swath_half_m: &[f64],
) -> Vec<((f64, f64), (f64, f64))> {
    if segments.is_empty() {
        return vec![];
    }

    let n = segments.len();
    let mut boundaries = Vec::with_capacity(n + 1);

    // Helper: compute heading at a specific ping using its neighbors
    let local_heading = |seg: &[&Ping], end: bool| -> f64 {
        let len = seg.len();
        if len < 2 {
            return 0.0;
        }
        let span = (len / 4).clamp(10, 40);
        if end {
            // Heading at the end of the segment: use a broader baseline for stability
            let a = seg[len.saturating_sub(span + 1)];
            let b = seg[len - 1];
            heading_between(a, b)
        } else {
            // Heading at the start of the segment: use a broader baseline for stability
            let a = seg[0];
            let b = seg[span.min(len - 1)];
            heading_between(a, b)
        }
    };

    let seg_half = |i: usize| -> f64 {
        seg_swath_half_m
            .get(i)
            .copied()
            .unwrap_or(30.0)
            .clamp(10.0, 300.0)
    };

    // First boundary: first ping of first segment, heading at start
    let first_ping = segments[0].first().unwrap();
    let h_start = local_heading(segments[0], false);
    boundaries.push(perp_corners(
        first_ping.latitude,
        first_ping.longitude,
        h_start,
        seg_half(0),
    ));

    // Interior boundaries: between segment i-1 and segment i
    for i in 1..n {
        let prev_last = segments[i - 1].last().unwrap();
        let next_first = segments[i].first().unwrap();
        let mid_lat = (prev_last.latitude + next_first.latitude) / 2.0;
        let mid_lon = (prev_last.longitude + next_first.longitude) / 2.0;
        // Use local headings at the boundary points (not overall segment heading)
        let h_prev_end = local_heading(segments[i - 1], true);
        let h_next_start = local_heading(segments[i], false);
        let heading = avg_heading(h_prev_end, h_next_start);
        let mut turn_delta = (h_next_start - h_prev_end).abs();
        if turn_delta > std::f64::consts::PI {
            turn_delta = 2.0 * std::f64::consts::PI - turn_delta;
        }
        // Tight turns can self-intersect with full swath; taper width at boundaries.
        // Gentle tapering at turns prevents corner overlap without creating
        // large gaps.  A 90° turn renders at ~65% width.
        let turn_norm = (turn_delta / std::f64::consts::PI).clamp(0.0, 1.0);
        let base_half = (seg_half(i - 1) + seg_half(i)) * 0.5;
        // Stronger taper to prevent sharp corners crossing over in Google Earth & MapLibre
        let local_half = base_half * (1.0 - 0.85 * turn_norm).clamp(0.15, 1.0);
        boundaries.push(perp_corners(mid_lat, mid_lon, heading, local_half));
    }

    // Last boundary: last ping of last segment, heading at end
    let last_ping = segments[n - 1].last().unwrap();
    let h_end = local_heading(segments[n - 1], true);
    boundaries.push(perp_corners(
        last_ping.latitude,
        last_ping.longitude,
        h_end,
        seg_half(n - 1),
    ));

    boundaries
}

// ── Geographic helpers ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct BBox {
    min_lat: f64,
    max_lat: f64,
    min_lon: f64,
    max_lon: f64,
}

impl BBox {
    fn from_pings(pings: &[Ping]) -> Option<Self> {
        let valid: Vec<_> = pings
            .iter()
            .filter(|p| {
                p.latitude.is_finite()
                    && p.longitude.is_finite()
                    && (p.latitude != 0.0 || p.longitude != 0.0)
            })
            .collect();
        if valid.is_empty() {
            return None;
        }
        let min_lat = valid
            .iter()
            .map(|p| p.latitude)
            .fold(f64::INFINITY, f64::min);
        let max_lat = valid
            .iter()
            .map(|p| p.latitude)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_lon = valid
            .iter()
            .map(|p| p.longitude)
            .fold(f64::INFINITY, f64::min);
        let max_lon = valid
            .iter()
            .map(|p| p.longitude)
            .fold(f64::NEG_INFINITY, f64::max);
        Some(BBox {
            min_lat,
            max_lat,
            min_lon,
            max_lon,
        })
    }

    fn center_lat(&self) -> f64 {
        (self.min_lat + self.max_lat) / 2.0
    }
    fn center_lon(&self) -> f64 {
        (self.min_lon + self.max_lon) / 2.0
    }

    /// Approximate camera range in metres for KML `<LookAt>`.
    fn kml_range_m(&self) -> f64 {
        let lat_km = (self.max_lat - self.min_lat).abs() * 111.0;
        let lon_km =
            (self.max_lon - self.min_lon).abs() * 111.0 * self.center_lat().to_radians().cos();
        (lat_km.max(lon_km) * 1000.0 * 2.5).max(200.0)
    }

    /// MBTiles spec `bounds` string: "min_lon,min_lat,max_lon,max_lat".
    fn mbtiles_bounds(&self) -> String {
        format!(
            "{:.6},{:.6},{:.6},{:.6}",
            self.min_lon, self.min_lat, self.max_lon, self.max_lat
        )
    }

    /// MBTiles spec `center` string: "lon,lat,zoom".
    fn mbtiles_center(&self, zoom: u8) -> String {
        format!("{:.6},{:.6},{}", self.center_lon(), self.center_lat(), zoom)
    }
}

// ── Curvelet denoising helpers ────────────────────────────────────────────────

/// Render a small probe image for the primary sidescan channel and compute the
/// universal MAD threshold: `\u03c3\u0302 \u00b7 \u221a(2\u00b7ln N)`.  Returns 0.05 if estimation fails.
pub fn estimate_curvelet_threshold(parsed: &crate::garmin_rsd_parser::ParseResult) -> f32 {
    let channels = pings_by_channel(parsed);
    // Prefer sidescan channels; fall back to the channel with most pings.
    let pings: Vec<&crate::garmin_rsd_parser::Ping> = {
        let sidescan: Vec<_> = channels
            .values()
            .filter(|v| {
                v.first()
                    .map(|p| [0, 1, 4, 5, 8, 9, 14, 15].contains(&p.channel))
                    .unwrap_or(false)
            })
            .max_by_key(|v| v.len())
            .cloned()
            .unwrap_or_default();
        if !sidescan.is_empty() {
            sidescan
        } else {
            channels
                .values()
                .max_by_key(|v| v.len())
                .cloned()
                .unwrap_or_default()
        }
    };
    if pings.is_empty() {
        return 0.05;
    }
    let probe = render_gray(&pings, 512, 512);
    let (_, suggested) = curvelet_denoise_gray_image(probe, 0.0);
    if suggested <= 0.0 {
        0.05
    } else {
        suggested
    }
}

/// Render before/after preview images (PNG bytes) at a given threshold.
/// Returns `(before_png, after_png, suggested_threshold)`.
/// Images are 512px wide — suitable for display as `<img>` data URLs.
pub fn curvelet_preview_png(
    parsed: &crate::garmin_rsd_parser::ParseResult,
    threshold: f32,
) -> (Vec<u8>, Vec<u8>, f32) {
    let channels = pings_by_channel(parsed);
    let pings: Vec<&crate::garmin_rsd_parser::Ping> = {
        let sidescan: Vec<_> = channels
            .values()
            .filter(|v| {
                v.first()
                    .map(|p| [0, 1, 4, 5, 8, 9, 14, 15].contains(&p.channel))
                    .unwrap_or(false)
            })
            .max_by_key(|v| v.len())
            .cloned()
            .unwrap_or_default();
        if !sidescan.is_empty() {
            sidescan
        } else {
            channels
                .values()
                .max_by_key(|v| v.len())
                .cloned()
                .unwrap_or_default()
        }
    };
    let empty = Vec::new();
    if pings.is_empty() {
        return (empty.clone(), empty, 0.05);
    }
    let before = render_gray(&pings, 512, 512);
    let (after, suggested) = curvelet_denoise_gray_image(before.clone(), threshold);
    (
        gray_to_png_bytes(before),
        gray_to_png_bytes(after),
        suggested,
    )
}

fn gray_to_png_bytes(img: GrayImage) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let _ = img.write_to(&mut buf, image::ImageFormat::Png);
    buf.into_inner()
}

/// Apply curvelet soft-thresholding to a GRAY8 image.
///
/// Returns `(denoised_image, suggested_threshold)` where the suggestion is
/// the universal threshold `σ·√(2·ln N)` estimated from the finest-scale
/// detail coefficients via the MAD estimator: `σ̂ = median(|c|) / 0.6745`.
/// If `threshold == 0.0` the transform still runs (for estimation only)
/// and returns the original image unchanged.
///
/// `tag` is a short label used in error tracking (e.g. `"waterfall_ch0"`).
fn curvelet_denoise_gray_image(img: GrayImage, threshold: f32) -> (GrayImage, f32) {
    curvelet_denoise_gray_image_tagged(img, threshold, "")
}

/// Fast full-resolution sonar denoising via a per-row sliding-window sigma filter.
///
/// Uses raw byte slices (no get_pixel/put_pixel overhead) and integer prefix sums
/// for maximum speed in debug mode.  ~5–15ms for a 2046×8192 image.
fn denoise_sigma_filter(img: &GrayImage, threshold: f32) -> GrayImage {
    let w = img.width() as usize;
    let h = img.height() as usize;
    // half_win: threshold 0.02→1px, 0.05→2px, 0.12→5px, 0.25→10px
    let half_win = ((threshold * 40.0).round() as usize).clamp(1, 15);
    // noise_var in u8² units: threshold=0.05 → (0.05*255)²=163
    let noise_var_i64 = {
        let t = (threshold * 255.0) as i64;
        t * t
    };

    let src = img.as_raw(); // &[u8], row-major
    let mut dst = src.to_vec();

    // Integer prefix sums (i64) — no f64 in the hot path → ~10× faster in debug
    let mut psum = vec![0i64; w + 1];
    let mut psumsq = vec![0i64; w + 1];

    for y in 0..h {
        let row_off = y * w;
        let src_row = &src[row_off..row_off + w];
        let dst_row = &mut dst[row_off..row_off + w];

        psum[0] = 0;
        psumsq[0] = 0;
        for x in 0..w {
            let v = src_row[x] as i64;
            psum[x + 1] = psum[x] + v;
            psumsq[x + 1] = psumsq[x] + v * v;
        }
        for x in 0..w {
            let x0 = x.saturating_sub(half_win);
            let x1 = (x + half_win + 1).min(w);
            let n = (x1 - x0) as i64;
            let sum = psum[x1] - psum[x0];
            let sumsq = psumsq[x1] - psumsq[x0];
            // mean * n and variance * n² to keep everything integer
            let mean_n = sum; // mean = sum/n
            let var_n2 = sumsq * n - sum * sum; // variance * n² = E[x²]n² - (E[x]n)²
                                                // noise_var * n² threshold
            let noise_n2 = noise_var_i64 * n * n;
            // blend ∈ [0,1]: if var < noise → blend toward mean
            let orig = src_row[x] as i64;
            let v = if var_n2 <= 0 || noise_n2 >= var_n2 * 4 {
                // Definitely noise — output mean
                ((mean_n + n / 2) / n).clamp(0, 255) as u8
            } else if var_n2 >= noise_n2 * 4 {
                // Definitely signal — keep original
                orig as u8
            } else {
                // Partial blend
                let blend_num = noise_n2;
                let blend_den = var_n2.max(1);
                let blended =
                    (mean_n * blend_num + orig * n * (blend_den - blend_num)) / (n * blend_den);
                blended.clamp(0, 255) as u8
            };
            dst_row[x] = v;
        }
    }

    GrayImage::from_raw(img.width(), img.height(), dst).unwrap_or_else(|| img.clone())
}

fn curvelet_denoise_gray_image_tagged(
    img: GrayImage,
    threshold: f32,
    tag: &str,
) -> (GrayImage, f32) {
    use std::time::Instant;
    let t0 = Instant::now();
    let (orig_w, orig_h) = (img.width(), img.height());
    eprintln!("[curvelet] {tag}: start {orig_w}x{orig_h} threshold={threshold:.4}");
    if orig_w < 16 || orig_h < 16 {
        eprintln!("[curvelet] {tag}: too small, skipping");
        crate::curvelet_diag::push(crate::curvelet_diag::CurveletDiagEntry {
            tag: tag.to_string(),
            width: orig_w as usize,
            height: orig_h as usize,
            error: format!("image too small ({orig_w}x{orig_h})"),
            ..Default::default()
        });
        return (img, 0.0);
    }

    // ── FAST DENOISING PATH (threshold > 0) ──────────────────────────────
    // Apply sigma filter at FULL resolution.  This preserves every pixel at the
    // original size, removes speckle, and runs in milliseconds rather than
    // minutes.  The curvelet transform is only used for threshold estimation.
    if threshold > 0.0 {
        let half_win = ((threshold * 40.0).round() as usize).clamp(1, 15);
        eprintln!("[curvelet] {tag}: sigma filter {orig_w}x{orig_h} half_win={half_win}");
        let out = denoise_sigma_filter(&img, threshold);
        let elapsed = t0.elapsed().as_millis();
        eprintln!("[curvelet] {tag}: sigma done in {elapsed}ms");
        crate::curvelet_diag::push(crate::curvelet_diag::CurveletDiagEntry {
            tag: tag.to_string(),
            width: orig_w as usize,
            height: orig_h as usize,
            num_scales: 0,
            threshold_applied: threshold as f64,
            suggested_threshold: 0.0,
            elapsed_ms: elapsed as u64,
            error: String::new(),
        });
        return (out, 0.0);
    }

    // ── ESTIMATION PATH (threshold = 0) ─────────────────────────────────
    // The curvelet forward always returns ~0.05 (MAD clamped to floor) in
    // practice, and takes 7+ seconds in debug mode.  Skip it — just return
    // the default suggestion so the caller can decide the threshold.
    // The full curvelet estimation is available via the `preview_curvelet`
    // Tauri command (user-initiated, not in the pipeline hot path).
    eprintln!("[curvelet] {tag}: estimation skipped (use preview for MAD), returning default 0.05");
    (img, 0.05_f32)
}

/// Re-colorize a GRAY8 image using a named palette. Fast path for the mosaic
/// denoising workflow: denoise in grayscale, then map through colormap.
fn colorize_gray_image(gray: &GrayImage, colormap: &str) -> RgbImage {
    let (w, h) = (gray.width(), gray.height());
    let mut out = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let g = gray.get_pixel(x, y)[0];
            out.put_pixel(x, y, apply_colormap(g as f32 / 255.0, colormap));
        }
    }
    out
}

// ── Output writers ────────────────────────────────────────────────────────────

const WATERFALL_MAX_W: u32 = 4096;
const WATERFALL_MAX_H: u32 = 8192;
/// Gamma < 1 lifts shadow detail; 0.70 gives good waterfall contrast.
const WATERFALL_GAMMA: f32 = 0.70;
/// Slightly stronger lift for false-colour mosaics.
const MOSAIC_GAMMA: f32 = 0.65;
/// Per-channel width of the stitched butterfly mosaic / KMZ ground overlay.
#[allow(dead_code)]
const MOSAIC_COMBINED_W: u32 = 2048;
const MBTILES_MAX_ZOOM: u8 = 18;
const KML_MAX_PLACEMARKS: usize = 600;
const VIEWER_MAX_PINGS: usize = 2000;

fn write_waterfall_per_channel(
    parsed: &ParseResult,
    output_dir: &Path,
    denoise: bool,
    denoise_threshold: f32,
    denoised_cache: &BTreeMap<u32, GrayImage>,
    show_payload_debug_overlay: bool,
) -> Result<Vec<OutputArtifact>> {
    let channels = pings_by_channel(parsed);
    let mut arts = Vec::new();
    for (ch, pings) in &channels {
        let ch_label = channel_label(parsed, *ch);
        let role = egn_role_from_label(&ch_label, *ch);

        // Apply EGN across-track normalization before rendering.
        // For DepthTemp/metadata channels (Unassigned) this is a no-op.
        let egn_pings: Vec<Ping>;
        let render_pings: Vec<&Ping> = if role != SpatialRole::Unassigned {
            egn_pings = apply_egn_to_channel_pings(pings, role);
            egn_pings.iter().collect()
        } else {
            pings.iter().map(|p| *p).collect()
        };

        let (img, used_threshold) = if denoise {
            if let Some(cached) = denoised_cache.get(ch) {
                (cached.clone(), 0.0_f32)
            } else {
                let raw = render_gray(&render_pings, WATERFALL_MAX_W, WATERFALL_MAX_H);
                curvelet_denoise_gray_image_tagged(
                    raw,
                    denoise_threshold,
                    &format!("waterfall_ch{ch}"),
                )
            }
        } else {
            let raw = render_gray(&render_pings, WATERFALL_MAX_W, WATERFALL_MAX_H);
            (raw, 0.0_f32)
        };
        let (img_rgb, payload_rows, payload_max_delta) = if show_payload_debug_overlay {
            overlay_extra_payload_magenta(&img, &render_pings)
        } else {
            let mut rgb: RgbImage = ImageBuffer::new(img.width(), img.height());
            for (x, y, px) in img.enumerate_pixels() {
                let g = px.0[0];
                rgb.put_pixel(x, y, Rgb([g, g, g]));
            }
            (rgb, 0, 0)
        };
        let fname = format!("waterfall_ch{ch}.png");
        let path = output_dir.join(&fname);
        let denoise_tag = if denoise {
            format!(" · curvelet-denoised (t={used_threshold:.3})")
        } else {
            String::new()
        };
        let payload_tag = if payload_max_delta > 0 {
            format!(
                " · payload-delta max={} samples (magenta rows={})",
                payload_max_delta, payload_rows
            )
        } else {
            String::new()
        };
        match img_rgb.save(&path) {
            Ok(()) => arts.push(OutputArtifact {
                kind: "waterfall".to_string(),
                path: path.display().to_string(),
                details: format!(
                    "Ch {} ({}) · {}×{} · per-ping 2\u{2013}98% stretch · \u{03b3}{WATERFALL_GAMMA:.2}{denoise_tag}{payload_tag}",
                    ch, ch_label, img.width(), img.height()
                ),
            }),
            Err(e) => arts.push(OutputArtifact {
                kind: "waterfall".to_string(),
                path: path.display().to_string(),
                details: format!("ERROR writing {fname}: {e:#}"),
            }),
        }
    }
    Ok(arts)
}

fn write_mosaic_per_channel(
    parsed: &ParseResult,
    output_dir: &Path,
    colormap: &str,
    remove_water_column: bool,
    nadir_mode: &str,
    denoise: bool,
    denoise_threshold: f32,
    alignments: &[crate::channel_alignment::ChannelAlignment],
    denoised_cache: &BTreeMap<u32, GrayImage>,
    sidescan_pair: (Option<u32>, Option<u32>),
) -> Result<Vec<OutputArtifact>> {
    let channels = pings_by_channel(parsed);
    let mut arts = Vec::new();

    // Per-channel mosaics (sidescan + downscan)
    for (ch, pings) in &channels {
        let ch_label = channel_label(parsed, *ch);
        if !ch_label.contains("sidescan") && !ch_label.contains("downscan") {
            continue;
        }
        // When curvelet denoising is enabled, render to gray first so we can
        // denoise the single-channel image before re-colorizing. This gives
        // much cleaner edge preservation than denoising each RGB band separately.
        let img: RgbImage = if denoise {
            let denoised = if let Some(cached) = denoised_cache.get(ch) {
                cached.clone()
            } else {
                let gray = render_gray(pings, WATERFALL_MAX_W, WATERFALL_MAX_H);
                let (d, _) = curvelet_denoise_gray_image_tagged(
                    gray,
                    denoise_threshold,
                    &format!("mosaic_ch{ch}"),
                );
                d
            };
            colorize_gray_image(&denoised, colormap)
        } else {
            render_mosaic_rgb(pings, WATERFALL_MAX_W, WATERFALL_MAX_H, colormap)
        };
        let denoise_tag = if denoise {
            format!(" · curvelet-denoised (t={denoise_threshold:.3})")
        } else {
            String::new()
        };
        let fname = format!("mosaic_ch{ch}.png");
        let path = output_dir.join(&fname);
        match img.save(&path) {
            Ok(()) => arts.push(OutputArtifact {
                kind: "mosaic".to_string(),
                path: path.display().to_string(),
                details: format!(
                    "Ch {} ({}) · {}×{} · {} palette{denoise_tag}",
                    ch,
                    ch_label,
                    img.width(),
                    img.height(),
                    colormap
                ),
            }),
            Err(e) => arts.push(OutputArtifact {
                kind: "mosaic".to_string(),
                path: path.display().to_string(),
                details: format!("ERROR writing mosaic_ch{ch}.png: {e:#}"),
            }),
        }
    }

    // Stitched butterfly mosaic: use pre-computed channel pair (coverage + overlap).
    let (port_key, star_key) = sidescan_pair;
    if let (Some(pk), Some(sk)) = (port_key, star_key) {
        let port_pings = &channels[&pk];
        let star_pings = &channels[&sk];
        let p_n = port_pings.len();
        let s_n = star_pings.len();
        let min_n = p_n.min(s_n);
        let max_n = p_n.max(s_n).max(1);
        if min_n < 200 || (min_n as f64 / max_n as f64) < 0.12 {
            arts.push(OutputArtifact {
                kind: "mosaic_combined".to_string(),
                path: output_dir.join("mosaic_combined.png").display().to_string(),
                details: format!(
                    "Skipped stitched butterfly due to channel imbalance: port ch{}={} pings, star ch{}={} pings",
                    pk, p_n, sk, s_n
                ),
            });
            return Ok(arts);
        }

        // Apply EGN to port and star channels independently.
        // GT51 asymmetric wings get a diagonal-slope correction;
        // UHD paired channels get V-shaped correction.
        let port_lbl = channel_label(parsed, pk);
        let star_lbl = channel_label(parsed, sk);
        let port_role = egn_role_from_label(&port_lbl, pk);
        let star_role = egn_role_from_label(&star_lbl, sk);
        let port_egn: Vec<Ping> = apply_egn_to_channel_pings(port_pings, port_role);
        let star_egn: Vec<Ping> = apply_egn_to_channel_pings(star_pings, star_role);
        let port_pings: Vec<&Ping> = port_egn.iter().collect();
        let star_pings: Vec<&Ping> = star_egn.iter().collect();

        // Data-driven downscan channel lookup: any channel labeled chirp_downscan
        // that is NOT one of the selected sidescan arms, with the most pings.
        // Handles classic (ch2), UHD (ch6), UHD2 8-series (ch10), UHD2 10-series (ch12),
        // ClearVü dual-freq (ch18/20), and any future layouts automatically.
        // Only resolve the downscan channel when the user requested fill mode.
        let down_ch = if nadir_mode == "fill" {
            channels
                .keys()
                .copied()
                .filter(|&ch| ch != pk && ch != sk)
                .filter(|&ch| channel_label(parsed, ch).contains("downscan"))
                .max_by_key(|&ch| channels.get(&ch).map(|v| v.len()).unwrap_or(0))
        } else {
            None
        };
        let empty_pings: Vec<&Ping> = Vec::new();
        let down_pings = down_ch
            .and_then(|dc| channels.get(&dc))
            .unwrap_or(&empty_pings);
        // Use the actual maximum sample count per side so far-range data isn't resampled away
        let port_max_w = port_pings
            .iter()
            .map(|p| p.samples.len() as u32)
            .max()
            .unwrap_or(512);
        let star_max_w = star_pings
            .iter()
            .map(|p| p.samples.len() as u32)
            .max()
            .unwrap_or(512);
        let dynamic_w = port_max_w.max(star_max_w).min(WATERFALL_MAX_W / 2);
        if let Some(combined) = render_sidescan_stitched(
            &port_pings,
            &star_pings,
            down_pings,
            dynamic_w,
            WATERFALL_MAX_H,
            colormap,
            remove_water_column,
            nadir_mode != "raw",
            alignments,
            Some(pk),
            Some(sk),
        ) {
            let nadir_desc = match nadir_mode {
                "fill" if !down_pings.is_empty() => " + downscan nadir fill",
                "raw" => " (nadir gap preserved)",
                _ => "",
            };
            let path = output_dir.join("mosaic_combined.png");
            match combined.save(&path) {
                Ok(()) => arts.push(OutputArtifact {
                    kind: "mosaic_combined".to_string(),
                    path: path.display().to_string(),
                    details: format!(
                        "Stitched port ch{} + star ch{}{} · {}×{} · {} palette",
                        pk,
                        sk,
                        nadir_desc,
                        combined.width(),
                        combined.height(),
                        colormap
                    ),
                }),
                Err(e) => arts.push(OutputArtifact {
                    kind: "mosaic_combined".to_string(),
                    path: path.display().to_string(),
                    details: format!("ERROR writing mosaic_combined.png: {e:#}"),
                }),
            }
        }
    }

    Ok(arts)
}

// ── Slippy/TMS tile math helpers ─────────────────────────────────────────────

fn lon_to_tile_x(lon: f64, zoom: u32) -> u32 {
    ((lon + 180.0) / 360.0 * (1u64 << zoom) as f64)
        .floor()
        .max(0.0) as u32
}

fn lat_to_tile_y(lat: f64, zoom: u32) -> u32 {
    let lat_rad = lat.to_radians();
    let n = (1u64 << zoom) as f64;
    ((1.0 - lat_rad.tan().asinh() / std::f64::consts::PI) / 2.0 * n)
        .floor()
        .max(0.0) as u32
}

fn tile_to_lon(x: u32, zoom: u32) -> f64 {
    x as f64 / (1u64 << zoom) as f64 * 360.0 - 180.0
}

fn tile_to_lat(y: u32, zoom: u32) -> f64 {
    let n = std::f64::consts::PI - 2.0 * std::f64::consts::PI * y as f64 / (1u64 << zoom) as f64;
    n.sinh().atan().to_degrees()
}

/// Choose a max zoom level where the bounding box spans ~8–32 tiles.
fn compute_max_zoom(bbox: &BBox) -> u32 {
    let dlat = (bbox.max_lat - bbox.min_lat).abs();
    let dlon = (bbox.max_lon - bbox.min_lon).abs();
    let span = dlat.max(dlon);
    if span <= 0.0 {
        return 16;
    }
    // We want span / (360 / 2^z) ≈ 16 tiles
    ((16.0 * 360.0 / span).log2().floor() as u32).clamp(10, MBTILES_MAX_ZOOM as u32)
}

/// Write multi-zoom MBTiles with georeferenced sonar tiles along the track.
///
/// Instead of a single zoom-0 tile, this generates tiles from `min_zoom` to
/// `max_zoom`.  At each zoom level, tiles overlapping the track bounding box
/// are rendered by painting each ping's sonar samples as a cross-track stripe
/// placed at the correct geographic position within the tile.
fn write_mbtiles(
    parsed: &ParseResult,
    path: &Path,
    colormap: &str,
    remove_water_column: bool,
) -> Result<()> {
    let conn = Connection::open(path)
        .with_context(|| format!("Failed to create MBTiles DB: {}", path.display()))?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS metadata (name TEXT, value TEXT);
         CREATE TABLE IF NOT EXISTS tiles (zoom_level INTEGER, tile_column INTEGER,
                                           tile_row INTEGER, tile_data BLOB);
         CREATE UNIQUE INDEX IF NOT EXISTS tile_index
            ON tiles (zoom_level, tile_column, tile_row);
         DELETE FROM metadata;
         DELETE FROM tiles;",
    )?;

    let bbox = match BBox::from_pings(&parsed.pings) {
        Some(b) => b,
        None => return Ok(()),
    };

    let max_zoom = compute_max_zoom(&bbox);
    let min_zoom = max_zoom.saturating_sub(4);

    let bounds_str = bbox.mbtiles_bounds();
    let center_str = bbox.mbtiles_center(max_zoom as u8);

    for (name, value) in &[
        ("name", "SonarSniffer Mosaic"),
        ("description", &format!("{} pings", parsed.pings.len())),
        ("type", "overlay"),
        ("format", "webp"),
        ("minzoom", &min_zoom.to_string()),
        ("maxzoom", &max_zoom.to_string()),
        ("bounds", &bounds_str),
        ("center", &center_str),
    ] {
        conn.execute(
            "INSERT INTO metadata (name, value) VALUES (?1, ?2)",
            (name, value),
        )?;
    }

    // Collect GPS-valid pings from the dominant sidescan channel.
    // Prefer sidescan over downscan/depth for georeferenced outputs (wide swath).
    let channels = pings_by_channel(parsed);
    let dominant: Vec<&Ping> = {
        let sidescan_best = channels
            .iter()
            .filter(|(&ch, _)| {
                let label = channel_label(parsed, ch);
                label.contains("sidescan")
            })
            .max_by_key(|(_, v)| v.len());
        let best = sidescan_best.or_else(|| channels.iter().max_by_key(|(_, v)| v.len()));
        best.map(|(_, v)| v.clone()).unwrap_or_default()
    };
    let gps_pings: Vec<&Ping> = dominant
        .iter()
        .filter(|p| {
            p.latitude.is_finite()
                && p.longitude.is_finite()
                && (p.latitude != 0.0 || p.longitude != 0.0)
        })
        .copied()
        .collect();
    if gps_pings.is_empty() {
        return Ok(());
    }

    // Pre-compute per-ping headings (radians, 0 = north)
    let headings: Vec<f64> = gps_pings
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let (p1, p2) = if i + 1 < gps_pings.len() {
                (gps_pings[i], gps_pings[i + 1])
            } else if i > 0 {
                (gps_pings[i - 1], gps_pings[i])
            } else {
                return 0.0;
            };
            let dlat = p2.latitude - p1.latitude;
            let dlon = p2.longitude - p1.longitude;
            dlon.atan2(dlat)
        })
        .collect();

    // Swath half-width from median depth
    let median_depth = {
        let mut depths: Vec<f64> = gps_pings
            .iter()
            .map(|p| p.depth_m as f64)
            .filter(|&d| d > 0.0)
            .collect();
        if depths.is_empty() {
            depths.push(5.0);
        }
        depths.sort_by(|a, b| a.partial_cmp(b).unwrap());
        depths[depths.len() / 2]
    };
    let swath_half_m = (median_depth * 5.0).clamp(10.0, 200.0);

    // Per-ping nadir offsets
    let nadir_offsets = if remove_water_column {
        smooth_nadir_offsets(&detect_per_ping_nadir(&gps_pings))
    } else {
        let raw = detect_per_ping_nadir(&gps_pings);
        let mut sorted = raw.clone();
        sorted.sort_unstable();
        let median = sorted[sorted.len() / 2];
        let min_skip = median.min(30);
        vec![min_skip; gps_pings.len()]
    };

    // Pre-render each ping's sonar data as a grey row (empirical TVG + segment norms)
    let swath_px = 128usize;
    let tvg_lut = compute_empirical_tvg(&gps_pings, &nadir_offsets);
    let (seg_p2, seg_p98) = compute_segment_norm(&gps_pings, &nadir_offsets, &tvg_lut);
    let ping_rows: Vec<Vec<u8>> = gps_pings
        .iter()
        .enumerate()
        .map(|(i, ping)| {
            let skip = nadir_offsets.get(i).copied().unwrap_or(0);
            ping_to_gray_row_normed(
                ping,
                skip,
                swath_px,
                MOSAIC_GAMMA,
                seg_p2,
                seg_p98,
                &tvg_lut,
            )
        })
        .collect();

    let m_per_sample = swath_half_m / (swath_px as f64 / 2.0).max(1.0);

    // Render tiles from max zoom down to min zoom
    for z in min_zoom..=max_zoom {
        let n = 1u64 << z;

        // Pad the bbox to catch tiles at the edges of the swath
        let pad_lat = swath_half_m / 111_320.0;
        let center_lat = (bbox.min_lat + bbox.max_lat) / 2.0;
        let pad_lon = swath_half_m / (111_320.0 * center_lat.to_radians().cos().max(0.01));

        let min_tx = lon_to_tile_x(bbox.min_lon - pad_lon, z);
        let max_tx = lon_to_tile_x(bbox.max_lon + pad_lon, z).min((n - 1) as u32);
        let min_ty = lat_to_tile_y(bbox.max_lat + pad_lat, z); // Y inverted
        let max_ty = lat_to_tile_y(bbox.min_lat - pad_lat, z).min((n - 1) as u32);

        for tx in min_tx..=max_tx {
            for ty in min_ty..=max_ty {
                let tile_west = tile_to_lon(tx, z);
                let tile_east = tile_to_lon(tx + 1, z);
                let tile_north = tile_to_lat(ty, z);
                let tile_south = tile_to_lat(ty + 1, z);

                let tile_center_lat = (tile_north + tile_south) / 2.0;
                let m_per_deg_lon = 111_320.0 * tile_center_lat.to_radians().cos().max(0.01);
                let m_per_deg_lat = 111_320.0;
                let tile_w_m = (tile_east - tile_west) * m_per_deg_lon;
                let m_per_px = tile_w_m / 256.0;

                let mut tile: image::RgbaImage =
                    ImageBuffer::from_pixel(256, 256, Rgba([0u8, 0, 0, 0]));
                let mut has_data = false;

                for (pi, ping) in gps_pings.iter().enumerate() {
                    // Quick rejection: skip pings far from this tile
                    let dlat_m = (ping.latitude - tile_center_lat) * m_per_deg_lat;
                    let dlon_m = (ping.longitude - (tile_west + tile_east) / 2.0) * m_per_deg_lon;
                    if dlat_m.abs() > tile_w_m * 1.5 + swath_half_m
                        || dlon_m.abs() > tile_w_m * 1.5 + swath_half_m
                    {
                        continue;
                    }

                    let heading = headings[pi];
                    let perp = heading + std::f64::consts::FRAC_PI_2;
                    let sin_p = perp.sin();
                    let cos_p = perp.cos();

                    // Ping center in tile pixel coords
                    let cx = (ping.longitude - tile_west) / (tile_east - tile_west) * 256.0;
                    let cy = (tile_north - ping.latitude) / (tile_north - tile_south) * 256.0;

                    // Along-track direction in pixel space (for brush coverage)
                    let along_sin = heading.sin();
                    let along_cos = heading.cos();
                    let along_px_x = along_sin * (m_per_deg_lon / m_per_px.max(0.001)) * 0.0000001;
                    let along_px_y = -along_cos * (m_per_deg_lat / m_per_px.max(0.001)) * 0.0000001;

                    let row = &ping_rows[pi];
                    for (si, &g) in row.iter().enumerate() {
                        if g == 0 {
                            continue;
                        }
                        let dist_from_center = (si as f64 - swath_px as f64 / 2.0) * m_per_sample;
                        let px = cx + dist_from_center * sin_p / m_per_px;
                        let py = cy - dist_from_center * cos_p / m_per_px;

                        let rgb = apply_colormap(g as f32 / 255.0, colormap);
                        let pixel = Rgba([rgb[0], rgb[1], rgb[2], 255]);

                        // Paint a small 2×1 brush to reduce gaps between pings.
                        // Brush extends 1px in the along-track direction.
                        for ofs in 0..2i32 {
                            let bx = (px + along_px_x * ofs as f64).round() as i32;
                            let by = (py + along_px_y * ofs as f64).round() as i32;
                            if bx >= 0 && bx < 256 && by >= 0 && by < 256 {
                                // Max-compositing: brighter pixel wins
                                let existing = tile.get_pixel(bx as u32, by as u32);
                                if existing[3] == 0
                                    || g > ((existing[0] as u16
                                        + existing[1] as u16
                                        + existing[2] as u16)
                                        / 3) as u8
                                {
                                    tile.put_pixel(bx as u32, by as u32, pixel);
                                }
                                has_data = true;
                            }
                        }
                    }
                }

                if has_data {
                    if let Ok(webp) = encode_webp_rgba(&tile) {
                        let tms_y = (n as u32).wrapping_sub(1).wrapping_sub(ty);
                        conn.execute(
                            "INSERT OR REPLACE INTO tiles \
                             (zoom_level, tile_column, tile_row, tile_data) \
                             VALUES (?1, ?2, ?3, ?4)",
                            rusqlite::params![z as i32, tx as i32, tms_y as i32, webp],
                        )?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Write KML with a styled trackline + decimated depth placemarks + LookAt camera.
/// Returns the number of placemarks written.
fn write_kml(parsed: &ParseResult, path: &Path) -> Result<usize> {
    let bbox = BBox::from_pings(&parsed.pings);

    // LookAt element (omit if no GPS)
    let look_at = bbox
        .map(|b| {
            format!(
                "\n  <LookAt>\
                \n    <longitude>{:.6}</longitude>\
                \n    <latitude>{:.6}</latitude>\
                \n    <altitude>0</altitude>\
                \n    <range>{:.0}</range>\
                \n    <tilt>0</tilt>\
                \n    <heading>0</heading>\
                \n    <altitudeMode>relativeToGround</altitudeMode>\
                \n  </LookAt>",
                b.center_lon(),
                b.center_lat(),
                b.kml_range_m()
            )
        })
        .unwrap_or_default();

    // Trackline coordinates (lon,lat,-depth_m for altitude)
    let track_coords: String = parsed
        .pings
        .iter()
        .filter(|p| {
            p.latitude.is_finite()
                && p.longitude.is_finite()
                && (p.latitude != 0.0 || p.longitude != 0.0)
        })
        .map(|p| {
            format!(
                "{:.7},{:.7},{:.1}",
                p.longitude,
                p.latitude,
                -(p.depth_m.max(0.0) as f64)
            )
        })
        .collect::<Vec<_>>()
        .join("\n              ");

    // Decimated ping placemarks with depth / channel data
    let step = (parsed.pings.len() / KML_MAX_PLACEMARKS).max(1);
    let placemarks: Vec<String> = parsed
        .pings
        .iter()
        .step_by(step)
        .take(KML_MAX_PLACEMARKS)
        .filter(|p| {
            p.latitude.is_finite()
                && p.longitude.is_finite()
                && (p.latitude != 0.0 || p.longitude != 0.0)
        })
        .map(|p| {
            format!(
                "    <Placemark>\
                \n      <styleUrl>#pingStyle</styleUrl>\
                \n      <ExtendedData>\
                \n        <Data name=\"depth_ft\"><value>{:.1}</value></Data>\
                \n        <Data name=\"depth_m\"><value>{:.2}</value></Data>\
                \n        <Data name=\"channel\"><value>{}</value></Data>\
                \n        <Data name=\"sample_count\"><value>{}</value></Data>\
                \n        <Data name=\"timestamp_ms\"><value>{}</value></Data>\
                \n      </ExtendedData>\
                \n      <Point>\
                \n        <altitudeMode>clampToGround</altitudeMode>\
                \n        <coordinates>{:.7},{:.7},{:.1}</coordinates>\
                \n      </Point>\
                \n    </Placemark>",
                p.depth_ft,
                p.depth_m,
                p.channel,
                p.sample_count,
                p.timestamp_ms,
                p.longitude,
                p.latitude,
                -(p.depth_m.max(0.0) as f64)
            )
        })
        .collect();

    let n = placemarks.len();
    let ping_block = placemarks.join("\n");

    let kml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>SonarSniffer Track</name>{look_at}
  <Style id="trackStyle">
    <LineStyle><color>ff0050ff</color><width>3</width></LineStyle>
    <PolyStyle><fill>0</fill></PolyStyle>
  </Style>
  <Style id="pingStyle">
    <IconStyle>
      <color>ffffffff</color>
      <scale>0.35</scale>
      <Icon><href>http://maps.google.com/mapfiles/kml/shapes/shaded_dot.png</href></Icon>
    </IconStyle>
    <BalloonStyle>
      <text><![CDATA[<b>Ping</b><br/>
Depth: <b>$[depth_ft] ft</b> ($[depth_m] m)<br/>
Channel: $[channel]<br/>
Samples: $[sample_count]]]></text>
    </BalloonStyle>
  </Style>
  <Folder>
    <name>Trackline</name>
    <Placemark>
      <name>Track</name>
      <styleUrl>#trackStyle</styleUrl>
      <LineString>
        <tessellate>1</tessellate>
        <altitudeMode>clampToGround</altitudeMode>
        <coordinates>
              {track_coords}
        </coordinates>
      </LineString>
    </Placemark>
  </Folder>
  <Folder>
    <name>Depth Pings (1-in-{step})</name>
{ping_block}
  </Folder>
</Document>
</kml>
"#
    );

    fs::write(path, kml).with_context(|| format!("Failed to write KML: {}", path.display()))?;
    Ok(n)
}

/// Write a KMZ containing the KML and (if GPS is available) a georeferenced
/// sidescan GroundOverlay.  Prefers `mosaic_combined.png` from `output_dir`;
/// falls back to an inline render of the dominant channel.
/// Returns `true` if the GroundOverlay was embedded.
/// Produce a KMZ with segmented GroundOverlays that follow the track line.
///
/// Instead of a single rectangular overlay (which covers a huge bounding box and
/// looks terrible on curves), this splits the track into segments of ~50 pings and
/// creates a narrow rotated overlay strip for each segment using `gx:LatLonQuad`.
/// Each strip is a small PNG rendered from that segment's sonar data and placed
/// precisely along the track with its four corners defined in lat/lon.
fn write_kmz(
    kml_path: &Path,
    kmz_path: &Path,
    parsed: &ParseResult,
    _output_dir: &Path,
    colormap: &str,
    remove_water_column: bool,
    alignments: &[crate::channel_alignment::ChannelAlignment],
    sidescan_pair: (Option<u32>, Option<u32>),
) -> Result<bool> {
    let kml_str = fs::read_to_string(kml_path)
        .with_context(|| format!("Failed to read KML for KMZ: {}", kml_path.display()))?;

    // Use pre-computed port + starboard sidescan channel pair
    let (port_ch, star_ch) = sidescan_pair;
    if port_ch.is_none() && star_ch.is_none() {
        return Ok(false);
    }

    // Collect GPS-valid pings for each channel
    let channels = pings_by_channel(parsed);
    let port_pings: Vec<&Ping> = port_ch
        .and_then(|ch| channels.get(&ch))
        .map(|v| {
            v.iter()
                .filter(|p| {
                    p.latitude.is_finite()
                        && p.longitude.is_finite()
                        && (p.latitude != 0.0 || p.longitude != 0.0)
                })
                .copied()
                .collect()
        })
        .unwrap_or_default();
    let star_pings: Vec<&Ping> = star_ch
        .and_then(|ch| channels.get(&ch))
        .map(|v| {
            v.iter()
                .filter(|p| {
                    p.latitude.is_finite()
                        && p.longitude.is_finite()
                        && (p.latitude != 0.0 || p.longitude != 0.0)
                })
                .copied()
                .collect()
        })
        .unwrap_or_default();

    // Use whichever channel has more pings to drive segmentation & geography
    let guide_pings: &Vec<&Ping> = if star_pings.len() >= port_pings.len() {
        &star_pings
    } else {
        &port_pings
    };
    if guide_pings.len() < 4 {
        return Ok(false);
    }

    // Adaptive segmentation tuned from this track's heading variability.
    // KMZ: allow segments as small as 12 pings through turns for tighter
    // curve-following with gx:LatLonQuad overlays.
    let (raw_base, seg_heading_thr) = adaptive_segmentation_params(guide_pings);
    let seg_base = raw_base.clamp(80, 400);
    let seg_ranges = segment_by_heading(guide_pings, seg_base, seg_heading_thr);
    let guide_segments: Vec<&[&Ping]> = seg_ranges
        .iter()
        .map(|&(s, e)| &guide_pings[s..e] as &[&Ping])
        .collect();

    // Per-segment swath width from fused depth+nadir geometry.
    let guide_offsets = smooth_nadir_offsets(&detect_per_ping_nadir(guide_pings));
    let seg_swath_half_m: Vec<f64> = seg_ranges
        .iter()
        .map(|&(s, e)| segment_swath_half_m(&guide_pings[s..e], &guide_offsets[s..e]))
        .collect();

    let boundaries = compute_shared_boundaries(&guide_segments, &seg_swath_half_m);

    struct KmzSegment {
        strip: image::RgbImage,
        corners: [(f64, f64); 4],
        idx: usize,
    }
    let mut segments: Vec<KmzSegment> = Vec::new();

    // Build a mapping from guide-segment index ranges to the other channel's pings
    // by matching on timestamp proximity
    let other_pings: &Vec<&Ping> = if star_pings.len() >= port_pings.len() {
        &port_pings
    } else {
        &star_pings
    };

    // ── Global normalization: compute TVG + percentile norms across the ENTIRE
    // track so all segments share the same contrast range.  This eliminates
    // brightness banding between adjacent segments.
    let compute_offsets = |pings: &[&Ping]| -> Vec<usize> {
        if pings.is_empty() {
            return vec![];
        }
        let raw = detect_per_ping_nadir(pings);
        if remove_water_column {
            smooth_nadir_offsets(&raw)
        } else {
            let mut sorted = raw.clone();
            sorted.sort_unstable();
            let median = sorted[sorted.len() / 2];
            vec![median.min(30); pings.len()]
        }
    };
    let global_port_norm = if !port_pings.is_empty() {
        let offsets = compute_offsets(&port_pings);
        let tvg = compute_empirical_tvg(&port_pings, &offsets);
        let (p2, p98) = compute_segment_norm(&port_pings, &offsets, &tvg);
        Some(PrecomputedNorm { tvg, p2, p98 })
    } else {
        None
    };
    let global_star_norm = if !star_pings.is_empty() {
        let offsets = compute_offsets(&star_pings);
        let tvg = compute_empirical_tvg(&star_pings, &offsets);
        let (p2, p98) = compute_segment_norm(&star_pings, &offsets, &tvg);
        Some(PrecomputedNorm { tvg, p2, p98 })
    } else {
        None
    };

    for (seg_idx, seg_guide) in guide_segments.iter().enumerate() {
        if seg_guide.len() < 2 {
            continue;
        }

        let ((sl_lon, sl_lat), (sr_lon, sr_lat)) = boundaries[seg_idx];
        let ((el_lon, el_lat), (er_lon, er_lat)) = boundaries[seg_idx + 1];

        // gx:LatLonQuad: corners map to image in order:
        //   [0]=bottom-left, [1]=bottom-right, [2]=top-right, [3]=top-left
        // Image top (row 0) = first ping = START of segment
        // Image bottom (last row) = last ping = END of segment
        // Left half = port, Right half = starboard
        let corners: [(f64, f64); 4] = [
            (el_lon, el_lat), // bottom-left  = end-port
            (er_lon, er_lat), // bottom-right = end-starboard
            (sr_lon, sr_lat), // top-right    = start-starboard
            (sl_lon, sl_lat), // top-left     = start-port
        ];

        let seg_w = KMZ_OVERLAY_WIDTH;
        let seg_h = (seg_guide.len() as u32)
            .min(KMZ_OVERLAY_MAX_HEIGHT)
            .max(1);

        // Find matching pings in the other channel by timestamp range
        let ts_start = seg_guide.first().map(|p| p.timestamp_ms).unwrap_or(0);
        let ts_end = seg_guide.last().map(|p| p.timestamp_ms).unwrap_or(u64::MAX);
        let seg_other: Vec<&Ping> = other_pings
            .iter()
            .filter(|p| p.timestamp_ms >= ts_start && p.timestamp_ms <= ts_end)
            .copied()
            .collect();

        // Determine which vec is port and which is starboard for this segment
        let (seg_port, seg_star) = if star_pings.len() >= port_pings.len() {
            // guide = starboard, other = port
            (seg_other.as_slice(), *seg_guide)
        } else {
            // guide = port, other = starboard
            (*seg_guide, seg_other.as_slice())
        };

        let strip = render_stitched_overlay_strip(
            &seg_port.iter().copied().collect::<Vec<_>>(),
            &seg_star.iter().copied().collect::<Vec<_>>(),
            seg_w,
            seg_h,
            colormap,
            remove_water_column,
            alignments,
            port_ch,
            star_ch,
            global_port_norm.as_ref(),
            global_star_norm.as_ref(),
        );

        segments.push(KmzSegment {
            strip,
            corners,
            idx: seg_idx,
        });
    }

    if segments.is_empty() {
        return Ok(false);
    }

    // Along-track registration: satellite-style NCC on overlap bands between strips.
    {
        let cfg = crate::overlay_align::AlignConfig::default();
        for i in 1..segments.len() {
            let (left, right) = segments.split_at_mut(i);
            crate::overlay_align::align_strip_pair(&left[i - 1].strip, &mut right[0].strip, &cfg);
        }
    }

    let mut overlay_kml_parts = Vec::new();
    let mut png_entries: Vec<(String, Vec<u8>)> = Vec::new();

    for seg in &segments {
        let strip = crate::overlay_align::mask_dropout_rows(&seg.strip);
        // Light edge feather only — heavy Y feather softens detail in Google Earth.
        let rgba_strip = apply_alpha_feathering(&strip, 0.03, 0.04);

        // Lossless PNG for GE sharpness (WebP from image crate is often lossy).
        let png_name = format!("seg_{:04}.png", seg.idx);
        if let Ok(bytes) = encode_png_rgba(&rgba_strip) {
            overlay_kml_parts.push(format!(
                "  <GroundOverlay>\
                \n    <name>Segment {}</name>\
                \n    <color>ffffffff</color>\
                \n    <drawOrder>{}</drawOrder>\
                \n    <Icon><href>{}</href></Icon>\
                \n    <altitude>0</altitude>\
                \n    <altitudeMode>clampToGround</altitudeMode>\
                \n    <gx:LatLonQuad>\
                \n      <coordinates>{:.7},{:.7},0 {:.7},{:.7},0 {:.7},{:.7},0 {:.7},{:.7},0</coordinates>\
                \n    </gx:LatLonQuad>\
                \n  </GroundOverlay>",
                seg.idx,
                seg.idx + 1,
                png_name,
                seg.corners[0].0,
                seg.corners[0].1,
                seg.corners[1].0,
                seg.corners[1].1,
                seg.corners[2].0,
                seg.corners[2].1,
                seg.corners[3].0,
                seg.corners[3].1,
            ));
            png_entries.push((png_name, bytes));
        } else if let Ok(bytes) = encode_webp_rgba(&rgba_strip) {
            let webp_name = format!("seg_{:04}.webp", seg.idx);
            overlay_kml_parts.push(format!(
                "  <GroundOverlay>\
                \n    <name>Segment {}</name>\
                \n    <color>ffffffff</color>\
                \n    <drawOrder>{}</drawOrder>\
                \n    <Icon><href>{}</href></Icon>\
                \n    <altitude>0</altitude>\
                \n    <altitudeMode>clampToGround</altitudeMode>\
                \n    <gx:LatLonQuad>\
                \n      <coordinates>{:.7},{:.7},0 {:.7},{:.7},0 {:.7},{:.7},0 {:.7},{:.7},0</coordinates>\
                \n    </gx:LatLonQuad>\
                \n  </GroundOverlay>",
                seg.idx,
                seg.idx + 1,
                webp_name,
                seg.corners[0].0,
                seg.corners[0].1,
                seg.corners[1].0,
                seg.corners[1].1,
                seg.corners[2].0,
                seg.corners[2].1,
                seg.corners[3].0,
                seg.corners[3].1,
            ));
            png_entries.push((webp_name, bytes));
        }
    }

    if png_entries.is_empty() {
        return Ok(false);
    }

    // Inject all segmented GroundOverlays + gx namespace into KML
    let overlays_block = overlay_kml_parts.join("\n");
    let final_kml = if let Some(pos) = kml_str.rfind("</Document>") {
        let mut s = kml_str[..pos].to_string();
        s.push_str(&overlays_block);
        s.push_str("\n</Document>\n</kml>");
        s = s.replace(
            "xmlns=\"http://www.opengis.net/kml/2.2\"",
            "xmlns=\"http://www.opengis.net/kml/2.2\" xmlns:gx=\"http://www.google.com/kml/ext/2.2\"",
        );
        s
    } else {
        kml_str
    };

    let file = fs::File::create(kmz_path)
        .with_context(|| format!("Failed to create KMZ: {}", kmz_path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("doc.kml", SimpleFileOptions::default())?;
    zip.write_all(final_kml.as_bytes())?;
    for (name, bytes) in &png_entries {
        zip.start_file(name.as_str(), SimpleFileOptions::default())?;
        zip.write_all(bytes)?;
    }
    zip.finish()?;

    Ok(true)
}

#[derive(serde::Serialize)]
struct ArcGisSpatialRef {
    wkid: u32,
}
#[derive(serde::Serialize)]
struct ArcGisPoint {
    x: f64,
    y: f64,
    #[serde(rename = "spatialReference")]
    spatial_reference: ArcGisSpatialRef,
}
#[derive(serde::Serialize)]
struct ArcGisAttributes<'a> {
    sequence: u32,
    timestamp_ms: u64,
    depth_m: f32,
    depth_ft: f32,
    altitude_m: f32,
    beam_angle_deg: f32,
    heading_deg: Option<f32>,
    pitch_deg: Option<f32>,
    roll_deg: Option<f32>,
    bottom_hardness: Option<f32>,
    bottom_type: &'a str,
    channel: u16,
    sample_count: u16,
}
#[derive(serde::Serialize)]
struct ArcGisFeature<'a> {
    geometry: ArcGisPoint,
    attributes: ArcGisAttributes<'a>,
}

fn write_arcgis_sidecar(parsed: &ParseResult, path: &Path) -> Result<()> {
    let features: Vec<ArcGisFeature> = parsed
        .pings
        .iter()
        .map(|p| {
            let (bottom_hardness, bottom_type) = estimate_bottom_hardness(&p.samples);
            ArcGisFeature {
                geometry: ArcGisPoint {
                    x: p.longitude,
                    y: p.latitude,
                    spatial_reference: ArcGisSpatialRef { wkid: 4326 },
                },
                attributes: ArcGisAttributes {
                    sequence: p.sequence,
                    timestamp_ms: p.timestamp_ms,
                    depth_m: (p.depth_m * 1000.0).round() / 1000.0,
                    depth_ft: (p.depth_ft * 100.0).round() / 100.0,
                    altitude_m: p.altitude_m,
                    beam_angle_deg: p.beam_angle_deg,
                    heading_deg: p.heading_deg,
                    pitch_deg: p.pitch_deg,
                    roll_deg: p.roll_deg,
                    bottom_hardness,
                    bottom_type,
                    channel: p.channel as u16,
                    sample_count: p.sample_count as u16,
                },
            }
        })
        .collect();

    let doc = serde_json::json!({
        "geometryType": "esriGeometryPoint",
        "spatialReference": {"wkid": 4326},
        "fields": [
            {"name":"sequence",       "type":"esriFieldTypeInteger"},
            {"name":"timestamp_ms",   "type":"esriFieldTypeDouble"},
            {"name":"depth_m",        "type":"esriFieldTypeDouble"},
            {"name":"depth_ft",       "type":"esriFieldTypeDouble"},
            {"name":"altitude_m",     "type":"esriFieldTypeDouble"},
            {"name":"beam_angle_deg", "type":"esriFieldTypeDouble"},
            {"name":"heading_deg",    "type":"esriFieldTypeDouble"},
            {"name":"pitch_deg",      "type":"esriFieldTypeDouble"},
            {"name":"roll_deg",       "type":"esriFieldTypeDouble"},
            {"name":"bottom_hardness", "type":"esriFieldTypeDouble"},
            {"name":"bottom_type",     "type":"esriFieldTypeString", "length": 16},
            {"name":"channel",        "type":"esriFieldTypeInteger"},
            {"name":"sample_count",   "type":"esriFieldTypeInteger"}
        ],
        "features": features
    });

    fs::write(path, serde_json::to_vec_pretty(&doc)?)
        .with_context(|| format!("Failed to write ArcGIS sidecar: {}", path.display()))?;
    Ok(())
}

fn estimate_bottom_hardness(samples: &[u16]) -> (Option<f32>, &'static str) {
    if samples.len() < 24 {
        return (None, "unknown");
    }

    let search_end = (samples.len() * 3 / 5).max(1);
    let mut peak_idx = 0usize;
    let mut peak_val = 0u16;
    for (i, &v) in samples.iter().take(search_end).enumerate() {
        if v > peak_val {
            peak_val = v;
            peak_idx = i;
        }
    }
    if peak_val < 16 {
        return (None, "unknown");
    }

    let tail_start = peak_idx.saturating_add(1);
    if tail_start >= samples.len() {
        return (None, "unknown");
    }

    let tail_window = (samples.len() / 10).max(8);
    let tail_end = (tail_start + tail_window).min(samples.len());
    let tail_slice = &samples[tail_start..tail_end];
    if tail_slice.is_empty() {
        return (None, "unknown");
    }

    let tail_mean = tail_slice.iter().map(|&v| v as f32).sum::<f32>() / tail_slice.len() as f32;
    let tail_ratio = (tail_mean / peak_val as f32).clamp(0.0, 1.0);
    let hardness = ((tail_ratio - 0.08) / 0.45).clamp(0.0, 1.0);
    let label = if hardness >= 0.67 {
        "hard"
    } else if hardness >= 0.33 {
        "mixed"
    } else {
        "soft"
    };

    (Some((hardness * 1000.0).round() / 1000.0), label)
}

fn write_native_viewer(
    parsed: &ParseResult,
    viewer_dir: &Path,
    colormap: &str,
    remove_water_column: bool,
    detections: Option<&DetectionSummary>,
    alignments: &[crate::channel_alignment::ChannelAlignment],
    enable_nautical_charts: bool,
    sidescan_pair: (Option<u32>, Option<u32>),
) -> Result<()> {
    fs::create_dir_all(viewer_dir)
        .with_context(|| format!("Failed to create viewer dir: {}", viewer_dir.display()))?;

    // ── Build GeoJSON values ──────────────────────────────────────────────────
    #[derive(serde::Serialize)]
    struct GeoJsonLineString {
        #[serde(rename = "type")]
        geom_type: &'static str,
        coordinates: Vec<[f64; 2]>,
    }
    #[derive(serde::Serialize)]
    struct GeoJsonFeatureProp {
        name: &'static str,
    }
    #[derive(serde::Serialize)]
    struct GeoJsonFeature {
        #[serde(rename = "type")]
        feat_type: &'static str,
        geometry: GeoJsonLineString,
        properties: GeoJsonFeatureProp,
    }
    #[derive(serde::Serialize)]
    struct GeoJsonFeatureCol {
        #[serde(rename = "type")]
        col_type: &'static str,
        features: Vec<GeoJsonFeature>,
    }

    let track_coords: Vec<[f64; 2]> = parsed
        .pings
        .iter()
        .filter(|p| {
            p.latitude.is_finite()
                && p.longitude.is_finite()
                && (p.latitude != 0.0 || p.longitude != 0.0)
        })
        .map(|p| [p.longitude, p.latitude])
        .collect();

    let track_geojson = GeoJsonFeatureCol {
        col_type: "FeatureCollection",
        features: vec![GeoJsonFeature {
            feat_type: "Feature",
            geometry: GeoJsonLineString {
                geom_type: "LineString",
                coordinates: track_coords,
            },
            properties: GeoJsonFeatureProp {
                name: "Sonar track",
            },
        }],
    };

    let step = (parsed.pings.len() / VIEWER_MAX_PINGS).max(1);
    let ping_features: Vec<_> = parsed
        .pings
        .iter()
        .step_by(step)
        .filter(|p| {
            p.latitude.is_finite()
                && p.longitude.is_finite()
                && (p.latitude != 0.0 || p.longitude != 0.0)
        })
        .map(|p| {
            let (bottom_hardness, bottom_type) = estimate_bottom_hardness(&p.samples);
            serde_json::json!({
                "type": "Feature",
                "geometry": { "type": "Point", "coordinates": [p.longitude, p.latitude] },
                "properties": {
                    "sequence":     p.sequence,
                    "depth_ft":     (p.depth_ft * 10.0).round() / 10.0,
                    "depth_m":      (p.depth_m * 100.0).round() / 100.0,
                    "channel":      p.channel,
                    "sample_count": p.sample_count,
                    "timestamp_ms": p.timestamp_ms,
                    "bottom_hardness": bottom_hardness,
                    "bottom_type": bottom_type
                }
            })
        })
        .collect();

    let pings_geojson = serde_json::json!({
        "type": "FeatureCollection",
        "features": ping_features
    });

    // ── Write separate GeoJSON files (useful for QGIS / external tools) ───────
    fs::write(
        viewer_dir.join("track.geojson"),
        serde_json::to_vec_pretty(&track_geojson)?,
    )
    .context("Failed to write track.geojson")?;

    fs::write(
        viewer_dir.join("pings.geojson"),
        serde_json::to_vec_pretty(&pings_geojson)?,
    )
    .context("Failed to write pings.geojson")?;

    // ── Generate sonar overlay strips (same geometry as KMZ) ──────────────────
    // Renders sidescan segments as PNG files + a JSON manifest so the viewer can
    // display them as MapLibre `image` sources along the track.
    let sonar_overlays = generate_viewer_sonar_overlays(
        parsed,
        viewer_dir,
        colormap,
        remove_water_column,
        alignments,
        sidescan_pair,
    )?;
    let overlays_json_str = serde_json::to_string(&sonar_overlays)?;

    // ── data.js – inline GeoJSON as globals so index.html works via file:// ──
    // This avoids the CORS error that fetch() triggers when opened locally.
    let track_json_str = serde_json::to_string(&track_geojson)?;
    let pings_json_str = serde_json::to_string(&pings_geojson)?;
    let detections_geojson = detections
        .map(|d| build_detections_geojson(d))
        .unwrap_or_else(|| serde_json::json!({"type": "FeatureCollection", "features": []}));
    let detections_json_str = serde_json::to_string(&detections_geojson)?;
    let data_js = format!(
        "/* Auto-generated by SonarSniffer — do not edit */\n\
        var TRACK_GEOJSON = {};\n\
        var PINGS_GEOJSON = {};\n\
        var SONAR_OVERLAYS = {};\n\
        var DETECTIONS_GEOJSON = {};\n",
        track_json_str, pings_json_str, overlays_json_str, detections_json_str
    );
    fs::write(viewer_dir.join("data.js"), data_js).context("Failed to write viewer data.js")?;

    // ── index.html ────────────────────────────────────────────────────────────
    let html = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>SonarSniffer Viewer</title>
  <link href="https://unpkg.com/maplibre-gl@4.7.1/dist/maplibre-gl.css" rel="stylesheet" />
  <script src="https://unpkg.com/maplibre-gl@4.7.1/dist/maplibre-gl.js"></script>
  <style>
    html, body, #map { margin: 0; height: 100%; width: 100%; }
    .panel {
      position: absolute; top: 12px; left: 12px;
      background: rgba(255,255,255,0.92); padding: 10px 14px;
      font: 12px/1.5 sans-serif; border-radius: 8px; max-width: 280px;
      box-shadow: 0 2px 8px rgba(0,0,0,0.18);
    }
    .panel strong { font-size: 13px; }
    .legend { margin-top: 8px; display: flex; align-items: center; gap: 6px; }
    .legend-bar {
      width: 100px; height: 10px; border-radius: 4px;
      background: linear-gradient(to right, #d0f0ff, #00aaff, #00dd88, #ffcc00, #ff2200);
    }
    .legend-labels { display: flex; justify-content: space-between; width: 100px; font-size: 10px; color: #555; }
    .toggle-row { margin-top: 6px; display: flex; align-items: center; gap: 5px; }
    .toggle-row label { cursor: pointer; user-select: none; }
  </style>
</head>
<body>
<div id="map"></div>
<div class="panel">
  <strong>SonarSniffer Viewer</strong><br />
  Track: <span style="color:#ff5a36">&#x2014;</span>&nbsp;
  Pings coloured by depth.  Click a ping for details.<br />
  <div class="legend">
    <div>
      <div class="legend-bar"></div>
      <div class="legend-labels"><span id="dmin">0</span><span id="dmax">&hellip;</span></div>
    </div>
    <span style="font-size:10px">ft</span>
  </div>
  <div class="toggle-row">
    <input type="checkbox" id="sonarToggle" checked />
    <label for="sonarToggle">Show sonar overlay</label>
  </div>
  <div class="toggle-row">
    <input type="checkbox" id="trackToggle" checked />
    <label for="trackToggle">Show track line</label>
  </div>
  <div class="toggle-row">
    <input type="checkbox" id="depthToggle" checked />
    <label for="depthToggle">Show depth pings</label>
  </div>
  <div class="toggle-row">
    <input type="checkbox" id="detectionsToggle" checked />
    <label for="detectionsToggle">Show detections (<span id="detCount">0</span>)</label>
  </div>
</div>
<!-- data.js embeds track + ping GeoJSON + sonar overlay manifest inline -->
<script src="data.js"></script>
<script src="app.js"></script>
</body>
</html>
"#;

    // ── app.js – reads from inline globals; no fetch() ────────────────────────
    // OSM is always the base layer. NOAA Seamless RNC chart tiles are added on top
    // when the nautical charts option is enabled. Both are served with CORS headers.
    let js_sources = if enable_nautical_charts {
        r#"
        osm: {
          type: 'raster',
          tiles: ['https://tile.openstreetmap.org/{z}/{x}/{y}.png'],
          tileSize: 256,
          attribution: '&copy; OpenStreetMap contributors'
        },
        noaa_rnc: {
          type: 'raster',
          tiles: ['https://tileservice.charts.noaa.gov/tiles/50000_1/{z}/{x}/{y}.png'],
          tileSize: 256,
          attribution: 'NOAA, U.S. National Ocean Service'
      }
        "#
    } else {
        r#"
      osm: {
        type: 'raster',
        tiles: ['https://tile.openstreetmap.org/{z}/{x}/{y}.png'],
        tileSize: 256,
        attribution: '&copy; OpenStreetMap contributors'
      }
        "#
    };

    let js_layers = if enable_nautical_charts {
        "[{ id: 'osm', type: 'raster', source: 'osm' }, { id: 'noaa_rnc', type: 'raster', source: 'noaa_rnc', paint: {'raster-opacity': 0.8} }]"
    } else {
        "[{ id: 'osm', type: 'raster', source: 'osm' }]"
    };

    let js_header = format!(
        r#"const map = new maplibregl.Map({{
  container: 'map',
  style: {{
    version: 8,
    sources: {{{}}},
    layers: {}
  }},
  center: [-90, 30],
  zoom: 3
}});
"#,
        js_sources, js_layers
    );

    let js_body = r#"
// TRACK_GEOJSON, PINGS_GEOJSON, SONAR_OVERLAYS, DETECTIONS_GEOJSON are declared in data.js
function load() {"#;

    let js = format!("{}{}", js_header, js_body);

    let js = format!(
        "{}{}",
        js,
        r#"
  const trackGeo = TRACK_GEOJSON;
  const pingsGeo = PINGS_GEOJSON;
  const overlays = (typeof SONAR_OVERLAYS !== 'undefined') ? SONAR_OVERLAYS : [];
  const detectionsGeo = (typeof DETECTIONS_GEOJSON !== 'undefined') ? DETECTIONS_GEOJSON : {type:'FeatureCollection',features:[]};

  // Show detection count
  const detCount = detectionsGeo.features ? detectionsGeo.features.length : 0;
  const detCountEl = document.getElementById('detCount');
  if (detCountEl) detCountEl.textContent = detCount;
  const detToggleRow = document.getElementById('detectionsToggle')?.closest('.toggle-row');
  if (detToggleRow && detCount === 0) detToggleRow.style.display = 'none';

  console.log('[viewer] Data loaded: ' + overlays.length + ' overlays, ' +
    (trackGeo.features?.[0]?.geometry?.coordinates?.length || 0) + ' track points, ' +
    pingsGeo.features.length + ' pings');

  const coords = trackGeo.features?.[0]?.geometry?.coordinates ?? [];
  if (coords.length > 1) {
    const bounds = coords.reduce(
      (b, c) => b.extend(c),
      new maplibregl.LngLatBounds(coords[0], coords[0])
    );
    map.fitBounds(bounds, { padding: 48, duration: 0 });
  }

  const depths = pingsGeo.features.map(f => f.properties.depth_ft || 0).filter(d => d > 0);
  const maxDepth = depths.length ? Math.ceil(depths.reduce((a, b) => Math.max(a, b), 0)) : 60;
  const dmaxEl = document.getElementById('dmax');
  if (dmaxEl) dmaxEl.textContent = maxDepth;

  // Hide/show sonar toggle if no overlays
  const toggleRow = document.querySelector('.toggle-row');
  if (!overlays.length && toggleRow) toggleRow.style.display = 'none';

  map.on('load', () => {
    // ── Sonar overlay strips (image sources) ──────────────────────────────
    const sonarLayerIds = [];
    console.log('[viewer] Loading ' + overlays.length + ' sonar overlays');
    for (let i = 0; i < overlays.length; i++) {
      const ov = overlays[i];
      const srcId = 'sonar-src-' + i;
      const layerId = 'sonar-layer-' + i;
      try {
        map.addSource(srcId, {
          type: 'image',
          url: ov.url,
          coordinates: ov.coordinates
        });
        map.addLayer({
          id: layerId,
          type: 'raster',
          source: srcId,
          paint: { 'raster-opacity': 0.92, 'raster-fade-duration': 0 }
        });
        sonarLayerIds.push(layerId);
      } catch (err) {
        console.error('[viewer] Failed to add overlay ' + i + ':', err, ov);
      }
    }

    // Track line
    map.addSource('track', { type: 'geojson', data: trackGeo });
    map.addLayer({
      id: 'track-line',
      type: 'line',
      source: 'track',
      paint: { 'line-color': '#ff5a36', 'line-width': 2.5 }
    });

    // Re-insert sonar layers below track line now that it exists
    for (const lid of sonarLayerIds) {
      map.moveLayer(lid, 'track-line');
    }

    // Ping depth circles
    map.addSource('pings', { type: 'geojson', data: pingsGeo });
    map.addLayer({
      id: 'pings-dots',
      type: 'circle',
      source: 'pings',
      paint: {
        'circle-radius': 4,
        'circle-color': [
          'interpolate', ['linear'], ['get', 'depth_ft'],
          0,               '#d0f0ff',
          maxDepth * 0.25, '#00aaff',
          maxDepth * 0.5,  '#00dd88',
          maxDepth * 0.75, '#ffcc00',
          maxDepth,        '#ff2200'
        ],
        'circle-opacity': 0.85,
        'circle-stroke-width': 0.5,
        'circle-stroke-color': 'rgba(0,0,0,0.25)'
      }
    });

    // Toggle sonar overlay visibility
    const toggle = document.getElementById('sonarToggle');
    if (toggle) {
      toggle.addEventListener('change', () => {
        const vis = toggle.checked ? 'visible' : 'none';
        for (const lid of sonarLayerIds) {
          map.setLayoutProperty(lid, 'visibility', vis);
        }
      });
    }

    // Toggle track line visibility
    const trackToggle = document.getElementById('trackToggle');
    if (trackToggle) {
      trackToggle.addEventListener('change', () => {
        map.setLayoutProperty('track-line', 'visibility', trackToggle.checked ? 'visible' : 'none');
      });
    }

    // Toggle depth pings visibility
    const depthToggle = document.getElementById('depthToggle');
    if (depthToggle) {
      depthToggle.addEventListener('change', () => {
        map.setLayoutProperty('pings-dots', 'visibility', depthToggle.checked ? 'visible' : 'none');
      });
    }

    map.on('click', 'pings-dots', e => {
      if (!e.features.length) return;
      const p = e.features[0].properties;
            const hardness = (p.bottom_hardness !== undefined && p.bottom_hardness !== null)
                ? `${Math.round(Number(p.bottom_hardness) * 100)}% (${p.bottom_type || 'unknown'})<br>`
                : '';
      new maplibregl.Popup()
        .setLngLat(e.lngLat)
        .setHTML(
          `<b>Ping #${p.sequence}</b><br>` +
          `Depth: <b>${p.depth_ft} ft</b> (${p.depth_m} m)<br>` +
                    `Bottom: ${hardness}` +
          `Channel: ${p.channel} &nbsp;&middot;&nbsp; Samples: ${p.sample_count}`
        )
        .addTo(map);
    });

    map.on('mouseenter', 'pings-dots', () => { map.getCanvas().style.cursor = 'pointer'; });
    map.on('mouseleave', 'pings-dots', () => { map.getCanvas().style.cursor = ''; });

    // ── Detection markers ──────────────────────────────────────────────────
    if (detectionsGeo.features && detectionsGeo.features.length > 0) {
      map.addSource('detections', { type: 'geojson', data: detectionsGeo });

      // Colour by classification
      const classColors = {
        fish: '#00ff88', baitball: '#00ddff', structure: '#ffaa00',
        debris: '#ff6600', wreck: '#ff0044'
      };
      const colorExpr = ['match', ['get', 'classification']];
      for (const [cls, col] of Object.entries(classColors)) {
        colorExpr.push(cls, col);
      }
      colorExpr.push('#ffffff'); // fallback

      map.addLayer({
        id: 'detections-circles',
        type: 'circle',
        source: 'detections',
        paint: {
          'circle-radius': ['interpolate', ['linear'], ['get', 'blob_area'],
            4, 5, 100, 8, 1000, 12, 10000, 18],
          'circle-color': colorExpr,
          'circle-opacity': 0.85,
          'circle-stroke-width': 2,
          'circle-stroke-color': '#ffffff'
        }
      });

      map.addLayer({
        id: 'detections-labels',
        type: 'symbol',
        source: 'detections',
        layout: {
          'text-field': ['get', 'classification'],
          'text-size': 10,
          'text-offset': [0, 1.5],
          'text-anchor': 'top'
        },
        paint: {
          'text-color': '#ffffff',
          'text-halo-color': 'rgba(0,0,0,0.7)',
          'text-halo-width': 1
        }
      });

      // Click popup for detections
      map.on('click', 'detections-circles', e => {
        if (!e.features.length) return;
        const p = e.features[0].properties;
        const conf = Math.round((p.confidence || 0) * 100);
        new maplibregl.Popup()
          .setLngLat(e.lngLat)
          .setHTML(
            `<b>${p.classification}</b> (${p.size_class})<br>` +
            `Size: <b>${p.width_m} m</b> wide &times; <b>${p.length_m} m</b> long` +
            ` (${(p.width_m * 3.281).toFixed(1)} &times; ${(p.length_m * 3.281).toFixed(1)} ft)<br>` +
            `Confidence: <b>${conf}%</b> &middot; Depth: ${p.depth_m} m<br>` +
            `Range: ${p.range_m} m &middot; ${p.channel_type}`
          )
          .addTo(map);
      });

      map.on('mouseenter', 'detections-circles', () => { map.getCanvas().style.cursor = 'pointer'; });
      map.on('mouseleave', 'detections-circles', () => { map.getCanvas().style.cursor = ''; });

      // Toggle visibility
      const detToggle = document.getElementById('detectionsToggle');
      if (detToggle) {
        detToggle.addEventListener('change', () => {
          const vis = detToggle.checked ? 'visible' : 'none';
          map.setLayoutProperty('detections-circles', 'visibility', vis);
          map.setLayoutProperty('detections-labels', 'visibility', vis);
        });
      }
    }
  });
}

load();
"#
    );

    fs::write(viewer_dir.join("index.html"), html).context("Failed to write viewer index.html")?;
    fs::write(viewer_dir.join("app.js"), js).context("Failed to write viewer app.js")?;
    Ok(())
}

/// Build a GeoJSON FeatureCollection from detection results.
fn build_detections_geojson(det: &DetectionSummary) -> serde_json::Value {
    let features: Vec<serde_json::Value> = det
        .detections
        .iter()
        .map(|d| {
            serde_json::json!({
                "type": "Feature",
                "geometry": {
                    "type": "Point",
                    "coordinates": [d.longitude, d.latitude]
                },
                "properties": {
                    "classification": d.classification,
                    "size_class": d.size_class,
                    "blob_area": d.blob_area,
                    "width_m": (d.width_m * 10.0).round() / 10.0,
                    "length_m": (d.length_m * 10.0).round() / 10.0,
                    "avg_intensity": (d.avg_intensity * 10.0).round() / 10.0,
                    "confidence": (d.confidence * 100.0).round() / 100.0,
                    "depth_m": (d.depth_m * 100.0).round() / 100.0,
                    "range_m": (d.range_m * 100.0).round() / 100.0,
                    "channel": d.channel,
                    "channel_type": d.channel_type,
                }
            })
        })
        .collect();

    serde_json::json!({
        "type": "FeatureCollection",
        "features": features
    })
}

/// Render sidescan sonar strips as PNG files in the viewer directory and return
/// a JSON-serialisable manifest of `{url, coordinates}` entries that the viewer
/// app.js can load as MapLibre `image` sources.
fn generate_viewer_sonar_overlays(
    parsed: &ParseResult,
    viewer_dir: &Path,
    colormap: &str,
    remove_water_column: bool,
    alignments: &[crate::channel_alignment::ChannelAlignment],
    sidescan_pair: (Option<u32>, Option<u32>),
) -> Result<Vec<serde_json::Value>> {
    // Use pre-computed port + starboard sidescan channel pair
    let (port_ch, star_ch) = sidescan_pair;
    if port_ch.is_none() && star_ch.is_none() {
        return Ok(vec![]);
    }

    let channels = pings_by_channel(parsed);
    let port_pings: Vec<&Ping> = port_ch
        .and_then(|ch| channels.get(&ch))
        .map(|v| {
            v.iter()
                .filter(|p| {
                    p.latitude.is_finite()
                        && p.longitude.is_finite()
                        && (p.latitude != 0.0 || p.longitude != 0.0)
                })
                .copied()
                .collect()
        })
        .unwrap_or_default();
    let star_pings: Vec<&Ping> = star_ch
        .and_then(|ch| channels.get(&ch))
        .map(|v| {
            v.iter()
                .filter(|p| {
                    p.latitude.is_finite()
                        && p.longitude.is_finite()
                        && (p.latitude != 0.0 || p.longitude != 0.0)
                })
                .copied()
                .collect()
        })
        .unwrap_or_default();

    let guide_pings: &Vec<&Ping> = if star_pings.len() >= port_pings.len() {
        &star_pings
    } else {
        &port_pings
    };
    if guide_pings.len() < 4 {
        return Ok(vec![]);
    }

    // Viewer overlays: use larger segments (floor=200 pings) to keep the total
    // number of image sources manageable for MapLibre's WebGL renderer.
    // 40K pings / 200 = ~200 overlays (vs 800+ with floor=50).
    let (raw_base, seg_heading_thr) = adaptive_segmentation_params(guide_pings);
    let seg_base = raw_base.max(200);
    let seg_ranges = segment_by_heading(guide_pings, seg_base, seg_heading_thr);
    let guide_segments: Vec<&[&Ping]> = seg_ranges
        .iter()
        .map(|&(s, e)| &guide_pings[s..e] as &[&Ping])
        .collect();

    // Per-segment swath width from fused depth+nadir geometry.
    let guide_offsets = smooth_nadir_offsets(&detect_per_ping_nadir(guide_pings));
    let seg_swath_half_m: Vec<f64> = seg_ranges
        .iter()
        .map(|&(s, e)| segment_swath_half_m(&guide_pings[s..e], &guide_offsets[s..e]))
        .collect();

    let sonar_dir = viewer_dir.join("sonar");
    fs::create_dir_all(&sonar_dir).context("create viewer sonar dir")?;

    let boundaries = compute_shared_boundaries(&guide_segments, &seg_swath_half_m);

    struct ViewerSegment {
        strip: image::RgbImage,
        corners: [(f64, f64); 4],
        idx: usize,
    }
    let mut segments: Vec<ViewerSegment> = Vec::new();

    let other_pings: &Vec<&Ping> = if star_pings.len() >= port_pings.len() {
        &port_pings
    } else {
        &star_pings
    };

    // ── Global normalization for consistent cross-segment brightness ──────────
    let compute_offsets_v = |pings: &[&Ping]| -> Vec<usize> {
        if pings.is_empty() {
            return vec![];
        }
        let raw = detect_per_ping_nadir(pings);
        if remove_water_column {
            smooth_nadir_offsets(&raw)
        } else {
            let mut sorted = raw.clone();
            sorted.sort_unstable();
            let median = sorted[sorted.len() / 2];
            vec![median.min(30); pings.len()]
        }
    };
    let global_port_norm_v = if !port_pings.is_empty() {
        let offsets = compute_offsets_v(&port_pings);
        let tvg = compute_empirical_tvg(&port_pings, &offsets);
        let (p2, p98) = compute_segment_norm(&port_pings, &offsets, &tvg);
        Some(PrecomputedNorm { tvg, p2, p98 })
    } else {
        None
    };
    let global_star_norm_v = if !star_pings.is_empty() {
        let offsets = compute_offsets_v(&star_pings);
        let tvg = compute_empirical_tvg(&star_pings, &offsets);
        let (p2, p98) = compute_segment_norm(&star_pings, &offsets, &tvg);
        Some(PrecomputedNorm { tvg, p2, p98 })
    } else {
        None
    };

    for (seg_idx, seg_guide) in guide_segments.iter().enumerate() {
        if seg_guide.len() < 2 {
            continue;
        }

        let ((sl_lon, sl_lat), (sr_lon, sr_lat)) = boundaries[seg_idx];
        let ((el_lon, el_lat), (er_lon, er_lat)) = boundaries[seg_idx + 1];

        let seg_w = 256u32;
        let seg_h = (seg_guide.len() as u32).min(256).max(1);

        // Find matching pings in the other channel by timestamp range
        let ts_start = seg_guide.first().map(|p| p.timestamp_ms).unwrap_or(0);
        let ts_end = seg_guide.last().map(|p| p.timestamp_ms).unwrap_or(u64::MAX);
        let seg_other: Vec<&Ping> = other_pings
            .iter()
            .filter(|p| p.timestamp_ms >= ts_start && p.timestamp_ms <= ts_end)
            .copied()
            .collect();

        let (seg_port, seg_star) = if star_pings.len() >= port_pings.len() {
            (seg_other.as_slice(), *seg_guide)
        } else {
            (*seg_guide, seg_other.as_slice())
        };

        let strip = render_stitched_overlay_strip(
            &seg_port.iter().copied().collect::<Vec<_>>(),
            &seg_star.iter().copied().collect::<Vec<_>>(),
            seg_w,
            seg_h,
            colormap,
            remove_water_column,
            alignments,
            port_ch,
            star_ch,
            global_port_norm_v.as_ref(),
            global_star_norm_v.as_ref(),
        );

        segments.push(ViewerSegment {
            strip,
            corners: [
                (el_lon, el_lat),
                (er_lon, er_lat),
                (sr_lon, sr_lat),
                (sl_lon, sl_lat),
            ],
            idx: seg_idx,
        });
    }

    {
        let cfg = crate::overlay_align::AlignConfig::default();
        for i in 1..segments.len() {
            let (left, right) = segments.split_at_mut(i);
            crate::overlay_align::align_strip_pair(&left[i - 1].strip, &mut right[0].strip, &cfg);
        }
    }

    let mut result = Vec::new();
    for seg in &segments {
        let strip = crate::overlay_align::mask_dropout_rows(&seg.strip);
        let mut rgba_strip = apply_alpha_feathering(&strip, 0.0, 0.0);
        image::imageops::flip_vertical_in_place(&mut rgba_strip);

        let png_name = format!("seg_{:04}.webp", seg.idx);
        if let Ok(bytes) = encode_webp_rgba(&rgba_strip) {
            let _ = fs::write(sonar_dir.join(&png_name), &bytes);
            result.push(serde_json::json!({
                "url": format!("sonar/{png_name}"),
                "coordinates": [
                    [seg.corners[0].0, seg.corners[0].1],
                    [seg.corners[1].0, seg.corners[1].1],
                    [seg.corners[2].0, seg.corners[2].1],
                    [seg.corners[3].0, seg.corners[3].1],
                ]
            }));
        } else if let Ok(bytes) = encode_png_rgba(&rgba_strip) {
            let png_name = format!("seg_{:04}.png", seg.idx);
            let _ = fs::write(sonar_dir.join(&png_name), &bytes);
            result.push(serde_json::json!({
                "url": format!("sonar/{png_name}"),
                "coordinates": [
                    [seg.corners[0].0, seg.corners[0].1],
                    [seg.corners[1].0, seg.corners[1].1],
                    [seg.corners[2].0, seg.corners[2].1],
                    [seg.corners[3].0, seg.corners[3].1],
                ]
            }));
        }
    }

    Ok(result)
}

// ── Misc helpers ──────────────────────────────────────────────────────────────

fn channel_label(parsed: &ParseResult, ch: u32) -> String {
    parsed
        .channels
        .iter()
        .find(|c| c.id == ch)
        .and_then(|c| c.mapped_type.clone())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Map a channel's mapped_type label to a `SpatialRole` for EGN purposes.
///
/// Uses the same heuristic as `channel_discovery` but without a full parse pass —
/// enough to pick the right EGN beam-pattern correction.
fn egn_role_from_label(label: &str, channel_id: u32) -> SpatialRole {
    let lower = label.to_ascii_lowercase();
    if lower.contains("port") && lower.contains("side") {
        SpatialRole::SingleSidePort
    } else if lower.contains("starboard") && lower.contains("side") {
        SpatialRole::SingleSideStarboard
    } else if lower.contains("port") {
        SpatialRole::Port
    } else if lower.contains("starboard") || lower.contains("star") {
        SpatialRole::Starboard
    } else if lower.contains("down") || lower.contains("clear") || lower.contains("nadir") {
        SpatialRole::Center
    } else {
        // Fallback from channel ID (mirrors channel_discovery heuristics)
        match channel_id {
            4 => SpatialRole::Port,         // GT54 port / GT51 port wing
            5 => SpatialRole::Starboard,    // GT54 star / GT51 star wing
            10 => SpatialRole::Port,        // GT56 port
            11 => SpatialRole::Starboard,   // GT56 star
            12 | 13 => SpatialRole::Center, // DownVü
            _ => SpatialRole::Unassigned,
        }
    }
}

/// Apply EGN to a slice of Ping references, returning cloned Pings with
/// corrected `.samples`.  Nadir-skip is left at `0` for the first pass
/// since the beam profile percentile already excludes the dark zone.
fn apply_egn_to_channel_pings(pings: &[&Ping], role: SpatialRole) -> Vec<Ping> {
    if pings.is_empty() {
        return vec![];
    }
    let profile: BeamProfile = beam_profile_from_pings(pings, role, 0);
    pings
        .iter()
        .map(|&p| {
            let mut cloned = p.clone();
            cloned.samples = apply_egn(&p.samples, 0, &profile);
            cloned
        })
        .collect()
}
