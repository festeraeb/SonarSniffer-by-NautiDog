//! BAG file reader (GDAL).
//!
//! Ported from `bag_wreck_detector.py::BAGReader` (and the metadata reader in
//! `masking_scanner.py::BAGMetaReader`). The Python code reads the BAG HDF5
//! groups directly with h5py; here we go through GDAL (which has a native BAG
//! driver), so band 1 is elevation and band 2 (if present) is uncertainty.
//!
//! Behavior matched from the Python reference:
//!   * NODATA sentinel `1_000_000.0` -> NaN (`elevation[elevation >= NODATA-1] = nan`)
//!   * Downsample on read when the grid exceeds ~10M cells
//!     (`BAGReader.MAX_CELLS = 10_000_000`, `step = sqrt(total/MAX)+1`)
//!   * Resolution is scaled by the read step (`resolution_m *= step`)
//!   * Valid/total cell counts + depth min/max stats

use crate::types::{BagInfo, Knobs};
use gdal::Dataset;
use ndarray::Array2;
use std::path::Path;
use tracing::{info, warn};

/// Result of reading a BAG file: the elevation grid (NaN = nodata), an
/// optional uncertainty grid, and the parsed georeferencing info.
pub struct BagData {
    /// Elevation/depth grid, row-major (rows, cols). NoData -> NaN.
    pub elevation: Array2<f64>,
    /// Uncertainty grid (band 2) if present. NoData -> NaN.
    pub uncertainty: Option<Array2<f64>>,
    pub info: BagInfo,
}

/// Open a BAG file and read elevation (band 1) + uncertainty (band 2 if any).
///
/// Mirrors `BAGReader.read_bag`: downsample huge grids, NoData->NaN, gather stats.
pub fn read_bag(path: &str, knobs: &Knobs) -> Result<BagData, Box<dyn std::error::Error>> {
    let dataset = Dataset::open(path)?;
    let band1 = dataset.rasterband(1)?;
    let (full_cols, full_rows) = band1.size(); // gdal size() = (width, height) = (cols, rows)
    let total_cells = full_cols * full_rows;

    // ── Read-time downsample for huge grids (BAGReader.MAX_CELLS) ──
    let step = if total_cells > knobs.max_cells {
        // step = max(2, int(sqrt(total/MAX)) + 1)
        let s = ((total_cells as f64 / knobs.max_cells as f64).sqrt() as usize) + 1;
        s.max(2)
    } else {
        1
    };

    let read_cols = (full_cols + step - 1) / step;
    let read_rows = (full_rows + step - 1) / step;

    info!(
        "BAG {}: full {}x{} cells={}, read step={} -> {}x{}",
        path, full_rows, full_cols, total_cells, step, read_rows, read_cols
    );

    // GDAL resamples on read via the (window, window_size, shape) decimation.
    let elevation = read_band_to_array(&dataset, 1, full_cols, full_rows, read_cols, read_rows, knobs.nodata)?;

    // ── Uncertainty band (band 2) if present ──
    let has_uncert = dataset.raster_count() >= 2;
    let uncertainty = if has_uncert {
        match read_band_to_array(&dataset, 2, full_cols, full_rows, read_cols, read_rows, knobs.nodata) {
            Ok(u) => Some(u),
            Err(e) => {
                warn!("Failed to read uncertainty band 2: {e}");
                None
            }
        }
    } else {
        None
    };

    // ── Georeferencing ──
    let mut info = parse_geo_info(&dataset, path, (read_rows, read_cols), knobs)?;
    info.read_step = step;
    if step > 1 {
        info.resolution_m *= step as f64;
    }
    info.has_uncertainty = uncertainty.is_some();

    // ── Stats (BAGReader: valid count, depth min/max) ──
    let mut valid = 0usize;
    let mut dmin = f64::INFINITY;
    let mut dmax = f64::NEG_INFINITY;
    for &v in elevation.iter() {
        if v.is_finite() {
            valid += 1;
            dmin = dmin.min(v);
            dmax = dmax.max(v);
        }
    }
    info.valid_cell_count = valid;
    info.total_cell_count = elevation.len();
    info.depth_min = if valid > 0 { dmin } else { 0.0 };
    info.depth_max = if valid > 0 { dmax } else { 0.0 };

    Ok(BagData {
        elevation,
        uncertainty,
        info,
    })
}

/// Read one band, decimating on read, and map NoData -> NaN.
fn read_band_to_array(
    dataset: &Dataset,
    band_index: usize,
    full_cols: usize,
    full_rows: usize,
    read_cols: usize,
    read_rows: usize,
    nodata: f64,
) -> Result<Array2<f64>, Box<dyn std::error::Error>> {
    let band = dataset.rasterband(band_index)?;
    // read_as(window_origin, window_size_in_source, output_shape, resample)
    let buf = band.read_as::<f64>(
        (0, 0),
        (full_cols, full_rows),
        (read_cols, read_rows),
        None,
    )?;
    let data: Vec<f64> = buf.data().to_vec();

    // Band-level nodata (if GDAL reports one) OR the BAG sentinel.
    let band_nodata = band.no_data_value();
    let mut arr = Array2::<f64>::from_shape_vec((read_rows, read_cols), data)?;
    for v in arr.iter_mut() {
        let is_nodata = !v.is_finite()
            || *v >= nodata - 1.0
            || band_nodata.map(|nd| (*v - nd).abs() < 1e-6).unwrap_or(false);
        if is_nodata {
            *v = f64::NAN;
        }
    }
    Ok(arr)
}

/// Build [`BagInfo`] from the GDAL dataset's geo-transform + projection.
///
/// The Python `_parse_metadata` parses the BAG XML for corner points; GDAL's
/// geo-transform gives us the same affine, so SW corner = transform applied to
/// (col=0, row=rows) (bottom-left) and resolution = |transform[1]|.
fn parse_geo_info(
    dataset: &Dataset,
    path: &str,
    shape: (usize, usize),
    knobs: &Knobs,
) -> Result<BagInfo, Box<dyn std::error::Error>> {
    let (rows, cols) = shape;
    let gt = dataset.geo_transform()?; // [c, a, b, f, d, e]
    let resolution_m = gt[1].abs();

    // Pixel/line -> projected: x = c + col*a + row*b ; y = f + col*d + row*e
    let xy = |col: f64, row: f64| -> (f64, f64) {
        let x = gt[0] + col * gt[1] + row * gt[2];
        let y = gt[3] + col * gt[4] + row * gt[5];
        (x, y)
    };

    // BAG/GDAL origin is top-left (row 0). The Python BAGInfo stores the SW
    // (bottom-left) corner because its grid_to_utm adds +row*res. We expose
    // both corners; geo.rs uses the affine directly so orientation is exact.
    let (tl_e, tl_n) = xy(0.0, 0.0);
    let (br_e, br_n) = xy(cols as f64, rows as f64);
    let sw_easting = tl_e.min(br_e);
    let ne_easting = tl_e.max(br_e);
    let sw_northing = tl_n.min(br_n);
    let ne_northing = tl_n.max(br_n);

    // CRS WKT + EPSG via OSR.
    let (crs_wkt, epsg_code) = match dataset.spatial_ref() {
        Ok(sr) => {
            let wkt = sr.to_wkt().unwrap_or_default();
            let epsg = sr.auth_code().unwrap_or(0);
            (wkt, epsg)
        }
        Err(_) => (dataset.projection(), 0),
    };

    let basename = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string();
    let survey_id = if let Some(idx) = basename.find('_') {
        basename[..idx].to_string()
    } else {
        basename.trim_end_matches(".bag").to_string()
    };

    Ok(BagInfo {
        filepath: path.to_string(),
        survey_id,
        shape,
        sw_easting,
        sw_northing,
        ne_easting,
        ne_northing,
        resolution_m,
        crs_wkt,
        epsg_code,
        vertical_datum: "Unknown".to_string(),
        nodata_value: knobs.nodata,
        valid_cell_count: 0,
        total_cell_count: 0,
        depth_min: 0.0,
        depth_max: 0.0,
        has_uncertainty: false,
        read_step: 1,
    })
}

/// Fraction of nodata (NaN) cells, percent. Used in the MissionReport summary.
pub fn nodata_pct(elevation: &Array2<f64>) -> f64 {
    if elevation.is_empty() {
        return 0.0;
    }
    let nan = elevation.iter().filter(|v| !v.is_finite()).count();
    (nan as f64 / elevation.len() as f64) * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodata_pct_basic() {
        let mut a = Array2::<f64>::from_elem((2, 2), 1.0);
        a[[0, 0]] = f64::NAN;
        assert!((nodata_pct(&a) - 25.0).abs() < 1e-9);
    }
}
