//! GeoTransform — six-parameter affine mapping from pixel to projected coordinates.

use serde::{Deserialize, Serialize};
use crate::geo::GeoTiffError;

/// Six-parameter affine mapping from pixel (col, row) to projected (x, y).
///
/// Follows the GDAL GeoTransform convention:
/// ```text
///   x = coeffs[0] + col * coeffs[1] + row * coeffs[2]
///   y = coeffs[3] + col * coeffs[4] + row * coeffs[5]
/// ```
///
/// For a north-up image with no rotation:
/// - `coeffs[0]` = x (easting) of the upper-left corner
/// - `coeffs[1]` = pixel width (positive)
/// - `coeffs[2]` = 0.0 (no rotation)
/// - `coeffs[3]` = y (northing) of the upper-left corner
/// - `coeffs[4]` = 0.0 (no rotation)
/// - `coeffs[5]` = pixel height (negative for north-up)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoTransform {
    /// The six affine coefficients stored with f64 precision.
    pub coeffs: [f64; 6],
    /// EPSG code of the source CRS, if known (e.g. 4326 for WGS-84, 32617 for UTM zone 17N).
    pub epsg: Option<u32>,
    /// WKT projection string, if available.
    pub projection_wkt: Option<String>,
}

impl GeoTransform {
    /// Construct a GeoTransform from raw coefficients.
    pub fn new(coeffs: [f64; 6], epsg: Option<u32>, projection_wkt: Option<String>) -> Self {
        Self { coeffs, epsg, projection_wkt }
    }

    /// Convert pixel (row, col) to projected (x, y) in the source CRS.
    ///
    /// Uses the GDAL affine formula:
    /// ```text
    ///   x = coeffs[0] + col * coeffs[1] + row * coeffs[2]
    ///   y = coeffs[3] + col * coeffs[4] + row * coeffs[5]
    /// ```
    pub fn pixel_to_projected(&self, row: usize, col: usize) -> (f64, f64) {
        let c = &self.coeffs;
        let x = c[0] + col as f64 * c[1] + row as f64 * c[2];
        let y = c[3] + col as f64 * c[4] + row as f64 * c[5];
        (x, y)
    }

    /// Convert pixel (row, col) to WGS-84 (latitude, longitude).
    ///
    /// First converts to projected (x, y) via the affine transform, then
    /// reprojects to WGS-84 using the PROJ library.
    ///
    /// Returns `(latitude, longitude)` in decimal degrees.
    pub fn pixel_to_wgs84(&self, row: usize, col: usize) -> Result<(f64, f64), GeoTiffError> {
        let (x, y) = self.pixel_to_projected(row, col);
        self.projected_to_wgs84(x, y)
    }

    /// Convert WGS-84 (lat, lon) back to pixel (row, col).
    ///
    /// Used for round-trip verification. Returns fractional pixel coordinates.
    pub fn wgs84_to_pixel(&self, lat: f64, lon: f64) -> Result<(f64, f64), GeoTiffError> {
        let (x, y) = self.wgs84_to_projected(lat, lon)?;
        self.projected_to_pixel(x, y)
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Reproject (x, y) from the source CRS to WGS-84 (lat, lon).
    fn projected_to_wgs84(&self, x: f64, y: f64) -> Result<(f64, f64), GeoTiffError> {
        // If the source CRS is already WGS-84 (EPSG:4326), no reprojection needed.
        // x = longitude, y = latitude in geographic CRS.
        if self.epsg == Some(4326) || self.is_geographic_wgs84() {
            return Ok((y, x)); // (lat, lon)
        }

        // Use the proj crate for reprojection.
        self.reproject_to_wgs84(x, y)
    }

    /// Reproject WGS-84 (lat, lon) to the source CRS (x, y).
    fn wgs84_to_projected(&self, lat: f64, lon: f64) -> Result<(f64, f64), GeoTiffError> {
        if self.epsg == Some(4326) || self.is_geographic_wgs84() {
            return Ok((lon, lat)); // x = lon, y = lat
        }
        self.reproject_from_wgs84(lat, lon)
    }

    /// Convert projected (x, y) back to fractional pixel (row, col).
    ///
    /// Inverts the affine transform. Assumes no rotation (coeffs[2] == 0, coeffs[4] == 0)
    /// for the common north-up case; falls back to a general 2×2 matrix inverse otherwise.
    fn projected_to_pixel(&self, x: f64, y: f64) -> Result<(f64, f64), GeoTiffError> {
        let c = &self.coeffs;
        // General 2×2 inverse of [[c1, c2], [c4, c5]]
        let det = c[1] * c[5] - c[2] * c[4];
        if det.abs() < 1e-15 {
            return Err(GeoTiffError::ReprojectError(
                "Degenerate GeoTransform (zero determinant)".into(),
            ));
        }
        let dx = x - c[0];
        let dy = y - c[3];
        let col = (c[5] * dx - c[2] * dy) / det;
        let row = (c[1] * dy - c[4] * dx) / det;
        Ok((row, col))
    }

    /// Returns true if the CRS is WGS-84 geographic (no reprojection needed).
    fn is_geographic_wgs84(&self) -> bool {
        if let Some(wkt) = &self.projection_wkt {
            let wkt_upper = wkt.to_uppercase();
            return wkt_upper.contains("WGS_1984") || wkt_upper.contains("WGS84")
                || wkt_upper.contains("EPSG:4326");
        }
        false
    }

    /// Reproject (x, y) from source CRS to WGS-84 using the proj crate.
    fn reproject_to_wgs84(&self, x: f64, y: f64) -> Result<(f64, f64), GeoTiffError> {
        #[cfg(feature = "proj")]
        {
            use proj::Proj;
            let src_crs = self.source_crs_string()?;
            let converter = Proj::new_known_crs(&src_crs, "EPSG:4326", None)
                .map_err(|e| GeoTiffError::ReprojectError(e.to_string()))?;
            let (lon, lat) = converter.convert((x, y))
                .map_err(|e| GeoTiffError::ReprojectError(e.to_string()))?;
            return Ok((lat, lon));
        }
        #[cfg(not(feature = "proj"))]
        {
            // Without PROJ, we can only handle geographic CRS (degrees already).
            // For UTM or other projected CRS, install the proj feature.
            Err(GeoTiffError::ReprojectError(
                "Reprojection from non-geographic CRS requires the `proj` feature. \
                 Add `nauticuvs = { features = [\"proj\"] }` to your Cargo.toml.".into()
            ))
        }
    }

    /// Reproject WGS-84 (lat, lon) to source CRS using the proj crate.
    fn reproject_from_wgs84(&self, lat: f64, lon: f64) -> Result<(f64, f64), GeoTiffError> {
        #[cfg(feature = "proj")]
        {
            use proj::Proj;
            let src_crs = self.source_crs_string()?;
            let converter = Proj::new_known_crs("EPSG:4326", &src_crs, None)
                .map_err(|e| GeoTiffError::ReprojectError(e.to_string()))?;
            let (x, y) = converter.convert((lon, lat))
                .map_err(|e| GeoTiffError::ReprojectError(e.to_string()))?;
            return Ok((x, y));
        }
        #[cfg(not(feature = "proj"))]
        {
            Err(GeoTiffError::ReprojectError(
                "Reprojection from non-geographic CRS requires the `proj` feature.".into()
            ))
        }
    }

    /// Build a CRS string for the proj crate from EPSG code or WKT.
    fn source_crs_string(&self) -> Result<String, GeoTiffError> {
        if let Some(epsg) = self.epsg {
            return Ok(format!("EPSG:{}", epsg));
        }
        if let Some(wkt) = &self.projection_wkt {
            return Ok(wkt.clone());
        }
        Err(GeoTiffError::ReprojectError(
            "No EPSG code or WKT projection string available for reprojection".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_to_projected_north_up() {
        // North-up GeoTIFF: origin at (lon=-83.5, lat=42.5), 0.01 deg/pixel
        let gt = GeoTransform::new(
            [-83.5, 0.01, 0.0, 42.5, 0.0, -0.01],
            Some(4326),
            None,
        );
        let (x, y) = gt.pixel_to_projected(0, 0);
        assert!((x - (-83.5)).abs() < 1e-10);
        assert!((y - 42.5).abs() < 1e-10);

        let (x2, y2) = gt.pixel_to_projected(10, 5);
        assert!((x2 - (-83.45)).abs() < 1e-10);
        assert!((y2 - 42.4).abs() < 1e-10);
    }

    #[test]
    fn pixel_to_wgs84_already_geographic() {
        let gt = GeoTransform::new(
            [-83.5, 0.01, 0.0, 42.5, 0.0, -0.01],
            Some(4326),
            None,
        );
        let (lat, lon) = gt.pixel_to_wgs84(0, 0).unwrap();
        assert!((lat - 42.5).abs() < 1e-10);
        assert!((lon - (-83.5)).abs() < 1e-10);
    }

    #[test]
    fn round_trip_pixel_wgs84_pixel() {
        let gt = GeoTransform::new(
            [-83.5, 0.01, 0.0, 42.5, 0.0, -0.01],
            Some(4326),
            None,
        );
        let (lat, lon) = gt.pixel_to_wgs84(7, 3).unwrap();
        let (row2, col2) = gt.wgs84_to_pixel(lat, lon).unwrap();
        assert!((row2 - 7.0).abs() < 0.001, "row round-trip error: {}", (row2 - 7.0).abs());
        assert!((col2 - 3.0).abs() < 0.001, "col round-trip error: {}", (col2 - 3.0).abs());
    }
}


