use std::path::{Path, PathBuf};

use gdal::raster::RasterBand;
use gdal::Dataset;
use image::{GrayImage, Rgb, RgbImage};
use std::fs;

use crate::model::ExportCandidate;

const WIN: usize = 256;

pub fn attach_thumbnails(
    candidates: &mut [ExportCandidate],
    thumbs_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let tifs = list_geotiffs(thumbs_dir)?;
    if tifs.is_empty() {
        return Ok(());
    }
    let out_dir = thumbs_dir.join("_export_thumbs");
    fs::create_dir_all(&out_dir)?;
    for c in candidates.iter_mut() {
        if let Some(path) = pick_dataset(&tifs, c.lat, c.lon) {
            let png_name = format!("{}.png", sanitize_id(&c.id));
            let png_path = out_dir.join(&png_name);
            if render_thumbnail(&path, c.lat, c.lon, &png_path).is_ok() {
                c.thumb_png = Some(png_name);
            }
        }
    }
    Ok(())
}

fn list_geotiffs(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut paths = Vec::new();
    if !dir.is_dir() {
        return Ok(paths);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("tif") {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.contains("hillshade") || name.contains("recon") {
                paths.push(p);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn pick_dataset(tifs: &[PathBuf], lat: f64, lon: f64) -> Option<PathBuf> {
    for p in tifs {
        if let Ok(ds) = Dataset::open(p) {
            if geo_contains(&ds, lat, lon) {
                return Some(p.clone());
            }
        }
    }
    tifs.first().cloned()
}

fn geo_contains(ds: &Dataset, lat: f64, lon: f64) -> bool {
    let Ok(gt) = ds.geo_transform() else {
        return true;
    };
    let (x_size, y_size) = ds.raster_size();
    let px = (lon - gt[0]) / gt[1];
    let py = (lat - gt[3]) / gt[5];
    px >= 0.0 && py >= 0.0 && px < x_size as f64 && py < y_size as f64
}

fn render_thumbnail(
    tif_path: &Path,
    lat: f64,
    lon: f64,
    out_png: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let ds = Dataset::open(tif_path)?;
    let band: RasterBand = ds.rasterband(1)?;
    let (x_size, y_size) = ds.raster_size();
    let gt = ds.geo_transform()?;

    let (cx, cy) = lonlat_to_pixel(&gt, lon, lat);
    let x0 = (cx as isize - WIN as isize / 2).max(0) as usize;
    let y0 = (cy as isize - WIN as isize / 2).max(0) as usize;
    let x1 = (x0 + WIN).min(x_size);
    let y1 = (y0 + WIN).min(y_size);
    let w = x1 - x0;
    let h = y1 - y0;
    if w == 0 || h == 0 {
        return Ok(());
    }

    let buf = band.read_as::<f32>(
        (x0 as isize, y0 as isize),
        (w, h),
        (w, h),
        None,
    )?;
    let pixels = buf.data();
    let is_recon = tif_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.contains("recon"))
        .unwrap_or(false);

    if is_recon {
        let mut rgb = RgbImage::new(w as u32, h as u32);
        let (min_v, max_v) = min_max_f32(pixels);
        for (i, &v) in pixels.iter().enumerate() {
            let t = if max_v > min_v {
                ((v - min_v) / (max_v - min_v)).clamp(0.0, 1.0)
            } else {
                0.5
            };
            let (r, g, b) = viridis(t);
            let x = (i % w) as u32;
            let y = (i / w) as u32;
            rgb.put_pixel(x, y, Rgb([r, g, b]));
        }
        rgb.save(out_png)?;
    } else {
        let mut gray = GrayImage::new(w as u32, h as u32);
        let (min_v, max_v) = min_max_f32(pixels);
        for (i, &v) in pixels.iter().enumerate() {
            let t = if max_v > min_v {
                ((v - min_v) / (max_v - min_v)).clamp(0.0, 1.0)
            } else {
                0.5
            };
            let x = (i % w) as u32;
            let y = (i / w) as u32;
            gray.put_pixel(x, y, image::Luma([(t * 255.0) as u8]));
        }
        gray.save(out_png)?;
    }
    Ok(())
}

fn lonlat_to_pixel(gt: &[f64; 6], lon: f64, lat: f64) -> (usize, usize) {
    let px = ((lon - gt[0]) / gt[1]).round() as isize;
    let py = ((lat - gt[3]) / gt[5]).round() as isize;
    (px.max(0) as usize, py.max(0) as usize)
}

fn min_max_f32(data: &[f32]) -> (f32, f32) {
    let mut min_v = f32::INFINITY;
    let mut max_v = f32::NEG_INFINITY;
    for &v in data {
        if v.is_finite() {
            min_v = min_v.min(v);
            max_v = max_v.max(v);
        }
    }
    if !min_v.is_finite() {
        (0.0, 1.0)
    } else {
        (min_v, max_v)
    }
}

fn viridis(t: f32) -> (u8, u8, u8) {
    let r = (255.0 * (0.267 + t * (0.993 - 0.267))).clamp(0.0, 255.0) as u8;
    let g = (255.0 * (0.005 + t * (0.906 - 0.005))).clamp(0.0, 255.0) as u8;
    let b = (255.0 * (0.329 + t * (0.144 - 0.329)).clamp(0.0, 1.0)).clamp(0.0, 255.0) as u8;
    (r, g, b)
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}
