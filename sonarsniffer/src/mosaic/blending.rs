use crate::mosaic::grid::MosaicGrid;
use image::{ImageBuffer, Rgba};
use rusqlite::{Connection, Result};
use std::sync::Arc;
use std::path::Path;

const EARTH_CIRCUMFERENCE: f64 = 40075016.68;
const EARTH_HALF: f64 = 20037508.34;

fn meters_to_tile(x: f64, y: f64, zoom: u32) -> (u32, u32) {
    let scale = 1_u64 << zoom;
    let tile_size_meters = EARTH_CIRCUMFERENCE / (scale as f64);
    
    let col = ((x + EARTH_HALF) / tile_size_meters).floor() as u32;
    // MBTiles uses TMS, so Y origin is at the bottom (South)
    let row = ((y + EARTH_HALF) / tile_size_meters).floor() as u32;
    
    (col, row)
}

fn tile_bounds_meters(col: u32, row: u32, zoom: u32) -> (f64, f64, f64, f64) {
    let scale = 1_u64 << zoom;
    let tile_size_meters = EARTH_CIRCUMFERENCE / (scale as f64);
    
    let min_x = col as f64 * tile_size_meters - EARTH_HALF;
    let min_y = row as f64 * tile_size_meters - EARTH_HALF;
    let max_x = min_x + tile_size_meters;
    let max_y = min_y + tile_size_meters;
    
    (min_x, min_y, max_x, max_y)
}

pub fn export_mbtiles(grid: Arc<MosaicGrid>, zoom_levels: &[u32], output_path: &Path) -> Result<()> {
    let conn = Connection::open(output_path)?;
    
    // Initialize MBTiles schema
    conn.execute(
        "CREATE TABLE metadata (name text, value text);",
        [],
    )?;
    conn.execute(
        "CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);",
        [],
    )?;
    conn.execute(
        "CREATE UNIQUE INDEX tile_index on tiles (zoom_level, tile_column, tile_row);",
        [],
    )?;
    
    conn.execute("INSERT INTO metadata (name, value) VALUES ('name', 'Sonar Mosaic')", [])?;
    conn.execute("INSERT INTO metadata (name, value) VALUES ('type', 'overlay')", [])?;
    conn.execute("INSERT INTO metadata (name, value) VALUES ('version', '1.1')", [])?;
    conn.execute("INSERT INTO metadata (name, value) VALUES ('description', 'Processed with SonarSniffer Rust')", [])?;
    conn.execute("INSERT INTO metadata (name, value) VALUES ('format', 'png')", [])?;

    // Determine grid max intensity for normalization
    let mut max_val: f32 = 0.0;
    for px in 0..grid.width {
        for py in 0..grid.height {
            let val = grid.get_normalized_pixel(px, py);
            if val > max_val { max_val = val; }
        }
    }
    if max_val == 0.0 { max_val = 1.0; } // Avoid div by zero

    for &zoom in zoom_levels {
        let (min_col, min_row) = meters_to_tile(grid.min_x, grid.min_y, zoom);
        let (max_col, max_row) = meters_to_tile(grid.max_x, grid.max_y, zoom);

        for col in min_col..=max_col {
            for row in min_row..=max_row {
                let (t_min_x, t_min_y, t_max_x, t_max_y) = tile_bounds_meters(col, row, zoom);
                let tile_dx = (t_max_x - t_min_x) / 256.0;
                let tile_dy = (t_max_y - t_min_y) / 256.0;
                
                let mut img = ImageBuffer::new(256, 256);
                let mut has_data = false;

                for py in 0..256 {
                    for px in 0..256 {
                        // MBTiles draws from bottom up in standard TMS, but image buffer is top-down (0,0 is top-left)
                        let y_meters = t_max_y - (py as f64 * tile_dy);
                        let x_meters = t_min_x + (px as f64 * tile_dx);
                        
                        // Map back to grid integer indices
                        if x_meters >= grid.min_x && x_meters <= grid.max_x && y_meters >= grid.min_y && y_meters <= grid.max_y {
                            let gx = ((x_meters - grid.min_x) / grid.resolution) as usize;
                            let gy = ((y_meters - grid.min_y) / grid.resolution) as usize;
                            if gx < grid.width && gy < grid.height {
                                let val = grid.get_normalized_pixel(gx, gy);
                                if val > 0.0 {
                                    has_data = true;
                                    let norm = (val / max_val * 255.0).min(255.0) as u8;
                                    // Golden color map approx
                                    img.put_pixel(px as u32, py as u32, Rgba([norm, (norm as f32 * 0.8) as u8, (norm as f32 * 0.3) as u8, 255]));
                                }
                            }
                        }
                    }
                }

                if has_data {
                    let mut png_bytes: Vec<u8> = Vec::new();
                    let mut cursor = std::io::Cursor::new(&mut png_bytes);
                    img.write_to(&mut cursor, image::ImageFormat::Png).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))))?;
                    
                    conn.execute(
                        "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![zoom, col, row, png_bytes],
                    )?;
                }
            }
        }
    }
    
    Ok(())
}
pub fn export_kmz(
    grid: Arc<MosaicGrid>,
    _kml_path: &Path,
    kmz_path: &Path,
) -> Result<bool, anyhow::Error> {
    use image::{ImageBuffer, Rgba};
    use std::io::{Write, Cursor};
    use zip::write::SimpleFileOptions;
    
    // Determine bounds in lat/lon
    let (max_lat, min_lon) = crate::mosaic::projection::meters_to_latlon(grid.min_x, grid.min_y);
    let (min_lat, max_lon) = crate::mosaic::projection::meters_to_latlon(grid.max_x, grid.max_y);

    let max_dimension = grid.width.max(grid.height);
    let scale = if max_dimension > 4096 {
        4096.0 / max_dimension as f64
    } else {
        1.0
    };
    
    let out_w = (grid.width as f64 * scale) as u32;
    let out_h = (grid.height as f64 * scale) as u32;
    
    if out_w == 0 || out_h == 0 { return Ok(false); }

    let mut img = ImageBuffer::new(out_w, out_h);
    let mut max_val: f32 = 0.0;
    for px in 0..grid.width {
        for py in 0..grid.height {
            let val = grid.get_normalized_pixel(px, py);
            if val > max_val { max_val = val; }
        }
    }
    if max_val == 0.0 { max_val = 1.0; }

    let mut has_data = false;
    for y in 0..out_h {
        let gy = ((y as f64 / scale) as usize).min(grid.height - 1);
        for x in 0..out_w {
            let gx = ((x as f64 / scale) as usize).min(grid.width - 1);
            let val = grid.get_normalized_pixel(gx, gy);
            
            if val > 0.0 {
                has_data = true;
                let norm = (val / max_val * 255.0).min(255.0) as u8;
                // Render top-down (flip Y for KML standard overlay layout)
                img.put_pixel(x, out_h - 1 - y, Rgba([norm, (norm as f32 * 0.8) as u8, (norm as f32 * 0.3) as u8, 255]));
            }
        }
    }

    if !has_data { return Ok(false); }

    let mut png_bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)?;

    let kml_content = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <name>Sonar Mosaic</name>
    <GroundOverlay>
      <name>High-Res Sonar Overlay</name>
      <Icon>
        <href>overlay.png</href>
      </Icon>
      <LatLonBox>
        <north>{max_lat}</north>
        <south>{min_lat}</south>
        <east>{max_lon}</east>
        <west>{min_lon}</west>
        <rotation>0</rotation>
      </LatLonBox>
    </GroundOverlay>
  </Document>
</kml>"#, max_lat=max_lat, min_lat=min_lat, max_lon=max_lon, min_lon=min_lon);

    let file = std::fs::File::create(kmz_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("doc.kml", options)?;
    zip.write_all(kml_content.as_bytes())?;

    zip.start_file("overlay.png", options)?;
    zip.write_all(&png_bytes)?;

    zip.finish()?;
    Ok(true)
}

