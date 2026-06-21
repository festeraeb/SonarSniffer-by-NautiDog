//! SoundTiles — standalone mosaic feature-alignment CLI
//!
//! A sub-program of the SonarSniffer / CesarOPS infrastructure.
//!
//! Usage:
//!   soundtiles --input path/to/file.RSD [--channel AUTO] [--tiles 5] [--verbose]

// ── Pull source files from the sibling src-tauri crate without a library dep ──
#[path = "../../src/healing_api.rs"]
mod healing_api;

#[path = "../../src/garmin_rsd_parser.rs"]
mod garmin_rsd_parser;

#[path = "../../src/mosaic/feature.rs"]
mod feature;

use anyhow::Result;
use clap::Parser;
use image::GrayImage;
use std::path::Path;

use garmin_rsd_parser::{GarminRSDParser, Ping};
use feature::*;

// ─────────────────────────────────────────────────────────────────────────────
// Tile geometry constants
// ─────────────────────────────────────────────────────────────────────────────

/// Number of consecutive pings that form one 2-D sonar tile.
/// BRIEF descriptors need a ±12 px border (13 px) on each side.  With STEP=16
/// tiles the overlap zone falls at rows 0-15 (tile T+1) and TILE_HEIGHT-16 to
/// TILE_HEIGHT-1 (tile T).  `TILE_HEIGHT` must be large enough that those rows
/// are NOT inside the 13-px exclusion band, i.e. TILE_HEIGHT ≥ STEP + 26 = 42.
/// Using 64 gives a generous 35-row BRIEF-computable overlap region.
const TILE_HEIGHT: usize = 64;
/// Stride between tile starts (25 % overlap → more tiles per file for testing).
const TILE_STEP: usize = 16;

// ─────────────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name = "soundtiles",
    version,
    author = "NautiDog Sailing",
    about = "SoundTiles feature-alignment processor for Garmin RSD sonar files"
)]
struct Args {
    /// Path to RSD file to process
    #[arg(short, long)]
    input: String,

    /// Sonar channel to use (use 'auto' or omit to pick the first available)
    #[arg(short = 'C', long, default_value = "auto")]
    channel: String,

    /// Number of sonar tiles to test (each tile = 32 pings stacked as rows)
    #[arg(short = 'n', long, default_value = "5")]
    tiles: usize,

    /// Print per-keypoint details
    #[arg(short, long, default_value = "false")]
    verbose: bool,
}

// ─────────────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let args = Args::parse();

    println!("╔══════════════════════════════════════════╗");
    println!("║  SoundTiles Feature Alignment Processor  ║");
    println!("╚══════════════════════════════════════════╝");
    println!("  Input  : {}", args.input);
    println!("  Tiles  : {} × {} pings (step {})", args.tiles, TILE_HEIGHT, TILE_STEP);
    println!();

    // ── Parse RSD ──────────────────────────────────────────────────────────
    println!("📡 Parsing RSD file...");
    let mut parser = GarminRSDParser::new();
    let parsed = parser.parse_file(Path::new(&args.input));

    if let Some(e) = &parsed.error_message {
        anyhow::bail!("Parse error: {}", e);
    }

    println!(
        "✅ Parsed {} records  ({} channels)",
        parsed.record_count,
        parsed.channel_counts.len()
    );
    let ch_list: Vec<String> = parsed.channel_counts.keys().map(|k| k.to_string()).collect();
    println!("   Available channels: [{}]", ch_list.join(", "));
    println!();

    // ── Resolve channel ────────────────────────────────────────────────────
    let channel: u32 = if args.channel.eq_ignore_ascii_case("auto") {
        // Pick the channel with the most pings
        parsed
            .channel_counts
            .iter()
            .max_by_key(|(_, &v)| v)
            .map(|(&k, _)| k)
            .unwrap_or_else(|| {
                eprintln!("⚠️  No channels found in file.");
                std::process::exit(1);
            })
    } else {
        args.channel.parse::<u32>().unwrap_or_else(|_| {
            eprintln!("⚠️  Invalid channel '{}'. Use a number or 'auto'.", args.channel);
            std::process::exit(1);
        })
    };

    println!("  Channel: {} ({})", channel,
        if args.channel.eq_ignore_ascii_case("auto") { "auto-selected" } else { "user-specified" });
    println!();

    // ── Collect pings needed to fill the requested number of tiles ─────────
    let needed = TILE_HEIGHT + (args.tiles.saturating_sub(1)) * TILE_STEP;

    let channel_pings: Vec<&Ping> = parsed
        .pings
        .iter()
        .filter(|p| p.channel == channel)
        .take(needed)
        .collect();

    if channel_pings.len() < TILE_HEIGHT {
        eprintln!(
            "⚠️  Not enough pings for channel {} (need ≥ {}, got {}).",
            channel, TILE_HEIGHT, channel_pings.len()
        );
        eprintln!("   Try one of: [{}]", ch_list.join(", "));
        std::process::exit(1);
    }

    let n_tiles = ((channel_pings.len().saturating_sub(TILE_HEIGHT)) / TILE_STEP + 1).min(args.tiles);

    // Warn if samples are all zero (indicates parser/channel mismatch)
    let sample_sum: u64 = channel_pings.iter()
        .flat_map(|p| p.samples.iter())
        .map(|&s| s as u64)
        .sum();
    let sample_count: usize = channel_pings.iter().map(|p| p.samples.len()).sum();
    if sample_count == 0 || sample_sum == 0 {
        eprintln!("⚠️  All {} pings on channel {} have empty/zero samples.", channel_pings.len(), channel);
        eprintln!("   The parser may not support this channel's sample encoding.");
        eprintln!("   Feature detection will likely fail.");
        eprintln!();
    } else {
        let avg = sample_sum as f64 / sample_count as f64;
        println!(
            "📐 Sample stats: {} pings × avg {:.0} samples  mean={:.1}  max={}",
            channel_pings.len(),
            sample_count as f64 / channel_pings.len() as f64,
            avg,
            channel_pings.iter().flat_map(|p| p.samples.iter()).copied().max().unwrap_or(0)
        );
    }

    println!(
        "📊 Building {} tiles ({} pings per tile, {} step)",
        n_tiles, TILE_HEIGHT, TILE_STEP
    );
    println!();

    // ── Feature detection ──────────────────────────────────────────────────
    println!("🔍 Feature detection (FAST-12 + BRIEF):");
    println!("─────────────────────────────────────────");

    let aligner = FeatureAligner::new()?;
    let detector = OrbDetector::default_detector()?;

    let mut tile_images: Vec<GrayImage> = Vec::new();

    for i in 0..n_tiles {
        let start = i * TILE_STEP;
        let end = (start + TILE_HEIGHT).min(channel_pings.len());
        let tile_pings = &channel_pings[start..end];

        let first_ping = tile_pings.first().unwrap();
        let last_ping = tile_pings.last().unwrap();

        let img = pings_to_tile(tile_pings);
        let kps = detector.detect(&img)?;

        println!(
            "  Tile {:3} (pings {:5}-{:5}, {}×{}): {:4} features  depth={:.1}m  pos=({:.4}, {:.4})",
            i,
            first_ping.sequence,
            last_ping.sequence,
            img.width(),
            img.height(),
            kps.len(),
            first_ping.depth_m,
            first_ping.latitude,
            first_ping.longitude,
        );

        if args.verbose {
            let mut sorted = kps.clone();
            sorted.sort_by(|a, b| b.response.partial_cmp(&a.response)
                .unwrap_or(std::cmp::Ordering::Equal));
            for (j, kp) in sorted.iter().take(5).enumerate() {
                println!(
                    "             [{j}] ({:.1}, {:.1}) resp={:.2}",
                    kp.x, kp.y, kp.response
                );
            }
        }

        tile_images.push(img);
    }

    println!();

    // ── Pairwise alignment ─────────────────────────────────────────────────
    println!("🔗 Pairwise tile alignment (RANSAC homography):");
    println!("─────────────────────────────────────────────────");

    let (mut ok, mut fail) = (0usize, 0usize);
    let mut total_inliers = 0usize;
    let mut total_matches = 0usize;

    for i in 0..tile_images.len().saturating_sub(1) {
        let start_a = i * TILE_STEP;
        let start_b = ((i + 1) * TILE_STEP).min(channel_pings.len().saturating_sub(1));
        let seq_a = channel_pings[start_a].sequence;
        let seq_b = channel_pings[start_b].sequence;

        match aligner.align(&tile_images[i], &tile_images[i + 1]) {
            Ok(r) => {
                let tag = if r.is_good() { "✅ GOOD" } else { "⚠️  WEAK" };
                println!(
                    "  tile {}→{}: {} {:2}/{:2}  ({:.0}%)  roll={:+.1}°  err={:.2}px",
                    seq_a, seq_b, tag,
                    r.inlier_count, r.total_matches,
                    r.inlier_ratio * 100.0,
                    r.roll_deg, r.mean_error,
                );
                if r.is_good() { ok += 1; } else { fail += 1; }
                total_inliers += r.inlier_count;
                total_matches += r.total_matches;
            }
            Err(e) => {
                println!("  tile {}→{}: ❌ FAILED — {}", seq_a, seq_b, e);
                fail += 1;
            }
        }
    }

    let pairs = ok + fail;

    println!();
    println!("═══════════════════════════════════════════");
    println!("📈 Summary");
    println!("   Tile pairs tested : {}", pairs);
    println!("   Good alignments   : {} / {}", ok, pairs);
    if total_matches > 0 {
        println!(
            "   Avg inlier ratio  : {:.1}%",
            total_inliers as f64 / total_matches as f64 * 100.0
        );
    }

    if pairs > 0 && ok as f64 / pairs as f64 > 0.6 {
        println!();
        println!("🎉 Feature alignment is working well!");
        println!("   Ready for full mosaic processing.");
    } else if pairs > 0 {
        println!();
        println!("⚠️  Alignment quality is low.");
        if sample_sum == 0 {
            println!("   The channel samples appear to be zero — try a different channel.");
        } else {
            println!("   Try --tiles with a larger value, or use --verbose to inspect keypoints.");
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Build a 2-D sonar tile from a slice of consecutive pings.
//
// Each ping becomes ONE HORIZONTAL ROW in the output image (the sonar
// waterfall view).  Per-ping normalization removes TVG / gain variation
// so that FAST-12 sees real bottom-texture contrast rather than gain ramps.
// ─────────────────────────────────────────────────────────────────────────────

fn pings_to_tile(pings: &[&Ping]) -> GrayImage {
    if pings.is_empty() {
        return GrayImage::new(1, 1);
    }
    let width = pings.iter()
        .map(|p| p.samples.len())
        .max()
        .unwrap_or(0)
        .min(2048) as u32;
    let height = pings.len() as u32;

    if width == 0 {
        return GrayImage::new(1, height.max(1));
    }

    let mut img = GrayImage::new(width, height);

    for (y, ping) in pings.iter().enumerate() {
        if ping.samples.is_empty() {
            continue;
        }
        // Per-ping normalisation: map [0 .. max_sample] → [0 .. 255]
        let max_s = ping.samples.iter().copied().max().unwrap_or(1).max(1) as f32;
        for (x, &s) in ping.samples.iter().take(width as usize).enumerate() {
            let intensity = ((s as f32 / max_s) * 255.0) as u8;
            img.put_pixel(x as u32, y as u32, image::Luma([intensity]));
        }
    }

    img
}
