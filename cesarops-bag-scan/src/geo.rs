//! Coordinate transforms: grid -> projected (UTM) -> WGS84 lat/lon.
//!
//! CRITICAL FIX over the old stub: `main.rs::transform_to_geo` emitted raw
//! projected easting/northing and labelled them x/y. Detections therefore had
//! no real geographic position. Here we add the missing WGS84 reprojection
//! using GDAL's OSR (`SpatialRef` + `CoordTransform`), which is exactly what
//! `bag_wreck_detector.py::CoordinateTransformer` does with pyproj:
//!
//! ```python
//! transformer = pyproj.Transformer.from_crs(src_crs, EPSG:4326, always_xy=True)
//! lon, lat = transformer.transform(easting, northing)
//! ```
//!
//! grid -> projected mirrors `CoordinateTransformer.grid_to_utm`:
//! ```python
//! easting  = sw_easting  + col * resolution_m
//! northing = sw_northing + row * resolution_m
//! ```
//! but we prefer the dataset's full affine geo-transform when available so that
//! rotation/orientation are handled exactly (the SW-corner formula assumes an
//! axis-aligned, north-up grid).

use crate::types::BagInfo;
use gdal::spatial_ref::{AxisMappingStrategy, CoordTransform, SpatialRef};

/// Reprojects projected (easting, northing) coordinates to WGS84 lat/lon and
/// maps grid (row, col) cells onto the projected plane.
pub struct GeoTransformer {
    /// Affine geo-transform [c, a, b, f, d, e] (GDAL order). When `None`, the
    /// SW-corner formula from `BagInfo` is used instead.
    geo_transform: Option<[f64; 6]>,
    sw_easting: f64,
    sw_northing: f64,
    resolution_m: f64,
    /// Multiplier from our (possibly decimated) grid coords to FULL-resolution
    /// pixel/line coords. The dataset affine is for the full grid, but bag_io
    /// may have decimated by `read_step`, so a grid cell (row,col) corresponds
    /// to full pixel (row*scale, col*scale).
    grid_scale: f64,
    /// Source -> WGS84 transform. `None` if the CRS could not be resolved
    /// (then projected coords are passed through unchanged, as the Python
    /// fallback `return y, x` does).
    to_wgs84: Option<CoordTransform>,
}

impl GeoTransformer {
    /// Build from a parsed [`BagInfo`] (uses EPSG, falling back to WKT) and an
    /// optional dataset affine geo-transform. `grid_scale` is the read-time
    /// decimation factor (`BagInfo.read_step`).
    pub fn new(info: &BagInfo, geo_transform: Option<[f64; 6]>, grid_scale: usize) -> Self {
        let src = build_source_srs(info.epsg_code, &info.crs_wkt);
        let to_wgs84 = src.and_then(|s| make_wgs84_transform(s));
        GeoTransformer {
            geo_transform,
            sw_easting: info.sw_easting,
            sw_northing: info.sw_northing,
            resolution_m: info.resolution_m,
            grid_scale: grid_scale.max(1) as f64,
            to_wgs84,
        }
    }

    /// Test/standalone constructor from an explicit source EPSG code.
    pub fn from_epsg(src_epsg: u32, sw_easting: f64, sw_northing: f64, resolution_m: f64) -> Self {
        let to_wgs84 = SpatialRef::from_epsg(src_epsg)
            .ok()
            .and_then(make_wgs84_transform);
        GeoTransformer {
            geo_transform: None,
            sw_easting,
            sw_northing,
            resolution_m,
            grid_scale: 1.0,
            to_wgs84,
        }
    }

    /// True if a real reprojection to WGS84 is available.
    pub fn has_wgs84(&self) -> bool {
        self.to_wgs84.is_some()
    }

    /// grid (row, col) -> projected (easting, northing).
    /// Ported from `CoordinateTransformer.grid_to_utm`.
    pub fn grid_to_projected(&self, row: f64, col: f64) -> (f64, f64) {
        if let Some(gt) = self.geo_transform {
            // Scale decimated grid coords up to full-resolution pixel/line, at
            // cell centers (+0.5 in the full grid).
            let c = col * self.grid_scale + 0.5;
            let r = row * self.grid_scale + 0.5;
            let x = gt[0] + c * gt[1] + r * gt[2];
            let y = gt[3] + c * gt[4] + r * gt[5];
            (x, y)
        } else {
            let easting = self.sw_easting + col * self.resolution_m;
            let northing = self.sw_northing + row * self.resolution_m;
            (easting, northing)
        }
    }

    /// projected (easting, northing) -> WGS84 (lat, lon).
    /// Ported from `CoordinateTransformer.utm_to_latlon`.
    /// If no transform is available, returns (northing, easting) — the same
    /// degenerate fallback the Python `convert_to_latlon` uses (`return y, x`).
    pub fn projected_to_latlon(&self, easting: f64, northing: f64) -> (f64, f64) {
        match &self.to_wgs84 {
            Some(ct) => {
                // TraditionalGisOrder => x=lon, y=lat on both sides.
                let mut xs = [easting];
                let mut ys = [northing];
                let mut zs: [f64; 0] = [];
                match ct.transform_coords(&mut xs, &mut ys, &mut zs) {
                    Ok(()) => (ys[0], xs[0]), // (lat, lon)
                    Err(_) => (northing, easting),
                }
            }
            None => (northing, easting),
        }
    }

    /// grid (row, col) -> WGS84 (lat, lon).
    /// Ported from `CoordinateTransformer.grid_to_latlon`.
    pub fn grid_to_latlon(&self, row: f64, col: f64) -> (f64, f64) {
        let (e, n) = self.grid_to_projected(row, col);
        self.projected_to_latlon(e, n)
    }
}

/// Build the source SRS, preferring EPSG, then WKT. Returns None if neither
/// resolves (mirrors the Python try/except around `from_epsg` then WKT).
fn build_source_srs(epsg_code: i32, crs_wkt: &str) -> Option<SpatialRef> {
    if epsg_code > 0 {
        if let Ok(sr) = SpatialRef::from_epsg(epsg_code as u32) {
            return Some(sr);
        }
    }
    if !crs_wkt.is_empty() {
        if let Ok(sr) = SpatialRef::from_wkt(crs_wkt) {
            return Some(sr);
        }
    }
    None
}

/// Make a source -> WGS84 transform with traditional GIS axis order so that
/// `transform_coords` consumes (easting, northing) and yields (lon, lat).
fn make_wgs84_transform(mut src: SpatialRef) -> Option<CoordTransform> {
    let mut wgs84 = SpatialRef::from_epsg(4326).ok()?;
    // always_xy=True equivalent: force lon/lat ordering on both ends.
    src.set_axis_mapping_strategy(AxisMappingStrategy::TraditionalGisOrder);
    wgs84.set_axis_mapping_strategy(AxisMappingStrategy::TraditionalGisOrder);
    CoordTransform::new(&src, &wgs84).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known UTM -> WGS84 reprojection check.
    ///
    /// EPSG:32616 = WGS84 / UTM zone 16N. The false easting is 500_000 at the
    /// central meridian (-87 deg). So easting=500_000 must map to lon ~ -87,
    /// and northing=5_000_000 maps to lat ~ 45.14 deg N.
    #[test]
    fn utm16n_to_wgs84_known_point() {
        let gt = GeoTransformer::from_epsg(32616, 0.0, 0.0, 1.0);
        assert!(gt.has_wgs84(), "expected EPSG:32616 -> 4326 transform");

        let (lat, lon) = gt.projected_to_latlon(500_000.0, 5_000_000.0);
        assert!((lon - (-87.0)).abs() < 1e-4, "lon={lon} (want ~ -87.0)");
        assert!((lat - 45.142).abs() < 0.05, "lat={lat} (want ~ 45.14)");
    }

    /// A second known point off the central meridian to ensure easting offset
    /// produces a longitude shift in the right direction (east of CM).
    #[test]
    fn utm16n_east_of_central_meridian() {
        let gt = GeoTransformer::from_epsg(32616, 0.0, 0.0, 1.0);
        let (_lat, lon_cm) = gt.projected_to_latlon(500_000.0, 4_800_000.0);
        let (_lat2, lon_e) = gt.projected_to_latlon(600_000.0, 4_800_000.0);
        assert!(lon_e > lon_cm, "east offset should increase longitude: {lon_e} > {lon_cm}");
        assert!((lon_cm - (-87.0)).abs() < 1e-4);
    }

    #[test]
    fn grid_to_projected_sw_formula() {
        // No affine -> SW-corner formula: easting = sw + col*res.
        let gt = GeoTransformer::from_epsg(32616, 400_000.0, 4_900_000.0, 2.0);
        let (e, n) = gt.grid_to_projected(10.0, 5.0);
        assert_eq!(e, 400_000.0 + 5.0 * 2.0);
        assert_eq!(n, 4_900_000.0 + 10.0 * 2.0);
    }
}
