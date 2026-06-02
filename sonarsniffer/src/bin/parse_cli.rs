/// parse_cli — Full pipeline CLI for SonarSniffer.
///
/// Parses a sonar file, runs the output pipeline, and prints results as JSON.
///
/// Usage:
///   cargo run --bin parse_cli -- <file> [--light] [--output-dir DIR] [--no-video]
///     [--no-kml] [--no-kmz] [--no-mbtiles] [--no-mosaic] [--no-waterfall]
///     [--no-arcgis] [--no-viewer] [--summary] [--first-n N]

use std::env;
use std::path::Path;

use sonarsniffer_lib::format_detector;
use sonarsniffer_lib::outputs::{build_outputs, PipelineOptions};

fn main() {
    let mut args = env::args().skip(1);
    let mut file: Option<String> = None;

    let mut options = PipelineOptions::default();
    let mut summary_only = false;
    let mut first_n: Option<usize> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--light" => {
                options.video = false;
                options.mosaic = false;
                options.waterfall = false;
                options.mbtiles = false;
                options.kmz = false;
                options.arcgis = false;
                options.web_viewer = false;
            }
            "--curvelet" => {
                options.curvelet_denoise = true;
            }
            "--curvelet-auto" => {
                options.curvelet_denoise = true;
                options.curvelet_auto = true;
            }
            "--curvelet-threshold" => {
                if let Some(t) = args.next() {
                    if let Ok(v) = t.parse::<f32>() {
                        options.curvelet_denoise = true;
                        options.curvelet_auto = false;
                        options.curvelet_threshold = v;
                    }
                }
            }
            "--no-video" => options.video = false,
            "--no-kml" => options.kml = false,
            "--no-kmz" => options.kmz = false,
            "--no-mbtiles" => options.mbtiles = false,
            "--no-mosaic" => options.mosaic = false,
            "--no-waterfall" => options.waterfall = false,
            "--no-arcgis" => options.arcgis = false,
            "--no-viewer" => options.web_viewer = false,
            "--video-downscope-rtl" => options.video_downscope_rtl = true,
            "--summary" => summary_only = true,
            "--first-n" => {
                if let Some(n_str) = args.next() {
                    match n_str.parse::<usize>() {
                        Ok(n) => first_n = Some(n),
                        Err(_) => eprintln!("--first-n requires an integer argument"),
                    }
                } else {
                    eprintln!("--first-n requires an integer argument");
                }
            }
            "--output-dir" => {
                if let Some(dir) = args.next() {
                    options.output_dir = Some(dir);
                }
            }
            val if val.starts_with('-') => {
                eprintln!("Unknown flag: {val}");
            }
            val => {
                file = Some(val.to_string());
            }
        }
    }

    let Some(file_name) = file else {
        eprintln!("usage: parse_cli <file> [--light] [--output-dir DIR] [--no-video] [--no-kml] [--no-kmz] [--no-mbtiles] [--no-mosaic] [--no-waterfall] [--no-arcgis] [--no-viewer] [--video-downscope-rtl] [--summary] [--first-n N]");
        std::process::exit(1);
    };

    let path = Path::new(&file_name);
    if !path.exists() {
        eprintln!("File not found: {}", file_name);
        std::process::exit(1);
    }

    // Detect format and parse
    let detected = format_detector::detect_and_parse(path);
    let mut parse_result = detected.parse;
    eprintln!("parsed pings: {}", parse_result.pings.len());
    eprintln!("format: {}", detected.format);

    // Run output pipeline if we have pings
    let output_summary = if !parse_result.pings.is_empty() {
        match build_outputs(path, &parse_result, &options, None, None) {
            Ok(summary) => Some(summary),
            Err(e) => {
                eprintln!("Output pipeline error: {:#}", e);
                None
            }
        }
    } else {
        None
    };

    let ping_count = parse_result.pings.len();

    if summary_only {
        parse_result.pings.clear();
    } else if let Some(n) = first_n {
        parse_result.pings.truncate(n);
        for (i, p) in parse_result.pings.iter().enumerate() {
            eprintln!(
                "PING {i}: ch={} seq={} t_ms={} depth_m={:.2} depth_ft={:.2} samples={} lat={} lon={}",
                p.channel,
                p.sequence,
                p.timestamp_ms,
                p.depth_m,
                p.depth_ft,
                p.sample_count,
                p.latitude,
                p.longitude,
            );
        }
    }

    // Output JSON summary
    let output = serde_json::json!({
        "input_file": file_name,
        "format": detected.format.to_string(),
        "record_count": parse_result.record_count,
        "ping_count": ping_count,
        "channels": parse_result.channels,
        "outputs": output_summary,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()));
}
