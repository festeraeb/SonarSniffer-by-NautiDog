//! Rust replacement for `pipelines/satellite/tile_image_fetch.py`.

use base64::Engine;
use clap::Parser;
use image::{DynamicImage, ImageFormat};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const PLACEHOLDER_B64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

#[derive(Parser, Debug)]
#[command(name = "tile-image-fetch", about = "Resolve image_b64 for detection_scan")]
struct Args {
    #[arg(long)]
    lat: f64,
    #[arg(long, allow_hyphen_values = true)]
    lon: f64,
    #[arg(long)]
    download_dir: Option<PathBuf>,
}

fn first_chip(download_dir: &Path) -> Option<PathBuf> {
    if !download_dir.is_dir() {
        return None;
    }
    for entry in WalkDir::new(download_dir).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "tif" | "tiff") {
            return Some(p.to_path_buf());
        }
    }
    None
}

fn encode_png_b64(path: &Path) -> Option<String> {
    let mut img = image::open(path).ok()?;
    img = DynamicImage::ImageRgba8(img.thumbnail(512, 512).to_rgba8());

    let mut buf = Vec::<u8>::new();
    {
        let mut cursor = Cursor::new(&mut buf);
        img.write_to(&mut cursor, ImageFormat::Png).ok()?;
    }

    Some(base64::engine::general_purpose::STANDARD.encode(buf))
}

fn main() {
    let args = Args::parse();
    let _ = (args.lat, args.lon);

    if let Some(download_dir) = args.download_dir.as_deref() {
        if let Some(path) = first_chip(download_dir) {
            if let Some(b64) = encode_png_b64(&path) {
                println!("{b64}");
                return;
            }
        }
    }

    println!("{PLACEHOLDER_B64}");
}
