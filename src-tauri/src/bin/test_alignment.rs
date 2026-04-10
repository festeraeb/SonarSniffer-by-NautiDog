//! Test binary for feature-based alignment
//! 
//! Usage: cargo run --bin test_alignment -- --input <path.to.RSD.file>

use anyhow::Result;
use clap::Parser;
use image::GrayImage;
use std::path::Path;
use tauri_appsonarsniffer_lib::{garmin_rsd_parser::GarminRSDParser, mosaic::feature::*};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to RSD file to test
    #[arg(short, long)]
    input: String,
    
    /// Channel to test (4=port, 5=starboard)
    #[arg(short, long, default_value = "4")]
    channel: u32,
    
    /// Number of pings to test
    #[arg(short, long, default_value = "10")]
    count: usize,
    
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
    println!("Testing {} pings", args.count);
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
        .take(args.count)
        .collect();
    
    if channel_pings.is_empty() {
        anyhow::bail!("No pings found for channel {}", args.channel);
    }
    
    println!("📊 Found {} pings on channel {}", channel_pings.len(), args.channel);
    println!();
    
    // Create feature aligner
    println!("🔧 Initializing feature detector...");
    let aligner = FeatureAligner::new()?;
    println!("✅ ORB detector ready");
    println!();
    
    // Test feature detection on each ping
    println!("🎯 Testing feature detection:");
    println!("─────────────────────────────");
    
    for (i, ping) in channel_pings.iter().enumerate() {
        // Convert ping samples to image
        let image = ping_to_grayscale(ping);
        
        // Detect features
        let detector = OrbDetector::default_detector()?;
        let keypoints = detector.detect(&image)?;
        
        println!("  Ping {:3}: {} features (depth: {:.1}m, GPS: {:.4}, {:.4})", 
                 i, 
                 keypoints.len(),
                 ping.depth_m,
                 ping.latitude,
                 ping.longitude);
        
        if args.verbose && !keypoints.is_empty() {
            println!("           Top 3 features:");
            let mut sorted_kps = keypoints.clone();
            sorted_kps.sort_by(|a, b| b.response.partial_cmp(&a.response).unwrap_or(std::cmp::Ordering::Equal));
            
            for (j, kp) in sorted_kps.iter().take(3).enumerate() {
                println!("             {}: ({:.1}, {:.1}) size={:.1}, response={:.2}", 
                         j, kp.x, kp.y, kp.size, kp.response);
            }
        }
    }
    
    println!();
    
    // Test pair-wise alignment
    println!("🔗 Testing pair-wise alignment:");
    println!("──────────────────────────────");
    
    let mut success_count = 0;
    let mut total_inliers = 0;
    let mut total_matches = 0;
    
    for window in channel_pings.windows(2) {
        let img1 = ping_to_grayscale(window[0]);
        let img2 = ping_to_grayscale(window[1]);
        
        match aligner.align(&img1, &img2) {
            Ok(result) => {
                let quality = if result.is_good() { "✅ GOOD" } else { "⚠️  POOR" };
                println!("  Ping {}→{}: {} inliers/{} ({:.1}%) {} roll={:.2}°", 
                         window[0].sequence,
                         window[1].sequence,
                         result.inlier_count,
                         result.total_matches,
                         result.inlier_ratio * 100.0,
                         quality,
                         result.roll_deg);
                
                if result.is_good() {
                    success_count += 1;
                }
                total_inliers += result.inlier_count;
                total_matches += result.total_matches;
            }
            Err(e) => {
                println!("  Ping {}→{}: ❌ Failed - {}", 
                         window[0].sequence,
                         window[1].sequence,
                         e);
            }
        }
    }
    
    println!();
    println!("═══════════════════════════════════");
    println!("📈 Summary:");
    println!("   Successful alignments: {}/{}", success_count, channel_pings.len() - 1);
    println!("   Average inliers: {:.1}", 
             if channel_pings.len() > 1 { 
                 total_inliers as f64 / (channel_pings.len() - 1) as f64 
             } else { 
                 0.0 
             });
    println!("   Average match ratio: {:.1}%", 
             if total_matches > 0 {
                 total_inliers as f64 / total_matches as f64 * 100.0
             } else {
                 0.0
             });
    
    if success_count as f64 / (channel_pings.len() - 1) as f64 > 0.6 {
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

/// Convert ping samples to grayscale image for feature detection
fn ping_to_grayscale(ping: &tauri_appsonarsniffer_lib::garmin_rsd_parser::Ping) -> GrayImage {
    let samples = &ping.samples;
    
    if samples.is_empty() {
        // Return minimal image if no samples
        return GrayImage::new(100, 100);
    }
    
    // Create waterfall strip: width = samples, height = 1 (single ping line)
    // Stretch to make features more detectable
    let width = samples.len().min(2048);
    let height = 64;  // Stretch vertically for better feature detection
    
    let mut img = GrayImage::new(width as u32, height as u32);
    
    // Find max sample for normalization
    let max_sample = samples.iter().copied().max().unwrap_or(1) as f32;
    
    // Draw samples as horizontal strip
    for (x, &sample) in samples.iter().take(width).enumerate() {
        let intensity = ((sample as f32 / max_sample) * 255.0) as u8;
        
        // Fill vertical column
        for y in 0..height {
            img.put_pixel(x as u32, y as u32, image::Luma([intensity]));
        }
    }
    
    img
}
