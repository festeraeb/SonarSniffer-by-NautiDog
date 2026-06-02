//! Test binary for feature-based alignment
//! 
//! Usage: cargo run --bin test_alignment -- --input <path.to.RSD.file>

use anyhow::Result;
use clap::Parser;
use image::GrayImage;
use std::path::Path;
use sonarsniffer_lib::{garmin_rsd_parser::GarminRSDParser, mosaic::feature::*};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to RSD file to test
    #[arg(short, long)]
    input: String,
    
    /// Channel to test (4=port, 5=starboard)
    #[arg(short, long, default_value = "4")]
    channel: u32,
    
    /// Number of pings per tile (stacked into 2-D strips for feature detection)
    #[arg(short, long, default_value = "64")]
    tile_height: usize,

    /// Number of consecutive tile-pairs to test
    #[arg(short, long, default_value = "8")]
    tiles: usize,

    /// Skip this many channel pings before sampling (use mid-survey data)
    #[arg(long, default_value = "10000")]
    offset: usize,
    
    /// Verbose output
    #[arg(short, long, default_value = "false")]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    
    println!("🔍 SoundTiles Feature Alignment Test");
    println!("═══════════════════════════════════");
    println!("Input: {}", args.input);
    println!("Channel: {}", args.channel);
    println!(
        "Tiles: {} × {} pings, offset {}",
        args.tiles, args.tile_height, args.offset
    );
    println!();
    
    // Parse RSD file
    println!("📡 Parsing RSD file...");
    let mut parser = GarminRSDParser::new();
    let parsed = parser.parse_file(Path::new(&args.input));
    
    if let Some(err) = &parsed.error_message {
        anyhow::bail!("Parse error: {}", err);
    }
    
    println!("✅ Parsed {} records", parsed.record_count);
    println!("   Channels: {:?}", parsed.channel_counts.keys());
    println!();
    
    // Filter to requested channel
    let channel_pings: Vec<_> = parsed
        .pings
        .iter()
        .filter(|p| p.channel == args.channel)
        .skip(args.offset)
        .collect();
    
    if channel_pings.len() < args.tile_height * 2 {
        anyhow::bail!(
            "Need at least {} pings on ch{} after offset {} (got {})",
            args.tile_height * 2,
            args.channel,
            args.offset,
            channel_pings.len()
        );
    }
    
    println!(
        "📊 {} pings on channel {} (offset {})",
        channel_pings.len(),
        args.channel,
        args.offset
    );
    println!();
    
    let aligner = FeatureAligner::new()?;
    let detector = OrbDetector::default_detector()?;
    println!("🔧 FAST+BRIEF feature aligner ready");
    println!();
    
    let mut tile_images: Vec<GrayImage> = Vec::new();
    let step = args.tile_height;
    let need = (args.tiles + 1) * step;
    for start in (0..need).step_by(step) {
        if start + step > channel_pings.len() {
            break;
        }
        let tile = pings_to_tile(&channel_pings[start..start + step]);
        if args.verbose {
            let kps = detector.detect(&tile)?;
            println!("  Tile @{}: {}×{} · {} features", start, tile.width(), tile.height(), kps.len());
        }
        tile_images.push(tile);
    }
    
    if tile_images.len() < 2 {
        anyhow::bail!("Not enough tiles built ({})", tile_images.len());
    }
    
    println!("🔗 Tile-pair alignment (SoundTiles-style {}-ping stacks):", step);
    println!("──────────────────────────────");
    
    let mut success_count = 0;
    let mut total_inliers = 0;
    let mut total_matches = 0;
    let pairs = tile_images.len() - 1;
    
    for (i, window) in tile_images.windows(2).enumerate() {
        match aligner.align(&window[0], &window[1]) {
            Ok(result) => {
                let quality = if result.is_good() { "✅ GOOD" } else { "⚠️  POOR" };
                println!(
                    "  Tile {}→{}: {} inliers/{} ({:.1}%) {} roll={:.2}° scale={:.3}",
                    i,
                    i + 1,
                    result.inlier_count,
                    result.total_matches,
                    result.inlier_ratio * 100.0,
                    quality,
                    result.roll_deg,
                    result.scale
                );
                if result.is_good() {
                    success_count += 1;
                }
                total_inliers += result.inlier_count;
                total_matches += result.total_matches;
            }
            Err(e) => println!("  Tile {}→{}: ❌ {}", i, i + 1, e),
        }
    }
    
    println!();
    println!("═══════════════════════════════════");
    println!("📈 Summary:");
    println!("   Successful alignments: {}/{}", success_count, pairs);
    println!(
        "   Average inliers: {:.1}",
        if pairs > 0 {
            total_inliers as f64 / pairs as f64
        } else {
            0.0
        }
    );
    println!(
        "   Average match ratio: {:.1}%",
        if total_matches > 0 {
            total_inliers as f64 / total_matches as f64 * 100.0
        } else {
            0.0
        }
    );
    
    if pairs > 0 && success_count as f64 / pairs as f64 > 0.6 {
        println!();
        println!("🎉 Feature alignment is working well!");
        println!("   Ready for full mosaic processing.");
    } else {
        println!();
        println!("⚠️  Feature alignment needs tuning.");
        println!("   Try adjusting ORB parameters or RANSAC threshold.");
    }
    
    Ok(())
}

/// Stack consecutive pings into a 2-D tile (SoundTiles-style).
fn pings_to_tile(pings: &[&sonarsniffer_lib::garmin_rsd_parser::Ping]) -> GrayImage {
    if pings.is_empty() {
        return GrayImage::new(64, 64);
    }
    let width = pings
        .iter()
        .map(|p| p.samples.len())
        .max()
        .unwrap_or(1)
        .min(2048);
    let height = pings.len() as u32;
    let mut img = GrayImage::new(width as u32, height);
    for (y, ping) in pings.iter().enumerate() {
        if ping.samples.is_empty() {
            continue;
        }
        let max_sample = ping.samples.iter().copied().max().unwrap_or(1) as f32;
        for (x, &sample) in ping.samples.iter().take(width).enumerate() {
            let intensity = ((sample as f32 / max_sample) * 255.0) as u8;
            img.put_pixel(x as u32, y as u32, image::Luma([intensity]));
        }
    }
    img
}
