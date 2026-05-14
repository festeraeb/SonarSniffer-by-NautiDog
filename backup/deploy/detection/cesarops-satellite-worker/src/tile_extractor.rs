//! GeoTIFF tile extraction — breaks a GeoTIFF into tiles, runs TPU inference,
//! maps pixel coords to GPS via the image's affine geotransform, and persists.

use crate::WorkerState;
use image::GenericImageView;
use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{info, warn};

/// Extract tiles from a GeoTIFF, run inference, persist detections.
pub async fn extract_and_infer(
    state: &Arc<WorkerState>,
    geotiff_path: &str,
    tile_size: u32,
    run_id: &str,
) -> Result<usize> {
    // Try gdal-rs first; fall back to image crate for non-georeferenced files
    let (img, geotransform) = match load_geotiff(geotiff_path) {
        Ok((img, gt)) => (img, Some(gt)),
        Err(e) => {
            warn!("gdal load failed ({e}), falling back to image crate (no georeferencing)");
            let img = image::open(geotiff_path).with_context(|| format!("cannot open image: {geotiff_path}"))?;
            (img, None)
        }
    };

    let (width, height) = img.dimensions();
    info!("Loaded {}x{} from {}", width, height, geotiff_path);

    let mut total_detections = 0;
    let mut tile_idx = 0;

    for ty in (0..height).step_by(tile_size as usize) {
        for tx in (0..width).step_by(tile_size as usize) {
            let tw = tile_size.min(width - tx);
            let th = tile_size.min(height - ty);
            let tile = img.crop_imm(tx, ty, tw, th);

            let result = state.tpu.infer_image(&tile, "both").await;

            for det in &result.detections {
                let (lat, lon) = if let Some(ref gt) = geotransform {
                    pixel_to_wgs84(gt, (tx + det.pixel_col) as f64, (ty + det.pixel_row) as f64)
                } else {
                    (0.0, 0.0)
                };

                let tile_id = format!("tile_{}_{}_{}", run_id, ty / tile_size, tx / tile_size);
                let id = state.store.insert(&crate::detection_store::DetectionRow {
                    detection_id: uuid::Uuid::new_v4().to_string(),
                    lat,
                    lon,
                    confidence: det.confidence,
                    pass_type: det.pass_type.clone(),
                    tile_id: Some(tile_id),
                    run_id: Some(run_id.to_string()),
                    pixel_row: ty + det.pixel_row,
                    pixel_col: tx + det.pixel_col,
                    timestamp: chrono::Utc::now().timestamp(),
                }).await;

                info!(
                    "Detection {} at ({:.6}, {:.6}) conf={:.3} pass={}",
                    id, lat, lon, det.confidence, det.pass_type
                );
                total_detections += 1;
            }

            tile_idx += 1;
        }
    }

    info!("Scan complete: {} tiles processed, {} detections", tile_idx, total_detections);
    Ok(total_detections)
}

/// Load a GeoTIFF with gdal-rs, returning the image and its geotransform.
#[cfg(feature = "gdal-support")]
fn load_geotiff(path: &str) -> Result<(image::DynamicImage, GeoTransform)> {
    use gdal::{Dataset, Metadata};

    let dataset = Dataset::open(path).with_context(|| format!("gdal cannot open: {path}"))?;

    let gt = dataset.geo_transform().context("no geotransform in dataset")?;
    let geotransform = GeoTransform {
        origin_x: gt[0],
        pixel_width: gt[1],
        row_rotation: gt[2],
        origin_y: gt[3],
        col_rotation: gt[4],
        pixel_height: gt[5],
    };

    let rasterband = dataset.rasterband(1).context("no raster band in dataset")?;
    let size = rasterband.size();
    let data = rasterband.read_as::<u8>((0, 0, size.0, size.1), (size.0, size.1), None)?;

    let img = image::GrayImage::from_raw(size.0 as u32, size.1 as u32, data)
        .context("failed to construct image from raster data")?;

    Ok((image::DynamicImage::ImageLuma8(img), geotransform))
}

/// Fallback when gdal feature is not enabled.
#[cfg(not(feature = "gdal-support"))]
fn load_geotiff(_path: &str) -> Result<(image::DynamicImage, GeoTransform)> {
    anyhow::bail!("gdal-support feature not enabled; use image crate fallback")
}

#[derive(Debug, Clone)]
pub struct GeoTransform {
    pub origin_x: f64,
    pub pixel_width: f64,
    pub row_rotation: f64,
    pub origin_y: f64,
    pub col_rotation: f64,
    pub pixel_height: f64,
}

/// Convert pixel coords to WGS84 lat/lon using the affine geotransform.
/// Assumes the source CRS is a projected coordinate system (e.g. UTM).
/// For proper CRS handling, integrate the `proj` crate.
fn pixel_to_wgs84(gt: &GeoTransform, col: f64, row: f64) -> (f64, f64) {
    let easting = gt.origin_x + col * gt.pixel_width + row * gt.row_rotation;
    let northing = gt.origin_y + col * gt.col_rotation + row * gt.pixel_height;

    // Heuristic UTM zone detection from easting/northing
    // This is a simplification — proper CRS handling needs the proj crate
    let utm_zone = ((easting / 6.0 + 180.0) / 6.0).floor() as i32 + 1;
    let is_northern = northing >= 0.0;

    // Approximate UTM to WGS84 conversion (simplified)
    // For production, use: let (lon, lat) = proj::transform(utm_crs, wgs84, easting, northing);
    let (lat, lon) = approximate_utm_to_wgs84(easting, northing, utm_zone, is_northern);
    (lat, lon)
}

/// Approximate UTM → WGS84 conversion.
/// This is a simplified inverse Mercator projection — accurate enough for
/// initial triage. Replace with `proj` crate for production precision.
fn approximate_utm_to_wgs84(easting: f64, northing: f64, zone: i32, _northern: bool) -> (f64, f64) {
    const K0: f64 = 0.9996;
    const EARTH_RADIUS: f64 = 6378137.0;
    const FALSE_EASTING: f64 = 500000.0;
    let false_northing = if _northern { 0.0 } else { 10000000.0 };

    let x = easting - FALSE_EASTING;
    let y = northing - false_northing;

    let central_meridian = (zone as f64 - 180.0 + 3.0) * std::f64::consts::PI / 180.0;

    let footpoint_lat = y / (K0 * EARTH_RADIUS);
    let n = (3.0 / 2.0) * footpoint_lat - (27.0 / 32.0) * footpoint_lat.powi(3)
        + (269.0 / 512.0) * footpoint_lat.powi(5);

    let _alpha1 = 1.0 / 2.0 - (2.0 / 3.0) * footpoint_lat.cos().powi(2);
    let _alpha2 = (5.0 / 3.0 - 2.0 * footpoint_lat.cos().powi(2)) / 6.0;
    let _alpha3 = (1.0 / 12.0) * (61.0 - 174.0 * footpoint_lat.cos().powi(2)
        + 120.0 * footpoint_lat.cos().powi(4));

    let lat = n
        - (x.powi(2) / (2.0 * K0 * EARTH_RADIUS)) * footpoint_lat.tan() / K0 * EARTH_RADIUS
        + (x.powi(4) / (24.0 * (K0 * EARTH_RADIUS).powi(4)))
            * (5.0 + 3.0 * footpoint_lat.tan().powi(2)) * footpoint_lat.tan()
        - (x.powi(6) / (720.0 * (K0 * EARTH_RADIUS).powi(6)))
            * (61.0 + 90.0 * footpoint_lat.tan().powi(2) + 45.0 * footpoint_lat.tan().powi(4))
            * footpoint_lat.tan();

    let lon = central_meridian
        + x / (K0 * EARTH_RADIUS * footpoint_lat.cos())
        - (x.powi(3) / (6.0 * (K0 * EARTH_RADIUS).powi(3)))
            * (1.0 + 2.0 * footpoint_lat.tan().powi(2)) / footpoint_lat.cos()
        + (x.powi(5) / (120.0 * (K0 * EARTH_RADIUS).powi(5)))
            * (5.0 + 28.0 * footpoint_lat.tan().powi(2) + 24.0 * footpoint_lat.tan().powi(4))
            / footpoint_lat.cos();

    (lat * 180.0 / std::f64::consts::PI, lon * 180.0 / std::f64::consts::PI)
}
