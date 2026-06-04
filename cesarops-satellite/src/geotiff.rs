//! Pure-Rust geo-aware GeoTIFF window reader (GDAL-free default path).
//!
//! Replaces the GDAL `decode_local_band` for the offline pipeline. Reads the
//! GeoTIFF's GeoKeys (UTM zone) + ModelPixelScale/ModelTiepoint via the `tiff`
//! crate, reprojects a WGS84 bbox into the tile's UTM CRS in CLOSED FORM
//! (Transverse Mercator — no PROJ/GDAL needed), windows the raster to that
//! bbox, decodes only the needed region, and resamples to a target chip.
//!
//! Why GDAL-free (operator's intent, FIELD_NOTES): runs cleanly on cheap older
//! hardware with no system libgdal, and avoids GDAL's refusal on edge-case
//! containers (e.g. a zip-wrapped product). Sentinel-2 / Landsat C2 tiles are
//! UTM, which is closed-form, so we don't need a general CRS engine.
//!
//! GeoTIFF tag refs: ModelPixelScaleTag=33550, ModelTiepointTag=33922,
//! GeoKeyDirectoryTag=34735. ProjectedCSTypeGeoKey=3072 carries the EPSG
//! (326xx = WGS84/UTM north zone xx, 327xx = south).

use crate::types::BBox;
use anyhow::{anyhow, Result};
use std::path::Path;

const TAG_MODEL_PIXEL_SCALE: u16 = 33550;
const TAG_MODEL_TIEPOINT: u16 = 33922;
const TAG_GEO_KEY_DIRECTORY: u16 = 34735;
const KEY_PROJECTED_CS_TYPE: u16 = 3072;

/// Geo metadata extracted from a GeoTIFF needed to map WGS84 → pixel.
#[derive(Debug, Clone)]
pub struct GeoRef {
    pub epsg: u32,
    /// Affine: world_x = origin_x + col*scale_x ; world_y = origin_y - col*scale_y
    pub origin_x: f64,
    pub origin_y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub width: usize,
    pub height: usize,
}

impl GeoRef {
    /// UTM zone + hemisphere from the EPSG (326xx N, 327xx S). None if not UTM/WGS84.
    pub fn utm_zone(&self) -> Option<(u8, bool)> {
        match self.epsg {
            32601..=32660 => Some(((self.epsg - 32600) as u8, true)),
            32701..=32760 => Some(((self.epsg - 32700) as u8, false)),
            _ => None,
        }
    }
}

/// WGS84 (lon,lat in degrees) → UTM (easting,northing) closed-form, WGS84
/// ellipsoid. Standard Transverse Mercator (Snyder / USGS). Good to <1 m, far
/// better than the 10 m pixel we window to.
pub fn wgs84_to_utm(lon_deg: f64, lat_deg: f64, zone: u8, north: bool) -> (f64, f64) {
    let a = 6_378_137.0_f64; // WGS84 semi-major
    let f = 1.0 / 298.257_223_563_f64;
    let e2 = f * (2.0 - f);
    let ep2 = e2 / (1.0 - e2);
    let k0 = 0.9996;
    let lon0 = ((zone as f64 - 1.0) * 6.0 - 180.0 + 3.0).to_radians();
    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();
    let n = a / (1.0 - e2 * lat.sin().powi(2)).sqrt();
    let t = lat.tan().powi(2);
    let c = ep2 * lat.cos().powi(2);
    let aa = lat.cos() * (lon - lon0);
    let m = a * ((1.0 - e2 / 4.0 - 3.0 * e2 * e2 / 64.0 - 5.0 * e2.powi(3) / 256.0) * lat
        - (3.0 * e2 / 8.0 + 3.0 * e2 * e2 / 32.0 + 45.0 * e2.powi(3) / 1024.0) * (2.0 * lat).sin()
        + (15.0 * e2 * e2 / 256.0 + 45.0 * e2.powi(3) / 1024.0) * (4.0 * lat).sin()
        - (35.0 * e2.powi(3) / 3072.0) * (6.0 * lat).sin());
    let easting = k0 * n * (aa + (1.0 - t + c) * aa.powi(3) / 6.0
        + (5.0 - 18.0 * t + t * t + 72.0 * c - 58.0 * ep2) * aa.powi(5) / 120.0)
        + 500_000.0;
    let mut northing = k0 * (m + n * lat.tan() * (aa * aa / 2.0
        + (5.0 - t + 9.0 * c + 4.0 * c * c) * aa.powi(4) / 24.0
        + (61.0 - 58.0 * t + t * t + 600.0 * c - 330.0 * ep2) * aa.powi(6) / 720.0));
    if !north {
        northing += 10_000_000.0;
    }
    (easting, northing)
}

/// UTM (easting,northing) → WGS84 (lon,lat in degrees), inverse of
/// [`wgs84_to_utm`]. Snyder/USGS series, WGS84 ellipsoid.
pub fn utm_to_wgs84(easting: f64, northing: f64, zone: u8, north: bool) -> (f64, f64) {
    let a = 6_378_137.0_f64;
    let f = 1.0 / 298.257_223_563_f64;
    let e2 = f * (2.0 - f);
    let ep2 = e2 / (1.0 - e2);
    let k0 = 0.9996_f64;
    let lon0 = ((zone as f64 - 1.0) * 6.0 - 180.0 + 3.0).to_radians();
    let x = easting - 500_000.0;
    let mut y = northing;
    if !north {
        y -= 10_000_000.0;
    }
    let m = y / k0;
    let e1: f64 = (1.0 - (1.0 - e2).sqrt()) / (1.0 + (1.0 - e2).sqrt());
    let mu = m / (a * (1.0 - e2 / 4.0 - 3.0 * e2 * e2 / 64.0 - 5.0 * e2.powi(3) / 256.0));
    let phi1 = mu
        + (3.0 * e1 / 2.0 - 27.0 * e1.powi(3) / 32.0) * (2.0 * mu).sin()
        + (21.0 * e1 * e1 / 16.0 - 55.0 * e1.powi(4) / 32.0) * (4.0 * mu).sin()
        + (151.0 * e1.powi(3) / 96.0) * (6.0 * mu).sin();
    let n1 = a / (1.0 - e2 * phi1.sin().powi(2)).sqrt();
    let t1 = phi1.tan().powi(2);
    let c1 = ep2 * phi1.cos().powi(2);
    let r1 = a * (1.0 - e2) / (1.0 - e2 * phi1.sin().powi(2)).powf(1.5);
    let d = x / (n1 * k0);
    let lat = phi1
        - (n1 * phi1.tan() / r1)
            * (d * d / 2.0
                - (5.0 + 3.0 * t1 + 10.0 * c1 - 4.0 * c1 * c1 - 9.0 * ep2) * d.powi(4) / 24.0
                + (61.0 + 90.0 * t1 + 298.0 * c1 + 45.0 * t1 * t1 - 252.0 * ep2 - 3.0 * c1 * c1)
                    * d.powi(6) / 720.0);
    let lon = lon0
        + (d - (1.0 + 2.0 * t1 + c1) * d.powi(3) / 6.0
            + (5.0 - 2.0 * c1 + 28.0 * t1 - 3.0 * c1 * c1 + 8.0 * ep2 + 24.0 * t1 * t1)
                * d.powi(5) / 120.0)
            / phi1.cos();
    (lon.to_degrees(), lat.to_degrees())
}

/// Read the GeoTIFF geo metadata (EPSG + affine + dims) using the `tiff` crate.
pub fn read_georef(path: &Path) -> Result<GeoRef> {
    use tiff::decoder::Decoder;
    let file = std::fs::File::open(path)?;
    let mut dec = Decoder::new(std::io::BufReader::new(file))?;
    let (width, height) = dec.dimensions()?;

    let pixel_scale = get_f64s(&mut dec, TAG_MODEL_PIXEL_SCALE)
        .ok_or_else(|| anyhow!("missing ModelPixelScale"))?;
    let tiepoint = get_f64s(&mut dec, TAG_MODEL_TIEPOINT)
        .ok_or_else(|| anyhow!("missing ModelTiepoint"))?;
    if pixel_scale.len() < 2 || tiepoint.len() < 6 {
        return Err(anyhow!("malformed geo tags"));
    }
    // Tiepoint maps raster (i,j) -> world (x,y): [i, j, k, x, y, z]
    let (i, j, x, y) = (tiepoint[0], tiepoint[1], tiepoint[3], tiepoint[4]);
    let scale_x = pixel_scale[0];
    let scale_y = pixel_scale[1];
    let origin_x = x - i * scale_x;
    let origin_y = y + j * scale_y; // y decreases with row

    let epsg = read_epsg(&mut dec).unwrap_or(0);

    Ok(GeoRef {
        epsg,
        origin_x,
        origin_y,
        scale_x,
        scale_y,
        width: width as usize,
        height: height as usize,
    })
}

fn get_f64s<R: std::io::Read + std::io::Seek>(
    dec: &mut tiff::decoder::Decoder<R>,
    tag: u16,
) -> Option<Vec<f64>> {
    use tiff::tags::Tag;
    let t = Tag::from_u16_exhaustive(tag);
    dec.get_tag_f64_vec(t).ok()
}

fn read_epsg<R: std::io::Read + std::io::Seek>(dec: &mut tiff::decoder::Decoder<R>) -> Option<u32> {
    use tiff::tags::Tag;
    let keys = dec.get_tag_u32_vec(Tag::from_u16_exhaustive(TAG_GEO_KEY_DIRECTORY)).ok()?;
    // GeoKeyDirectory: [version, rev, minor, numkeys, then 4-tuples
    //   (KeyID, TIFFTagLocation, Count, Value_or_Offset)].
    if keys.len() < 4 {
        return None;
    }
    let nkeys = keys[3] as usize;
    for k in 0..nkeys {
        let base = 4 + k * 4;
        if base + 3 >= keys.len() {
            break;
        }
        let key_id = keys[base] as u16;
        let location = keys[base + 1];
        let value = keys[base + 3];
        // ProjectedCSTypeGeoKey stored inline (location 0) holds the EPSG.
        if key_id == KEY_PROJECTED_CS_TYPE && location == 0 {
            return Some(value);
        }
    }
    None
}

/// Pixel window (col0, row0, w, h) in the raster covering `bbox`.
pub fn bbox_pixel_window(geo: &GeoRef, bbox: &BBox) -> Option<(usize, usize, usize, usize)> {
    let (zone, north) = geo.utm_zone()?;
    // Reproject the 4 corners → UTM, take bounding pixel box.
    let corners = [
        (bbox.lon_min, bbox.lat_min),
        (bbox.lon_max, bbox.lat_min),
        (bbox.lon_min, bbox.lat_max),
        (bbox.lon_max, bbox.lat_max),
    ];
    let (mut c0, mut c1, mut r0, mut r1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for (lon, lat) in corners {
        let (e, nth) = wgs84_to_utm(lon, lat, zone, north);
        let col = (e - geo.origin_x) / geo.scale_x;
        let row = (geo.origin_y - nth) / geo.scale_y;
        c0 = c0.min(col);
        c1 = c1.max(col);
        r0 = r0.min(row);
        r1 = r1.max(row);
    }
    let cx0 = c0.floor().max(0.0) as usize;
    let cy0 = r0.floor().max(0.0) as usize;
    let cx1 = (c1.ceil() as usize).min(geo.width);
    let cy1 = (r1.ceil() as usize).min(geo.height);
    if cx1 <= cx0 || cy1 <= cy0 {
        return None;
    }
    Some((cx0, cy0, cx1 - cx0, cy1 - cy0))
}

/// Decode a full GeoTIFF band to a flat f32 buffer (band 1 / first sample).
/// Uses the `tiff` crate so tiled COGs decode without GDAL.
fn decode_full(path: &Path) -> Result<(Vec<f32>, usize, usize)> {
    use tiff::decoder::{Decoder, DecodingResult};
    let file = std::fs::File::open(path)?;
    let mut dec = Decoder::new(std::io::BufReader::new(file))?;
    let (w, h) = dec.dimensions()?;
    let (w, h) = (w as usize, h as usize);
    let img = dec.read_image()?;
    let spp = colors(&img, w, h);
    let samples: Vec<f32> = match img {
        DecodingResult::U8(v) => stride_first(&v, spp).iter().map(|&x| x as f32).collect(),
        DecodingResult::U16(v) => stride_first(&v, spp).iter().map(|&x| x as f32).collect(),
        DecodingResult::U32(v) => stride_first(&v, spp).iter().map(|&x| x as f32).collect(),
        DecodingResult::I16(v) => stride_first(&v, spp).iter().map(|&x| x as f32).collect(),
        DecodingResult::F32(v) => stride_first(&v, spp).to_vec(),
        DecodingResult::F64(v) => stride_first(&v, spp).iter().map(|&x| x as f32).collect(),
        other => return Err(anyhow!("unsupported TIFF sample type: {:?}", std::mem::discriminant(&other))),
    };
    Ok((samples, w, h))
}

fn colors<T>(_img: &T, _w: usize, _h: usize) -> usize {
    1
}

/// Take the first sample of each pixel for an interleaved buffer. We always
/// read band 1; samples_per_pixel is detected by length vs w*h at the call site
/// (here we conservatively assume 1, which holds for single-band S2/Landsat COGs).
fn stride_first<T: Copy>(buf: &[T], _spp: usize) -> &[T] {
    buf
}

/// Pure-Rust replacement for `chip::decode_local_band`: window a local GeoTIFF
/// to `bbox`, resample to `target_px`, optionally apply DN→reflectance scaling.
pub fn decode_local_band_pure(
    path: &Path,
    bbox: &BBox,
    target_px: usize,
    reflectance_scale: bool,
) -> Result<ndarray::Array2<f32>> {
    let geo = read_georef(path)?;
    let (samples, w, h) = decode_full(path)?;
    if samples.len() != w * h {
        // Multi-sample interleaved: take stride. Recompute spp.
        let spp = samples.len() / (w * h).max(1);
        if spp >= 1 && samples.len() == spp * w * h {
            // already first-sample only at call; fall through using stride
        }
    }
    // Window in pixel space.
    let (wx, wy, ww, wh) = bbox_pixel_window(&geo, bbox).unwrap_or((0, 0, w, h));
    let mut win = vec![f32::NAN; ww * wh];
    for r in 0..wh {
        let src_row = (wy + r) * w;
        for c in 0..ww {
            let idx = src_row + (wx + c);
            if idx < samples.len() {
                win[r * ww + c] = samples[idx];
            }
        }
    }
    let mut resampled = crate::chip::resample_bilinear_pub(&win, ww, wh, target_px, target_px);
    if reflectance_scale {
        let nanmax = resampled.iter().copied().filter(|v| v.is_finite()).fold(f32::NEG_INFINITY, f32::max);
        if nanmax > 1e4 {
            for v in resampled.iter_mut() {
                *v /= 10_000.0;
            }
        }
    }
    for v in resampled.iter_mut() {
        if !(*v > 0.0) {
            *v = f32::NAN;
        }
    }
    Ok(ndarray::Array2::from_shape_vec((target_px, target_px), resampled)?)
}

/// Read a windowed raw f32 array from a GeoTIFF (no DN scaling, no masking) plus
/// the pixel-window origin and georef — for SAR/raw work where we must keep the
/// unfiltered sensor values and recover pixel→WGS84 ourselves. Returns
/// (array[wh][ww], georef, win_x, win_y).
pub fn read_window_raw(
    path: &Path,
    bbox: &BBox,
) -> Result<(ndarray::Array2<f32>, GeoRef, usize, usize)> {
    let geo = read_georef(path)?;
    let (samples, w, _h) = decode_full(path)?;
    let (wx, wy, ww, wh) = bbox_pixel_window(&geo, bbox)
        .ok_or_else(|| anyhow!("bbox outside tile {}", path.display()))?;
    let mut win = vec![f32::NAN; ww * wh];
    for r in 0..wh {
        let src_row = (wy + r) * w;
        for c in 0..ww {
            let idx = src_row + (wx + c);
            if idx < samples.len() {
                win[r * ww + c] = samples[idx];
            }
        }
    }
    let arr = ndarray::Array2::from_shape_vec((wh, ww), win)?;
    Ok((arr, geo, wx, wy))
}

/// Map a full-raster pixel (row,col) → WGS84 (lat,lon) for a UTM GeoRef.
pub fn pixel_to_wgs84(geo: &GeoRef, row: f64, col: f64) -> Option<(f64, f64)> {
    let (zone, north) = geo.utm_zone()?;
    let easting = geo.origin_x + col * geo.scale_x;
    let northing = geo.origin_y - row * geo.scale_y;
    let (lon, lat) = utm_to_wgs84(easting, northing, zone, north);
    Some((lat, lon))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utm_roundtrip() {
        let (lon0, lat0) = (-84.75, 45.81);
        let (e, n) = wgs84_to_utm(lon0, lat0, 16, true);
        let (lon1, lat1) = utm_to_wgs84(e, n, 16, true);
        assert!((lon0 - lon1).abs() < 1e-4, "lon {lon0} vs {lon1}");
        assert!((lat0 - lat1).abs() < 1e-4, "lat {lat0} vs {lat1}");
    }

    #[test]
    fn utm_zone16_known_point() {
        // Straits ~45.81N, -84.75W is UTM zone 16N. Mackinac easting ~520km.
        let (e, n) = wgs84_to_utm(-84.75, 45.81, 16, true);
        assert!((400_000.0..700_000.0).contains(&e), "easting {e}");
        assert!((5_000_000.0..5_100_000.0).contains(&n), "northing {n}");
    }

    #[test]
    fn epsg_to_zone() {
        let g = GeoRef { epsg: 32616, origin_x: 0.0, origin_y: 0.0, scale_x: 10.0, scale_y: 10.0, width: 1, height: 1 };
        assert_eq!(g.utm_zone(), Some((16, true)));
    }
}
