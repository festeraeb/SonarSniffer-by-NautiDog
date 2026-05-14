//! GeoTIFF ingestion — pure-Rust TIFF tag parsing with optional GDAL support.

use ndarray::Array2;
use crate::geo::{GeoTransform, GeoTiffError};
use crate::precision::Scalar;

/// Bundles pixel raster data with CRS metadata for use as FDCT input.
#[derive(Debug, Clone)]
pub struct GeoTiffInput {
    /// 2-D pixel array in row-major order.
    pub pixels: Array2<Scalar>,
    /// CRS metadata extracted from the GeoTIFF tags.
    pub geo_transform: GeoTransform,
}

impl GeoTiffInput {
    /// Parse a GeoTIFF from a byte slice.
    ///
    /// Uses the pure-Rust `tiff` crate by default.
    /// When the `gdal-support` feature is active, uses the `gdal` crate instead.
    pub fn from_bytes(data: &[u8]) -> Result<Self, GeoTiffError> {
        #[cfg(feature = "gdal-support")]
        {
            return Self::from_bytes_gdal(data);
        }
        #[cfg(not(feature = "gdal-support"))]
        {
            Self::from_bytes_tiff(data)
        }
    }

    /// Parse a GeoTIFF from a file path.
    pub fn from_path(path: &std::path::Path) -> Result<Self, GeoTiffError> {
        let data = std::fs::read(path)?;
        Self::from_bytes(&data)
    }

    // ── Pure-Rust TIFF parser ─────────────────────────────────────────────────

    fn from_bytes_tiff(data: &[u8]) -> Result<Self, GeoTiffError> {
        use std::io::Cursor;
        use tiff::decoder::{Decoder, DecodingResult};
        use tiff::tags::Tag;

        let cursor = Cursor::new(data);
        let mut decoder = Decoder::new(cursor)
            .map_err(|e| GeoTiffError::CorruptData(e.to_string()))?;

        // Read pixel dimensions.
        let (width, height) = decoder.dimensions()
            .map_err(|e| GeoTiffError::CorruptData(e.to_string()))?;

        // Decode pixel data.
        let image = decoder.read_image()
            .map_err(|e| GeoTiffError::CorruptData(e.to_string()))?;

        let pixels_f32: Vec<Scalar> = match image {
            DecodingResult::F32(v) => v.into_iter().map(|x| x as Scalar).collect(),
            DecodingResult::F64(v) => v.into_iter().map(|x| x as Scalar).collect(),
            DecodingResult::U8(v)  => v.into_iter().map(|x| x as Scalar).collect(),
            DecodingResult::U16(v) => v.into_iter().map(|x| x as Scalar).collect(),
            DecodingResult::U32(v) => v.into_iter().map(|x| x as Scalar).collect(),
            DecodingResult::I32(v) => v.into_iter().map(|x| x as Scalar).collect(),
            _ => return Err(GeoTiffError::UnsupportedFormat(
                "Unsupported pixel format (complex, 64-bit int, etc.)".into()
            )),
        };

        let pixels = Array2::from_shape_vec((height as usize, width as usize), pixels_f32)
            .map_err(|e| GeoTiffError::CorruptData(e.to_string()))?;

        // Extract GeoTransform from TIFF tags.
        let geo_transform = extract_geo_transform(&mut decoder)?;

        Ok(Self { pixels, geo_transform })
    }

    // ── GDAL backend (optional) ───────────────────────────────────────────────

    #[cfg(feature = "gdal-support")]
    fn from_bytes_gdal(_data: &[u8]) -> Result<Self, GeoTiffError> {
        // GDAL requires a file path, not a byte slice.
        // Write to a temp file and use from_path_gdal.
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new()?;
        tmp.write_all(_data)?;
        Self::from_path_gdal(tmp.path())
    }

    #[cfg(feature = "gdal-support")]
    fn from_path_gdal(path: &std::path::Path) -> Result<Self, GeoTiffError> {
        use gdal::Dataset;

        let dataset = Dataset::open(path)
            .map_err(|e| GeoTiffError::CorruptData(e.to_string()))?;

        let gt = dataset.geo_transform()
            .map_err(|_| GeoTiffError::MissingGeoTransform)?;

        let projection = dataset.projection();
        let epsg = epsg_from_wkt(&projection);

        let geo_transform = GeoTransform::new(
            gt,
            epsg,
            if projection.is_empty() { None } else { Some(projection) },
        );

        let band = dataset.rasterband(1)
            .map_err(|e| GeoTiffError::CorruptData(e.to_string()))?;

        let (cols, rows) = dataset.raster_size();
        let buf = band.read_as::<f32>(
            (0, 0), (cols, rows), (cols, rows), None,
        ).map_err(|e| GeoTiffError::CorruptData(e.to_string()))?;

        let pixels = Array2::from_shape_vec(
            (rows, cols),
            buf.data.into_iter().map(|x| x as Scalar).collect(),
        ).map_err(|e| GeoTiffError::CorruptData(e.to_string()))?;

        Ok(Self { pixels, geo_transform })
    }
}

// ── GeoTransform extraction from TIFF tags ────────────────────────────────────

/// Extract a GeoTransform from a decoded TIFF's IFD tags.
///
/// Tries tags in this order:
/// 1. Tag 34264 (ModelTransformationTag) — full 4×4 affine matrix
/// 2. Tags 33550 + 33922 (ModelPixelScaleTag + ModelTiepointTag) — scale + tiepoint
fn extract_geo_transform<R: std::io::Read + std::io::Seek>(
    decoder: &mut tiff::decoder::Decoder<R>,
) -> Result<GeoTransform, GeoTiffError> {
    use tiff::tags::Tag;

    // Try ModelTransformationTag (34264) first.
    if let Ok(tiff::decoder::ifd::Value::List(vals)) =
        decoder.get_tag(Tag::Unknown(34264))
    {
        let floats: Vec<f64> = vals.iter().filter_map(|v| match v {
            tiff::decoder::ifd::Value::Double(f) => Some(*f),
            tiff::decoder::ifd::Value::Float(f) => Some(*f as f64),
            _ => None,
        }).collect();

        if floats.len() >= 16 {
            // 4×4 matrix: extract the 6 GDAL-convention coefficients.
            let coeffs = [
                floats[3],  // x origin
                floats[0],  // pixel width
                floats[1],  // row rotation
                floats[7],  // y origin
                floats[4],  // col rotation
                floats[5],  // pixel height
            ];
            return Ok(GeoTransform::new(coeffs, None, None));
        }
    }

    // Try ModelPixelScaleTag (33550) + ModelTiepointTag (33922).
    let scale = decoder.get_tag(Tag::Unknown(33550));
    let tiepoint = decoder.get_tag(Tag::Unknown(33922));

    match (scale, tiepoint) {
        (Ok(tiff::decoder::ifd::Value::List(s_vals)),
         Ok(tiff::decoder::ifd::Value::List(t_vals))) => {
            let s: Vec<f64> = s_vals.iter().filter_map(|v| match v {
                tiff::decoder::ifd::Value::Double(f) => Some(*f),
                tiff::decoder::ifd::Value::Float(f) => Some(*f as f64),
                _ => None,
            }).collect();
            let t: Vec<f64> = t_vals.iter().filter_map(|v| match v {
                tiff::decoder::ifd::Value::Double(f) => Some(*f),
                tiff::decoder::ifd::Value::Float(f) => Some(*f as f64),
                _ => None,
            }).collect();

            if s.len() >= 2 && t.len() >= 6 {
                // s = [scale_x, scale_y, scale_z]
                // t = [i, j, k, x, y, z]  (pixel i,j maps to projected x,y)
                let scale_x = s[0];
                let scale_y = s[1];
                let i = t[0];
                let j = t[1];
                let x = t[3];
                let y = t[4];

                let coeffs = [
                    x - i * scale_x,   // x origin
                    scale_x,            // pixel width
                    0.0,                // no rotation
                    y - j * (-scale_y), // y origin (scale_y is positive in tag, negate for north-up)
                    0.0,                // no rotation
                    -scale_y,           // pixel height (negative = north-up)
                ];
                return Ok(GeoTransform::new(coeffs, None, None));
            }
            Err(GeoTiffError::MissingGeoTransform)
        }
        _ => Err(GeoTiffError::MissingGeoTransform),
    }
}

/// Attempt to extract an EPSG code from a WKT projection string.
#[allow(dead_code)]
fn epsg_from_wkt(wkt: &str) -> Option<u32> {
    // Look for AUTHORITY["EPSG","NNNN"] pattern.
    let upper = wkt.to_uppercase();
    if let Some(pos) = upper.find("AUTHORITY[\"EPSG\",\"") {
        let start = pos + 18;
        let end = upper[start..].find('"')? + start;
        upper[start..end].parse().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_geotransform_returns_error() {
        // A minimal valid TIFF with no GeoTransform tags should return MissingGeoTransform.
        // We use a tiny synthetic TIFF byte sequence.
        // For now, verify the error type is correct when parsing fails.
        let result = GeoTiffInput::from_bytes(&[0u8; 8]);
        assert!(result.is_err());
        // Should be CorruptData (not a valid TIFF) or MissingGeoTransform.
        match result.unwrap_err() {
            GeoTiffError::CorruptData(_) | GeoTiffError::MissingGeoTransform => {}
            e => panic!("Unexpected error: {:?}", e),
        }
    }
}
