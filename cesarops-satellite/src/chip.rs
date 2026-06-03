//! Chip geometry — pixel coordinate math, bounding boxes, radius calculations.
//!
//! Ports:
//!   `_chip_bbox`, `_haversine_m`, geometry from wh2k_sentinel_wreck_targeting.py
//!   `extract_chip_from_tif` structure from wh2k_chip_extractor.py

use crate::types::BBox;

pub const DEG_PER_M_LAT: f64 = 1.0 / 111_325.0;
pub const EARTH_R_M: f64 = 6_371_000.0;

// ── Distance ──────────────────────────────────────────────────────────────────

/// Haversine distance in metres between two lat/lon points.
pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let phi1 = lat1.to_radians();
    let phi2 = lat2.to_radians();
    let dphi = (lat2 - lat1).to_radians();
    let dlam = (lon2 - lon1).to_radians();
    let a = (dphi / 2.0).sin().powi(2)
        + phi1.cos() * phi2.cos() * (dlam / 2.0).sin().powi(2);
    2.0 * EARTH_R_M * a.sqrt().atan2((1.0 - a).sqrt())
}

/// Haversine distance in nautical miles (used by drift physics).
pub fn haversine_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    haversine_m(lat1, lon1, lat2, lon2) / 1852.0
}

// ── Chip bbox ─────────────────────────────────────────────────────────────────

/// Return a [lat_min, lon_min, lat_max, lon_max] bounding box that encloses
/// a circle of `radius_m` metres centred on `(lat, lon)`.
///
/// Mirrors Python `_chip_bbox()` in wh2k_sentinel_wreck_targeting.py.
pub fn chip_bbox(lat: f64, lon: f64, radius_m: f64) -> BBox {
    let dlat = radius_m * DEG_PER_M_LAT;
    let cos_lat = lat.to_radians().cos().max(1e-6);
    let dlon = radius_m * DEG_PER_M_LAT / cos_lat;
    BBox {
        lat_min: lat - dlat,
        lon_min: lon - dlon,
        lat_max: lat + dlat,
        lon_max: lon + dlon,
    }
}

// ── Pixel-radius helpers ──────────────────────────────────────────────────────

/// Convert a physical radius (metres) to pixels given a known pixel resolution (m/px).
pub fn radius_m_to_px(radius_m: f64, m_per_px: f64) -> usize {
    (radius_m / m_per_px).ceil() as usize
}

/// Estimate metres-per-pixel for a given scene at a centre latitude.
/// `pixel_spacing_deg` is the native resolution in degrees (e.g. 10m Sentinel = ~0.0000899°).
pub fn m_per_px_at(pixel_spacing_deg: f64, lat_deg: f64) -> f64 {
    let cos_lat = lat_deg.to_radians().cos().max(1e-6);
    // average of x and y spacing (in metres)
    let m_y = pixel_spacing_deg / DEG_PER_M_LAT;
    let m_x = pixel_spacing_deg / (DEG_PER_M_LAT * cos_lat);
    (m_x + m_y) / 2.0
}

// ── Annular mask ──────────────────────────────────────────────────────────────

/// Build inner (signal) and outer (background annulus) pixel masks for a chip
/// of shape (height, width) centred on (cy_px, cx_px).
///
/// Returns `(signal_mask, bg_mask)` as flat index lists.
///
/// Mirrors the annular background logic in wh2k_sentinel_wreck_targeting.py:
///   signal  = pixels within `inner_r_m` of centre
///   bg      = pixels between `bg_inner_r_m` and `bg_outer_r_m` of centre
pub fn annular_masks(
    height: usize,
    width: usize,
    cy: f64,
    cx: f64,
    m_per_px: f64,
    inner_r_m: f64,
    bg_inner_r_m: f64,
    bg_outer_r_m: f64,
) -> (Vec<usize>, Vec<usize>) {
    let inner_r_px = inner_r_m / m_per_px;
    let bg_in_px = bg_inner_r_m / m_per_px;
    let bg_out_px = bg_outer_r_m / m_per_px;

    let mut signal = Vec::new();
    let mut bg = Vec::new();

    for row in 0..height {
        for col in 0..width {
            let dy = row as f64 - cy;
            let dx = col as f64 - cx;
            let d = (dy * dy + dx * dx).sqrt();
            let idx = row * width + col;
            if d <= inner_r_px {
                signal.push(idx);
            } else if d >= bg_in_px && d <= bg_out_px {
                bg.push(idx);
            }
        }
    }
    (signal, bg)
}

// ── COG chip downloader ───────────────────────────────────────────────────────

/// Download a Cloud-Optimised GeoTIFF window for a bounding box.
/// Returns raw band data as `Vec<f32>` with dimensions `(height, width)`.
///
/// This is a pure-Rust fallback using raw HTTP range requests.  For full
/// CRS-aware reprojection, enable the `gdal` feature instead.
pub async fn download_cog_chip(
    client: &reqwest::Client,
    href: &str,
    bbox: &BBox,
    target_size_px: usize,
    cache_dir: &std::path::Path,
) -> anyhow::Result<ndarray::Array2<f32>> {
    // Cache key: hash of (href, bbox, size)
    let cache_key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        href.hash(&mut h);
        format!(
            "{:.5}_{:.5}_{:.5}_{:.5}",
            bbox.lat_min, bbox.lon_min, bbox.lat_max, bbox.lon_max
        )
        .hash(&mut h);
        target_size_px.hash(&mut h);
        format!("{:x}", h.finish())
    };

    let cache_path = cache_dir.join(format!("{cache_key}.npy_f32"));
    if cache_path.exists() {
        return load_f32_cache(&cache_path);
    }

    // Fetch raw TIFF bytes and decode first band with the `tiff` crate
    let bytes = client.get(href).send().await?.bytes().await?;
    let arr = decode_tiff_band(&bytes, bbox, target_size_px)?;

    // Persist to cache
    std::fs::create_dir_all(cache_dir)?;
    save_f32_cache(&cache_path, &arr)?;
    Ok(arr)
}

/// Decode a (single-band) GeoTIFF/COG tile from raw bytes into an
/// `Array2<f32>` resampled to `target_size_px × target_size_px`, applying the
/// Sentinel-2 DN→reflectance scaling used by Python `_download_band_chip`
/// (wh2k_sentinel_wreck_targeting.py).
///
/// The STAC layer already fetches one tile per bbox, so we decode the full
/// returned tile and resample (bilinear) instead of doing a CRS-aware windowed
/// read.  `_bbox` is therefore unused here (kept for signature stability and to
/// document the per-bbox fetch contract).
///
/// On decode failure (e.g. a tiled COG the pure-Rust `image` decoder cannot
/// handle) this returns an `Err` so the caller logs a real failure — it never
/// returns a NaN-filled placeholder.
fn decode_tiff_band(
    bytes: &[u8],
    _bbox: &BBox,
    target_size_px: usize,
) -> anyhow::Result<ndarray::Array2<f32>> {
    use image::DynamicImage;

    // Decode the TIFF bytes with the pure-Rust `image` crate (tiff feature).
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Tiff)
        .map_err(|e| {
            anyhow::anyhow!(
                "image crate could not decode TIFF (tiled COG / unsupported layout?): {e}"
            )
        })?;

    let src_w = img.width() as usize;
    let src_h = img.height() as usize;
    if src_w == 0 || src_h == 0 {
        anyhow::bail!("decoded TIFF has zero dimensions");
    }

    // Extract band-1 raw sample magnitudes as f32 (preserving the native DN
    // scale — we must NOT normalise to [0,1], unlike `to_luma32f`).
    let samples: Vec<f32> = match &img {
        DynamicImage::ImageLuma16(buf) => buf.as_raw().iter().map(|&v| v as f32).collect(),
        DynamicImage::ImageLumaA16(buf) => buf.as_raw().chunks_exact(2).map(|c| c[0] as f32).collect(),
        DynamicImage::ImageLuma8(buf) => buf.as_raw().iter().map(|&v| v as f32).collect(),
        DynamicImage::ImageLumaA8(buf) => buf.as_raw().chunks_exact(2).map(|c| c[0] as f32).collect(),
        DynamicImage::ImageRgb16(buf) => buf.as_raw().chunks_exact(3).map(|c| c[0] as f32).collect(),
        DynamicImage::ImageRgba16(buf) => buf.as_raw().chunks_exact(4).map(|c| c[0] as f32).collect(),
        DynamicImage::ImageRgb8(buf) => buf.as_raw().chunks_exact(3).map(|c| c[0] as f32).collect(),
        DynamicImage::ImageRgba8(buf) => buf.as_raw().chunks_exact(4).map(|c| c[0] as f32).collect(),
        DynamicImage::ImageRgb32F(buf) => buf.as_raw().chunks_exact(3).map(|c| c[0]).collect(),
        DynamicImage::ImageRgba32F(buf) => buf.as_raw().chunks_exact(4).map(|c| c[0]).collect(),
        // Fallback for any other / future layout: take a 16-bit luma view.
        _ => img.to_luma16().as_raw().iter().map(|&v| v as f32).collect(),
    };

    if samples.len() != src_w * src_h {
        anyhow::bail!(
            "decoded sample count {} != {}x{}",
            samples.len(),
            src_w,
            src_h
        );
    }

    // Resample (bilinear) the raw DN tile to the target chip size.
    let mut resampled = resample_bilinear(&samples, src_w, src_h, target_size_px, target_size_px);

    // Sentinel-2 DN→reflectance scaling (mirrors `_download_band_chip`):
    //   if nanmax(arr) > 1e4 → arr = arr / 10_000.0
    //   arr[arr <= 0] = NaN
    let nanmax = resampled
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(f32::NEG_INFINITY, f32::max);
    if nanmax > 1e4 {
        for v in resampled.iter_mut() {
            *v /= 10_000.0;
        }
    }
    for v in resampled.iter_mut() {
        if !(*v > 0.0) {
            // covers <= 0 and any non-finite (matches arr[arr <= 0] = NaN,
            // and treats nodata=0 as NaN like the Python reader).
            *v = f32::NAN;
        }
    }

    Ok(ndarray::Array2::from_shape_vec(
        (target_size_px, target_size_px),
        resampled,
    )?)
}

/// Bilinear resample of a flat `sw×sh` f32 grid to `tw×th`.
/// Used to fit a decoded COG tile onto the fixed chip grid.
fn resample_bilinear(
    src: &[f32],
    sw: usize,
    sh: usize,
    tw: usize,
    th: usize,
) -> Vec<f32> {
    if sw == 0 || sh == 0 || tw == 0 || th == 0 {
        return vec![f32::NAN; tw * th];
    }
    if sw == tw && sh == th {
        return src.to_vec();
    }
    let mut out = vec![0.0f32; tw * th];
    for ty in 0..th {
        let fy = if th == 1 {
            0.0
        } else {
            ty as f64 * (sh as f64 - 1.0) / (th as f64 - 1.0)
        };
        let y0 = fy.floor() as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let wy = fy - y0 as f64;
        for tx in 0..tw {
            let fx = if tw == 1 {
                0.0
            } else {
                tx as f64 * (sw as f64 - 1.0) / (tw as f64 - 1.0)
            };
            let x0 = fx.floor() as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let wx = fx - x0 as f64;

            let v00 = src[y0 * sw + x0] as f64;
            let v01 = src[y0 * sw + x1] as f64;
            let v10 = src[y1 * sw + x0] as f64;
            let v11 = src[y1 * sw + x1] as f64;
            let top = v00 * (1.0 - wx) + v01 * wx;
            let bot = v10 * (1.0 - wx) + v11 * wx;
            out[ty * tw + tx] = (top * (1.0 - wy) + bot * wy) as f32;
        }
    }
    out
}

fn save_f32_cache(path: &std::path::Path, arr: &ndarray::Array2<f32>) -> anyhow::Result<()> {
    use std::io::Write;
    let bytes: Vec<u8> = arr.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut f = std::fs::File::create(path)?;
    // Header: 4 bytes height, 4 bytes width
    let (h, w) = arr.dim();
    f.write_all(&(h as u32).to_le_bytes())?;
    f.write_all(&(w as u32).to_le_bytes())?;
    f.write_all(&bytes)?;
    Ok(())
}

fn load_f32_cache(path: &std::path::Path) -> anyhow::Result<ndarray::Array2<f32>> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut buf)?;
    if buf.len() < 8 {
        anyhow::bail!("corrupt cache: {}", path.display());
    }
    let h = u32::from_le_bytes(buf[0..4].try_into()?) as usize;
    let w = u32::from_le_bytes(buf[4..8].try_into()?) as usize;
    let floats: Vec<f32> = buf[8..]
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    Ok(ndarray::Array2::from_shape_vec((h, w), floats)?)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn haversine_known_point() {
        // Erie basin centre to a point ~10 km north
        let d = haversine_m(41.5, -81.0, 41.59, -81.0);
        assert_relative_eq!(d, 10_014.0, epsilon = 50.0);
    }

    #[test]
    fn chip_bbox_symmetry() {
        let bb = chip_bbox(42.0, -82.0, 600.0);
        assert!(bb.lat_max > bb.lat_min);
        assert!(bb.lon_max > bb.lon_min);
        let half_lat_m = haversine_m(42.0, -82.0, bb.lat_max, -82.0);
        assert_relative_eq!(half_lat_m, 600.0, epsilon = 5.0);
    }

    #[test]
    fn annular_masks_non_overlapping() {
        let (sig, bg) = annular_masks(100, 100, 50.0, 50.0, 10.0, 150.0, 350.0, 1200.0);
        // signal inner 150 m / 10 m per px = 15 px radius
        assert!(!sig.is_empty());
        assert!(!bg.is_empty());
        // No overlap
        let sig_set: std::collections::HashSet<_> = sig.iter().collect();
        for b in &bg {
            assert!(!sig_set.contains(b));
        }
    }

    #[test]
    fn resample_bilinear_identity() {
        let src = vec![1.0f32, 2.0, 3.0, 4.0];
        let out = resample_bilinear(&src, 2, 2, 2, 2);
        assert_eq!(out, src);
    }

    #[test]
    fn resample_bilinear_upsamples_corners() {
        // 2x2 → 3x3: corners preserved, centre is the mean.
        let src = vec![0.0f32, 0.0, 0.0, 4.0];
        let out = resample_bilinear(&src, 2, 2, 3, 3);
        assert_relative_eq!(out[0] as f64, 0.0, epsilon = 1e-6); // top-left
        assert_relative_eq!(out[8] as f64, 4.0, epsilon = 1e-6); // bottom-right
        assert_relative_eq!(out[4] as f64, 1.0, epsilon = 1e-6); // centre = mean(0,0,0,4)
    }

    #[test]
    fn decode_tiff_band_scales_dn_to_reflectance() {
        // Build a synthetic single-band 16-bit TIFF with DN values > 1e4 so the
        // DN→reflectance scaling (÷10000) in `_download_band_chip` kicks in, plus
        // a zero pixel that must become NaN.
        use image::{ImageBuffer, Luma};
        let mut img: ImageBuffer<Luma<u16>, Vec<u16>> = ImageBuffer::new(2, 2);
        img.put_pixel(0, 0, Luma([20000])); // → 2.0
        img.put_pixel(1, 0, Luma([10000])); // → 1.0
        img.put_pixel(0, 1, Luma([15000])); // → 1.5
        img.put_pixel(1, 1, Luma([0]));     // → NaN (<= 0)

        let mut bytes: Vec<u8> = Vec::new();
        image::DynamicImage::ImageLuma16(img)
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Tiff)
            .expect("encode synthetic tiff");

        let bb = BBox { lat_min: 0.0, lon_min: 0.0, lat_max: 1.0, lon_max: 1.0 };
        // Resample to the same 2x2 grid so values are unchanged by interpolation.
        let arr = decode_tiff_band(&bytes, &bb, 2).expect("decode");
        assert_eq!(arr.dim(), (2, 2));
        assert_relative_eq!(arr[[0, 0]] as f64, 2.0, epsilon = 1e-5);
        assert_relative_eq!(arr[[0, 1]] as f64, 1.0, epsilon = 1e-5);
        assert_relative_eq!(arr[[1, 0]] as f64, 1.5, epsilon = 1e-5);
        assert!(arr[[1, 1]].is_nan(), "zero DN must map to NaN");
    }

    #[test]
    fn decode_tiff_band_errors_on_garbage() {
        // Non-TIFF bytes must produce an Err (a real failure), never a NaN array.
        let bb = BBox { lat_min: 0.0, lon_min: 0.0, lat_max: 1.0, lon_max: 1.0 };
        let res = decode_tiff_band(b"not a tiff at all", &bb, 8);
        assert!(res.is_err(), "garbage bytes must return Err, not NaN placeholder");
    }
}

// ── Local GeoTIFF band loader (for on-disk Sentinel-2 tiles) ─────────────────

/// Decode a local single-band Sentinel-2 GeoTIFF into an `Array2<f32>` windowed
/// to `bbox` and resampled to `target_size_px`, applying the same DN→reflectance
/// scaling as `download_cog_chip`. Uses GDAL (handles full tiled COGs the pure-
/// Rust `image` decoder chokes on) and the dataset geotransform for an exact
/// CRS-aware window read.
///
/// This is the offline path: run the optical pipeline on tiles already pulled
/// to disk (no STAC query / network).
pub fn decode_local_band(
    path: &std::path::Path,
    bbox: &BBox,
    target_size_px: usize,
) -> anyhow::Result<ndarray::Array2<f32>> {
    use gdal::Dataset;
    let ds = Dataset::open(path)?;
    let (full_w, full_h) = ds.raster_size();
    let gt = ds.geo_transform()?; // [ox, px, 0, oy, 0, py] in the tile's CRS

    // Project the bbox lat/lon corners into the tile CRS to get a pixel window.
    // Sentinel-2 AWS tiles are UTM; reproject WGS84 bbox → tile SRS.
    let win = bbox_to_pixel_window(&ds, &gt, full_w, full_h, bbox).unwrap_or((0, 0, full_w, full_h));
    let (wx, wy, ww, wh) = win;
    if ww == 0 || wh == 0 {
        anyhow::bail!("empty window for bbox in {}", path.display());
    }

    let band = ds.rasterband(1)?;
    let buf = band.read_as::<f32>((wx as isize, wy as isize), (ww, wh), (ww, wh), None)?;
    let samples: Vec<f32> = buf.data().to_vec();

    // Resample window → target chip.
    let mut resampled = resample_bilinear(&samples, ww, wh, target_size_px, target_size_px);

    // DN→reflectance scaling (mirror decode_tiff_band).
    let nanmax = resampled.iter().copied().filter(|v| v.is_finite()).fold(f32::NEG_INFINITY, f32::max);
    if nanmax > 1e4 {
        for v in resampled.iter_mut() {
            *v /= 10_000.0;
        }
    }
    for v in resampled.iter_mut() {
        if !(*v > 0.0) {
            *v = f32::NAN;
        }
    }
    Ok(ndarray::Array2::from_shape_vec((target_size_px, target_size_px), resampled)?)
}

/// Compute a pixel window (x, y, w, h) in the dataset for a WGS84 bbox,
/// reprojecting the bbox corners into the dataset CRS via OSR.
pub fn bbox_to_pixel_window(
    ds: &gdal::Dataset,
    gt: &[f64; 6],
    full_w: usize,
    full_h: usize,
    bbox: &BBox,
) -> Option<(usize, usize, usize, usize)> {
    use gdal::spatial_ref::{AxisMappingStrategy, CoordTransform, SpatialRef};
    let dst = ds.spatial_ref().ok()?;
    let mut src = SpatialRef::from_epsg(4326).ok()?;
    src.set_axis_mapping_strategy(AxisMappingStrategy::TraditionalGisOrder);
    let mut dstm = dst;
    dstm.set_axis_mapping_strategy(AxisMappingStrategy::TraditionalGisOrder);
    let ct = CoordTransform::new(&src, &dstm).ok()?;

    // Reproject the 4 bbox corners (lon,lat) → tile CRS.
    let mut xs = [bbox.lon_min, bbox.lon_max, bbox.lon_min, bbox.lon_max];
    let mut ys = [bbox.lat_min, bbox.lat_min, bbox.lat_max, bbox.lat_max];
    let mut zs: [f64; 0] = [];
    ct.transform_coords(&mut xs, &mut ys, &mut zs).ok()?;

    // Map projected (x,y) → pixel via inverse affine (axis-aligned assumption).
    let px = gt[1];
    let py = gt[5];
    if px == 0.0 || py == 0.0 {
        return None;
    }
    let to_pix = |x: f64, y: f64| -> (f64, f64) {
        (((x - gt[0]) / px), ((y - gt[3]) / py))
    };
    let mut col_min = f64::INFINITY;
    let mut col_max = f64::NEG_INFINITY;
    let mut row_min = f64::INFINITY;
    let mut row_max = f64::NEG_INFINITY;
    for i in 0..4 {
        let (c, r) = to_pix(xs[i], ys[i]);
        col_min = col_min.min(c);
        col_max = col_max.max(c);
        row_min = row_min.min(r);
        row_max = row_max.max(r);
    }
    let cx0 = col_min.floor().max(0.0) as usize;
    let cy0 = row_min.floor().max(0.0) as usize;
    let cx1 = (col_max.ceil() as usize).min(full_w);
    let cy1 = (row_max.ceil() as usize).min(full_h);
    if cx1 <= cx0 || cy1 <= cy0 {
        return None;
    }
    Some((cx0, cy0, cx1 - cx0, cy1 - cy0))
}
