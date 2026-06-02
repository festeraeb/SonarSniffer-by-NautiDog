# integrate/unmapped/laptopdump_programming_root/bridge_calibrate.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/bridge_calibrate.rs

## Rust source
```rust
//! Mackinac Bridge coordinate calibration diagnostic.
//!
//! This module provides diagnostic utilities for verifying coordinate
//! calibration of bridge imagery tiles. It performs read-only analysis
//! without modifying scan logic.
//!
//! Reference points (surveyed/GPS-verified):
//!   North tower anchor  : 45.81656 N, -84.72769 W
//!   South tower center  : 45.78633 N, -84.72705 W
//!   Bridge midpoint     : 45.80140 N, -84.72740 W
//!
//! Tests performed per tile:
//!   1. Report CRS, EPSG, pixel resolution, geotransform
//!   2. Forward: bridge lat/lon -> file CRS (easting/northing) -> row/col
//!   3. Backward: row/col -> file CRS -> lat/lon (exactly what scan does)
//!   4. Round-trip error reported in METRES
//!   5. Report what 3 km error in lat/lon looks like in pixel space

use std::path::Path;
use std::fmt;

use cesarops_common::error::CesaropsError;
use cesarops_common::logging::info;
use cesarops_common::util::haversine_m;

use geotiff::reader::Reader;
use geotiff::transform::Transform;
use proj::prelude::*;

/// Ground-truth reference points for Mackinac Bridge calibration.
const REF_POINTS: &[(&str, f64, f64)] = &[
    ("North tower anchor", 45.81656, -84.72769),
    ("South tower center", 45.78633, -84.72705),
    ("Bridge midpoint",    45.80140, -84.72740),
];

/// Diagnostic result for a single tile.
#[derive(Debug)]
pub struct TileDiagnostic {
    pub file_name: String,
    pub crs: Option<String>,
    pub epsg: Option<u32>,
    pub shape: (u32, u32),
    pub pixel_resolution: (f64, f64),
    pub geotransform: Transform,
    pub diagnostics: Vec<ReferenceDiagnostic>,
    pub error: Option<CesaropsError>,
}

/// Diagnostic result for a single reference point.
#[derive(Debug)]
pub struct ReferenceDiagnostic {
    pub label: String,
    pub ref_lat: f64,
    pub ref_lon: f64,
    pub native_x: f64,
    pub native_y: f64,
    pub row: Option<u32>,
    pub col: Option<u32>,
    pub px_x: Option<f64>,
    pub px_y: Option<f64>,
    pub lat_out: Option<f64>,
    pub lon_out: Option<f64>,
    pub error_meters: Option<f64>,
    pub dlat_meters: Option<f64>,
    pub dlon_meters: Option<f64>,
    pub pixels_for_3km: Option<f64>,
    pub in_bounds: bool,
}

/// Run calibration diagnostics on all GeoTIFF files in the given directory.
pub fn run_calibration(tiles_dir: &Path) -> Result<Vec<TileDiagnostic>, CesaropsError> {
    let tiles = tiles_dir
        .glob("*.tif")
        .filter_map(|p| p.ok())
        .filter(|p| p.is_file())
        .collect::<Vec<_>>();

    info!(
        "CESAROPS — MACKINAC BRIDGE COORDINATE CALIBRATION DIAGNOSTIC",
        tiles_dir = tiles_dir.display(),
        tile_count = tiles.len()
    );

    let mut results = Vec::new();

    for tiff_path in tiles {
        let file_name = tiff_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let diagnostic = match run_single_tile(&tiff_path) {
            Ok(d) => d,
            Err(e) => {
                info!("ERROR opening {}: {}", file_name, e);
                continue;
            }
        };

        results.push(diagnostic);
    }

    Ok(results)
}

/// Run diagnostics on a single GeoTIFF file.
fn run_single_tile(tiff_path: &Path) -> Result<TileDiagnostic, CesaropsError> {
    let mut reader = Reader::open(tiff_path)?;

    let crs_str = reader.crs()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "??".to_string());

    let epsg = reader.crs()
        .and_then(|c| c.to_epsg())
        .map(|e| e.to_string())
        .unwrap_or_else(|| "??".to_string());

    let shape = (reader.width(), reader.height());
    let pixel_resolution = (reader.transform.a.abs(), reader.transform.e.abs());
    let geotransform = reader.transform;

    let mut diagnostics = Vec::new();

    for (label, ref_lat, ref_lon) in REF_POINTS {
        let diagnostic = run_reference_point(
            &reader,
            *label,
            *ref_lat,
            *ref_lon,
            pixel_resolution.0,
        )?;

        diagnostics.push(diagnostic);
    }

    Ok(TileDiagnostic {
        file_name,
        crs: Some(crs_str),
        epsg: Some(epsg),
        shape,
        pixel_resolution,
        geotransform,
        diagnostics,
        error: None,
    })
}

/// Run diagnostics for a single reference point.
fn run_reference_point(
    reader: &Reader,
    label: &str,
    ref_lat: f64,
    ref_lon: f64,
    pixel_resolution_x: f64,
) -> Result<ReferenceDiagnostic, CesaropsError> {
    // Step 1: lat/lon -> file CRS native coords
    let (xs_native, ys_native) = warp_transform(
        "EPSG:4326",
        reader.crs().as_ref().unwrap(),
        vec![ref_lon],
        vec![ref_lat],
    )?;

    let x_nat = xs_native[0];
    let y_nat = ys_native[0];

    // Step 2: native coords -> pixel row/col
    let (row, col) = reader.index(x_nat, y_nat)?;

    let in_bounds = (0 <= row && row < reader.height())
        && (0 <= col && col < reader.width());

    let px_x = if in_bounds {
        Some(reader.xy(row, col).0)
    } else {
        None
    };

    let px_y = if in_bounds {
        Some(reader.xy(row, col).1)
    } else {
        None
    };

    let (lons_out, lats_out) = warp_transform(
        reader.crs().as_ref().unwrap(),
        "EPSG:4326",
        vec![px_x.unwrap_or(0.0)],
        vec![px_y.unwrap_or(0.0)],
    )?;

    let lat_out = lats_out[0];
    let lon_out = lons_out[0];

    let (dlat_meters, dlon_meters, error_meters) = if in_bounds {
        let (dlat, dlon) = (lat_out - ref_lat, lon_out - ref_lon);
        let dlat_m = dlat * 111000.0;
        let dlon_m = dlon * 77600.0;
        let total_error = haversine_m(ref_lat, ref_lon, lat_out, lon_out);
        (dlat_m, dlon_m, total_error)
    } else {
        (0.0, 0.0, 0.0)
    };

    let pixels_for_3km = if pixel_resolution_x > 0.0 {
        Some(3000.0 / pixel_resolution_x)
    } else {
        None
    };

    Ok(ReferenceDiagnostic {
        label: label.to_string(),
        ref_lat,
        ref_lon,
        native_x: x_nat,
        native_y: y_nat,
        row: if in_bounds { Some(row) } else { None },
        col: if in_bounds { Some(col) } else { None },
        px_x,
        px_y,
        lat_out: if in_bounds { Some(lat_out) } else { None },
        lon_out: if in_bounds { Some(lon_out) } else { None },
        error_meters: if in_bounds { Some(error_meters) } else { None },
        dlat_meters: if in_bounds { Some(dlat_meters) } else { None },
        dlon_meters: if in_bounds { Some(dlon_meters) } else { None },
        pixels_for_3km,
        in_bounds,
    })
}

/// Warp transform using proj.
fn warp_transform(
    from_crs: &str,
    to_crs: &proj::prelude::Crs,
    lons: Vec<f64>,
    lats: Vec<f64>,
) -> Result<(Vec<f64>, Vec<f64>), CesaropsError> {
    let from_crs = proj::prelude::Crs::from_epsg(from_crs.parse::<u32>().unwrap_or(4326))?;
    let transform = proj::prelude::Transform::from(from_crs, to_crs)?;
    let (x, y) = transform.transform(lons, lats)?;
    Ok((x, y))
}

/// Print diagnostic results for all tiles.
pub fn print_diagnostics(results: &[TileDiagnostic]) {
    println!("{}", "=" .repeat(72));
    println!("CESAROPS — MACKINAC BRIDGE COORDINATE CALIBRATION DIAGNOSTIC");
    println!("{}", "=" .repeat(72));

    for diagnostic in results {
        println!("\n{}", "─".repeat(60));
        println!("FILE:    {}", diagnostic.file_name);
        println!("  CRS:   {:?}  (EPSG:{:?})", diagnostic.crs, diagnostic.epsg);
        println!("  Shape: {} cols x {} rows  |  pixel {:.2f}m x {:.2f}m",
                 diagnostic.shape.0, diagnostic.shape.1,
                 diagnostic.pixel_resolution.0, diagnostic.pixel_resolution.1);
        println!("  Geotransform: {:?}", diagnostic.geotransform);

        for ref_diag in &diagnostic.diagnostics {
            println!("\n  [{}]  ref=({:.5f}, {:.5f})",
                     ref_diag.label, ref_diag.ref_lat, ref_diag.ref_lon);
            println!("    → native CRS:       x={:.3f}  y={:.3f}",
                     ref_diag.native_x, ref_diag.native_y);
            println!("    → pixel:            row={:?}  col={:?}",
                     ref_diag.row, ref_diag.col);
            println!("    → px center native: x={:?}  y={:?}",
                     ref_diag.px_x, ref_diag.px_y);
            println!("    → round-trip WGS84: lat={:?}  lon={:?}",
                     ref_diag.lat_out, ref_diag.lon_out);
            println!("    → ERROR:            Δlat={:?}m  Δlon={:?}m  total={:?}m  ({:.3f} km)",
                     ref_diag.dlat_meters, ref_diag.dlon_meters,
                     ref_diag.error_meters,
                     ref_diag.error_meters.map(|e| e / 1000.0).unwrap_or(0.0));
            println!("    [ref] 3 km = {:?} pixels ({:.0f}m pixel)",
                     ref_diag.pixels_for_3km, diagnostic.pixel_resolution.0);
        }
    }

    println!("\n{}", "=" .repeat(72));
    println!("NOTES:");
    println!("  Round-trip error > 1 pixel = geotransform or CRS mismatch");
    println!("  Round-trip error = 0m     = conversion is internally consistent");
    println!("  (Even 0m round-trip error can still mean the FILE has a geolocation");
    println!("   offset baked in — to catch that we need the optical bridge check below)");
    println!();
    println!("OPTICAL BRIDGE CHECK:");
    println!("  Open NIR/Red band, find the brightest straight-line feature near the bridge,");
    println!("  compare pixel location to ground-truth. That catches baked-in tile offsets.");
    println!("{}", "=" .repeat(72));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haversine_m() {
        // Test haversine distance calculation
        let dist = haversine_m(45.81656, -84.72769, 45.78633, -84.72705);
        assert!(dist > 0.0);
    }
}
```

## Forge wire
- **Pipeline diagnostic step**: Called from `cesarops-inference/src/pipeline/diagnostic.rs` after initial tile ingestion, before scan processing begins
- **CLI utility**: Exposed as `cargo run --bin bridge-calibrate -- --tiles-dir /path/to/tiles` for offline analysis
- **Integration test**: Used in `integration_tests/bridge_calibration.rs` to verify coordinate accuracy before deploying to production

## Risks
- **CRS parsing**: EPSG code parsing can fail for non-standard projections; fallback to EPSG:4326 may mask real issues
- **File I/O errors**: Missing or corrupted GeoTIFF files will be silently skipped; need better error reporting in production
- **Memory usage**: Large tile directories with many files may cause OOM; consider streaming or chunked processing
- **Projection accuracy**: The warp_transform function assumes standard PROJ behavior; edge cases with datum shifts may need special handling
- **Diagnostic output**: Verbose output may be overwhelming in CI/CD pipelines; consider adding --quiet flag for production use
