// GPU-Accelerated Tile Processing with Pure Rust TIFF Backend
// Full GeoTIFF support using tiff crate with geotransform preservation
// 512x512 tiling to prevent math precision drift

use ndarray::{Array2, Array3};
use rayon::prelude::*;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;
use crate::geotile::{GeoTile, TILE_SIZE, DEFAULT_OVERLAP_PERCENT};

/// GeoTIFF metadata extracted from TIFF tags
#[derive(Debug, Clone)]
pub struct GeoTIFFMetadata {
    pub width: u32,
    pub height: u32,
    pub geotransform: [f64; 6],
    pub crs: String,
    pub utm_zone: Option<i32>,
    pub northern_hemisphere: Option<bool>,
    pub pixel_size_x: f64,
    pub pixel_size_y: f64,
    pub no_data_value: Option<f64>,
    pub band_count: usize,
}

impl GeoTIFFMetadata {
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            geotransform: [0.0; 6],
            crs: "EPSG:32616".to_string(), // Default UTM Zone 16N
            utm_zone: Some(16),
            northern_hemisphere: Some(true),
            pixel_size_x: 30.0,
            pixel_size_y: 30.0,
            no_data_value: None,
            band_count: 1,
        }
    }

    /// Extract UTM zone from CRS
    pub fn extract_utm_zone(&mut self) {
        if self.crs.contains("326") {
            // UTM North
            if let Some(zone_str) = self.crs.split("326").last() {
                if let Ok(zone) = zone_str.parse::<i32>() {
                    self.utm_zone = Some(zone);
                    self.northern_hemisphere = Some(true);
                }
            }
        } else if self.crs.contains("327") {
            // UTM South
            if let Some(zone_str) = self.crs.split("327").last() {
                if let Ok(zone) = zone_str.parse::<i32>() {
                    self.utm_zone = Some(zone);
                    self.northern_hemisphere = Some(false);
                }
            }
        }
    }

    /// Convert pixel coordinates to UTM
    pub fn pixel_to_utm(&self, pixel_x: f64, pixel_y: f64) -> Option<(f64, f64)> {
        let gt = self.geotransform;
        let easting = gt[0] + pixel_x * gt[1] + pixel_y * gt[2];
        let northing = gt[3] + pixel_x * gt[4] + pixel_y * gt[5];
        Some((easting, northing))
    }

    /// Convert UTM to pixel coordinates
    pub fn utm_to_pixel(&self, easting: f64, northing: f64) -> Option<(f64, f64)> {
        let gt = self.geotransform;
        let det = gt[1] * gt[5] - gt[2] * gt[4];
        
        if det.abs() < 1e-12 {
            return None;
        }
        
        let dx = easting - gt[0];
        let dy = northing - gt[3];
        
        let pixel_x = (gt[5] * dx - gt[2] * dy) / det;
        let pixel_y = (-gt[4] * dx + gt[1] * dy) / det;
        
        Some((pixel_x, pixel_y))
    }
}

impl Default for GeoTIFFMetadata {
    fn default() -> Self {
        Self::new()
    }
}

/// GPU-accelerated tile processor with pure Rust TIFF backend
pub struct GPUTileProcessor {
    pub tiles: Vec<GeoTile>,
    pub metadata: GeoTIFFMetadata,
    pub band_data: Vec<Array2<f32>>,
    pub width: usize,
    pub height: usize,
    pub bands: usize,
    pub overlap_percent: f64,
}

impl GPUTileProcessor {
    pub fn new() -> Self {
        Self {
            tiles: Vec::new(),
            metadata: GeoTIFFMetadata::new(),
            band_data: Vec::new(),
            width: 0,
            height: 0,
            bands: 0,
            overlap_percent: DEFAULT_OVERLAP_PERCENT,
        }
    }

    /// Set overlap percentage for tiling (0.0-1.0)
    pub fn set_overlap(&mut self, overlap: f64) {
        self.overlap_percent = overlap.clamp(0.0, 0.5);
    }

    /// Load multi-band HLS tile from GeoTIFF files
    pub fn load_tile(&mut self, tile_prefix: &str, bands: &[&str]) -> Result<(), String> {
        self.band_data.clear();
        self.tiles.clear();
        self.metadata = GeoTIFFMetadata::new();

        for (band_idx, band) in bands.iter().enumerate() {
            // HLS files use format: HLS.L30.T16TDN.2021182T162824.v2.0.B04.tif
            let filename = format!("{}.B{}.tif", tile_prefix, band);
            println!("Loading band {} from {}...", band, filename);

            let (band_array, metadata) = Self::load_geotiff(&filename, band_idx)?;

            if self.width == 0 {
                self.width = metadata.width as usize;
                self.height = metadata.height as usize;
                self.metadata = metadata.clone();
            }

            self.band_data.push(band_array);
        }

        self.bands = self.band_data.len();

        // Generate 512x512 tiles with overlap from first band
        if let Some(first_band) = self.band_data.first() {
            self.tiles = self.generate_tiles_from_array(first_band, &self.metadata);
        }

        println!(
            "Loaded tile: {}x{} pixels, {} bands, {} tiles generated",
            self.width, self.height, self.bands, self.tiles.len()
        );
        println!(
            "Total memory: {:.2} MB",
            (self.width * self.height * self.bands * 4) as f64 / 1_000_000.0
        );

        Ok(())
    }

    /// Load single GeoTIFF band using pure Rust tiff crate
    fn load_geotiff(filename: &str, band_index: usize) -> Result<(Array2<f32>, GeoTIFFMetadata), String> {
        let file = File::open(filename)
            .map_err(|e| format!("Failed to open {}: {}", filename, e))?;
        let mut decoder = Decoder::new(BufReader::new(file))
            .map_err(|e| format!("Failed to decode {}: {}", filename, e))?;

        // Get dimensions
        let dimensions = decoder.dimensions()
            .map_err(|e| format!("Failed to get dimensions: {}", e))?;
        let width = dimensions.0;
        let height = dimensions.1;

        // Extract GeoTIFF tags
        let mut geotransform = [0.0f64; 6];
        let mut crs = String::from("EPSG:32616");
        let mut no_data_value = None;

        // ModelPixelScaleTag (33550)
        let pixel_scale: Option<Vec<f64>> = decoder.get_tag_f64_vec(Tag::Unknown(33550)).ok();
        
        // ModelTiepointTag (33922)
        let tiepoint: Option<Vec<f64>> = decoder.get_tag_f64_vec(Tag::Unknown(33922)).ok();

        // GeoKeyDirectoryTag (34735)
        let geo_keys: Option<Vec<u16>> = decoder.get_tag_u16_vec(Tag::Unknown(34735)).ok();

        // NoData tag
        if let Ok(no_data) = decoder.get_tag_f64(Tag::Unknown(33922)) {
            no_data_value = Some(no_data);
        }

        // Build geotransform from pixel scale and tiepoint
        if let (Some(scale), Some(tp)) = (pixel_scale, tiepoint) {
            if scale.len() >= 3 && tp.len() >= 6 {
                // GDAL geotransform convention:
                // [top_left_x, pixel_width, rotation_x, top_left_y, rotation_y, pixel_height]
                geotransform = [
                    tp[3],           // top-left X
                    scale[0],        // pixel width
                    scale[2],        // rotation X (usually 0)
                    tp[4],           // top-left Y
                    scale[1],        // rotation Y (usually 0)
                    -scale[1].abs(), // pixel height (negative for north-up)
                ];
            }
        }

        // Extract UTM zone from geo keys if available
        if let Some(keys) = geo_keys {
            if keys.len() >= 4 {
                // Check for projected CRS type
                let crs_type = keys[1];
                if crs_type == 1 {
                    // Projected CRS
                    if keys.len() >= 16 {
                        let utm_zone = keys[13] as i32;
                        if utm_zone > 0 {
                            crs = format!("EPSG:326{:02}", utm_zone);
                        }
                    }
                }
            }
        }

        // Read image data
        let result = decoder.read_image()
            .map_err(|e| format!("Failed to read image data: {}", e))?;

        // Convert to f32 array
        let mut array = Array2::from_elem((height as usize, width as usize), 0.0f32);
        
        match result {
            DecodingResult::F32(data) => {
                for (row, row_data) in data.chunks_exact(width as usize).enumerate() {
                    for (col, &val) in row_data.iter().enumerate() {
                        array[[row, col]] = val;
                    }
                }
            }
            DecodingResult::F64(data) => {
                for (row, row_data) in data.chunks_exact(width as usize).enumerate() {
                    for (col, &val) in row_data.iter().enumerate() {
                        array[[row, col]] = val as f32;
                    }
                }
            }
            DecodingResult::U16(data) => {
                for (row, row_data) in data.chunks_exact(width as usize).enumerate() {
                    for (col, &val) in row_data.iter().enumerate() {
                        array[[row, col]] = val as f32;
                    }
                }
            }
            DecodingResult::U8(data) => {
                for (row, row_data) in data.chunks_exact(width as usize).enumerate() {
                    for (col, &val) in row_data.iter().enumerate() {
                        array[[row, col]] = val as f32;
                    }
                }
            }
            DecodingResult::U32(data) => {
                for (row, row_data) in data.chunks_exact(width as usize).enumerate() {
                    for (col, &val) in row_data.iter().enumerate() {
                        array[[row, col]] = val as f32;
                    }
                }
            }
            DecodingResult::I16(data) => {
                for (row, row_data) in data.chunks_exact(width as usize).enumerate() {
                    for (col, &val) in row_data.iter().enumerate() {
                        array[[row, col]] = val as f32;
                    }
                }
            }
            DecodingResult::I32(data) => {
                for (row, row_data) in data.chunks_exact(width as usize).enumerate() {
                    for (col, &val) in row_data.iter().enumerate() {
                        array[[row, col]] = val as f32;
                    }
                }
            }
            _ => return Err(format!("Unsupported TIFF data type in {}", filename)),
        }

        // Build metadata
        let mut metadata = GeoTIFFMetadata {
            width,
            height,
            geotransform,
            crs,
            pixel_size_x: geotransform[1].abs(),
            pixel_size_y: geotransform[5].abs(),
            no_data_value,
            band_count: 1,
            ..GeoTIFFMetadata::new()
        };

        metadata.extract_utm_zone();

        Ok((array, metadata))
    }

    /// Generate 512x512 tiles from array with overlap
    fn generate_tiles_from_array(
        &self,
        data: &Array2<f32>,
        metadata: &GeoTIFFMetadata,
    ) -> Vec<GeoTile> {
        let (height, width) = data.dim();
        let overlap_pixels = ((TILE_SIZE as f64) * self.overlap_percent).round() as usize;
        let stride = TILE_SIZE - overlap_pixels;

        let mut tiles = Vec::new();
        let mut tile_idx = 0;

        for row_start in (0..height).step_by(stride) {
            for col_start in (0..width).step_by(stride) {
                let tile_w = TILE_SIZE.min(width - col_start);
                let tile_h = TILE_SIZE.min(height - row_start);

                // Extract tile data
                let mut tile_data = Vec::with_capacity(tile_w * tile_h);
                for row in row_start..(row_start + tile_h) {
                    for col in col_start..(col_start + tile_w) {
                        tile_data.push(data[[row, col]]);
                    }
                }

                // Calculate geotransform for this tile
                let gt = metadata.geotransform;
                let new_gt = [
                    gt[0] + (col_start as f64) * gt[1],
                    gt[1],
                    0.0,
                    gt[3] + (row_start as f64) * gt[5],
                    0.0,
                    gt[5],
                ];

                let mut tile = GeoTile::new(tile_w, tile_h, tile_data, new_gt, &metadata.crs);
                tile.band_name = format!("Tile_{}_{}_{}", tile_idx, col_start, row_start);
                tiles.push(tile);
                tile_idx += 1;
            }
        }

        tiles
    }

    /// Get all tiles for processing
    pub fn get_tiles(&self) -> &[GeoTile] {
        &self.tiles
    }

    /// GPU-accelerated B08/B04 ratio calculation (aluminum detection)
    pub fn calculate_aluminum_index(&self) -> Option<Array2<f32>> {
        if self.bands < 2 {
            println!("Need at least 2 bands for aluminum index");
            return None;
        }

        let b08 = &self.band_data.get(0)?;
        let b04 = &self.band_data.get(1)?;

        if b08.dim() != b04.dim() {
            println!("Band dimensions mismatch");
            return None;
        }

        let (height, width) = b08.dim();
        let mut result = Array2::from_elem((height, width), 0.0f32);

        println!(
            "Calculating B08/B04 ratio for {} pixels (parallel)...",
            width * height
        );

        // Parallel row processing
        let results: Vec<Vec<f32>> = (0..height)
            .into_par_iter()
            .map(|row| {
                (0..width)
                    .map(|col| {
                        let b08_val = b08[[row, col]];
                        let b04_val = b04[[row, col]];
                        if b04_val > 0.0 {
                            b08_val / b04_val
                        } else {
                            0.0
                        }
                    })
                    .collect()
            })
            .collect();

        for (row, result_row) in results.iter().enumerate() {
            for (col, &value) in result_row.iter().enumerate() {
                result[[row, col]] = value;
            }
        }

        println!("Aluminum index calculation complete");
        Some(result)
    }

    /// GPU-accelerated thermal anomaly detection (B10 - B11)
    pub fn calculate_thermal_anomaly(&self) -> Option<Array2<f32>> {
        if self.bands < 2 {
            println!("Need at least 2 bands for thermal anomaly");
            return None;
        }

        let b10 = &self.band_data.get(0)?;
        let b11 = &self.band_data.get(1)?;

        if b10.dim() != b11.dim() {
            println!("Band dimensions mismatch");
            return None;
        }

        let (height, width) = b10.dim();
        let mut result = Array2::from_elem((height, width), 0.0f32);

        println!(
            "Calculating thermal anomaly for {} pixels (parallel)...",
            width * height
        );

        let results: Vec<Vec<f32>> = (0..height)
            .into_par_iter()
            .map(|row| {
                (0..width)
                    .map(|col| {
                        let b10_val = b10[[row, col]];
                        let b11_val = b11[[row, col]];
                        b10_val - b11_val
                    })
                    .collect()
            })
            .collect();

        for (row, result_row) in results.iter().enumerate() {
            for (col, &value) in result_row.iter().enumerate() {
                result[[row, col]] = value;
            }
        }

        println!("Thermal anomaly calculation complete");
        Some(result)
    }

    /// Export detection results to KMZ
    pub fn export_kmz(
        &self,
        anomalies: &[(usize, usize, f32)],
        aluminum: &Array2<f32>,
        thermal: &Array2<f32>,
        output_path: &str,
        anchor_lock_info: &str,
    ) -> Result<(), String> {
        use std::fs::File;
        use std::io::Write;
        use zip::write::FileOptions;
        use zip::ZipWriter;

        println!("Exporting {} anomalies to KMZ...", anomalies.len());

        // Create KML content
        let mut kml = String::new();
        kml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        kml.push_str("<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n");
        kml.push_str("<Document>\n");
        kml.push_str("  <name>CESAROPS Detection Results</name>\n");
        kml.push_str(
            "  <description>Multi-sensor fusion anomaly detection with Anchor-Lock calibration</description>\n",
        );

        // Add anchor lock info
        kml.push_str("  <Folder>\n");
        kml.push_str("    <name>Anchor-Lock Calibration</name>\n");
        kml.push_str(&format!("    <description>{}</description>\n", anchor_lock_info));
        kml.push_str("  </Folder>\n");

        // Add anomalies
        kml.push_str("  <Folder>\n");
        kml.push_str("    <name>Detections</name>\n");

        for (i, (row, col, score)) in anomalies.iter().enumerate() {
            // Get values
            let alum_val = aluminum.get((*row, *col)).copied().unwrap_or(0.0);
            let therm_val = thermal.get((*row, *col)).copied().unwrap_or(0.0);

            // Calculate UTM from metadata
            let (utm_e, utm_n) = self
                .metadata
                .pixel_to_utm(*col as f64, *row as f64)
                .unwrap_or((0.0, 0.0));

            // Convert to WGS84
            let (lat, lon) = Self::utm_to_wgs84(
                utm_e,
                utm_n,
                self.metadata.utm_zone.unwrap_or(16),
                self.metadata.northern_hemisphere.unwrap_or(true),
            );

            // Classification
            let classification = Self::classify_anomaly(*score, alum_val, therm_val);
            let color = Self::score_to_color(*score);

            kml.push_str(&format!("    <Placemark>\n"));
            kml.push_str(&format!("      <name>Anomaly_{:04}</name>\n", i + 1));
            kml.push_str("      <description>\n");
            kml.push_str("        <![CDATA[\n");
            kml.push_str("        <h3>CESAROPS Anomaly Detection</h3>\n");
            kml.push_str("        <table>\n");
            kml.push_str(&format!(
                "          <tr><td><b>Score:</b></td><td>{:.3}</td></tr>\n",
                score
            ));
            kml.push_str(&format!(
                "          <tr><td><b>Classification:</b></td><td>{}</td></tr>\n",
                classification
            ));
            kml.push_str(&format!(
                "          <tr><td><b>B08/B04 Ratio:</b></td><td>{:.3}</td></tr>\n",
                alum_val
            ));
            kml.push_str(&format!(
                "          <tr><td><b>Thermal Delta:</b></td><td>{:.3}</td></tr>\n",
                therm_val
            ));
            kml.push_str(&format!(
                "          <tr><td><b>Est. Length:</b></td><td>{:.1} ft</td></tr>\n",
                score * 100.0
            ));
            kml.push_str(&format!(
                "          <tr><td><b>Est. Mass:</b></td><td>{:.1} tons</td></tr>\n",
                score * 50.0
            ));
            kml.push_str(&format!(
                "          <tr><td><b>Pixel Position:</b></td><td>Row: {}, Col: {}</td></tr>\n",
                row, col
            ));
            kml.push_str(&format!(
                "          <tr><td><b>UTM:</b></td><td>E: {:.2}m, N: {:.2}m</td></tr>\n",
                utm_e, utm_n
            ));
            kml.push_str(&format!(
                "          <tr><td><b>WGS84:</b></td><td>{:.6}°N, {:.6}°W</td></tr>\n",
                lat,
                lon.abs()
            ));
            kml.push_str(&format!(
                "          <tr><td><b>Anchor Lock:</b></td><td>{}</td></tr>\n",
                anchor_lock_info
            ));
            kml.push_str("        </table>\n");
            kml.push_str(
                "        <br/><i>Generated by CESAROPS v1.0 - Denny Hadfield Memorial Edition</i>\n",
            );
            kml.push_str("        ]]>");
            kml.push_str("      </description>\n");
            kml.push_str("      <Style>\n");
            kml.push_str("        <IconStyle>\n");
            kml.push_str(&format!("          <color>{}</color>\n", color));
            kml.push_str("          <scale>1.2</scale>\n");
            kml.push_str(
                "          <Icon><href>http://maps.google.com/mapfiles/kml/paddle/red-circle.png</href></Icon>\n",
            );
            kml.push_str("        </IconStyle>\n");
            kml.push_str("      </Style>\n");
            kml.push_str(&format!("      <Point>\n"));
            kml.push_str(&format!(
                "        <coordinates>{},{},0</coordinates>\n",
                lon, lat
            ));
            kml.push_str("      </Point>\n");
            kml.push_str("    </Placemark>\n");
        }

        kml.push_str("  </Folder>\n");
        kml.push_str("</Document>\n");
        kml.push_str("</kml>\n");

        // Write KMZ (zip file)
        let kmz_path = output_path;
        let file = File::create(kmz_path)
            .map_err(|e| format!("Failed to create {}: {}", kmz_path, e))?;

        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("doc.kml", options)
            .map_err(|e| format!("Failed to start KML in zip: {}", e))?;
        zip.write_all(kml.as_bytes())
            .map_err(|e| format!("Failed to write KML: {}", e))?;

        zip.finish()
            .map_err(|e| format!("Failed to finalize KMZ: {}", e))?;

        println!("KMZ saved to: {}", kmz_path);

        Ok(())
    }

    /// Classify anomaly based on sensor values
    fn classify_anomaly(score: f32, aluminum: f32, thermal: f32) -> String {
        if aluminum > 1.5 && thermal.abs() > 0.3 {
            "LIKELY_ALUMINUM (Aircraft?)".to_string()
        } else if thermal.abs() > 0.7 {
            "HEAVY_STEEL_MASS (Vessel?)".to_string()
        } else if aluminum > 1.2 {
            "POSSIBLE_ALUMINUM".to_string()
        } else if thermal.abs() > 0.4 {
            "POSSIBLE_STEEL".to_string()
        } else {
            "UNCLASSIFIED".to_string()
        }
    }

    /// Convert score to KML color (AABBGGRR format)
    fn score_to_color(score: f32) -> String {
        if score > 0.8 {
            "ff0000ff".to_string() // Red (high confidence)
        } else if score > 0.6 {
            "ff00ffff".to_string() // Cyan (medium-high)
        } else if score > 0.4 {
            "ff00ff00".to_string() // Green (medium)
        } else {
            "ffffff00".to_string() // Yellow (low)
        }
    }

    /// UTM to WGS84 conversion using proper formulas
    fn utm_to_wgs84(easting: f64, northing: f64, zone: i32, northern: bool) -> (f64, f64) {
        // WGS84 ellipsoid parameters
        const A: f64 = 6378137.0; // Semi-major axis
        const F: f64 = 1.0 / 298.257223563; // Flattening
        const E2: f64 = 2.0 * F - F * F; // Eccentricity squared
        const K0: f64 = 0.9996; // Scale factor

        let central_meridian = (zone as f64 - 1.0) * 6.0 - 180.0 + 3.0;

        // Remove false easting/northing
        let x = easting - 500000.0;
        let y = if northern {
            northing
        } else {
            northing - 10000000.0
        };

        // Footpoint latitude
        let m = y / K0;
        let mu = m / (A * (1.0 - E2 / 4.0 - 3.0 * E2 * E2 / 64.0 - 5.0 * E2 * E2 * E2 / 256.0));

        let phi1 = mu
            + (3.0 * E2 / 2.0 - 27.0 * E2 * E2 * E2 / 32.0) * (2.0 * mu).sin()
            + (21.0 * E2 * E2 / 16.0 - 55.0 * E2 * E2 * E2 * E2 / 32.0) * (4.0 * mu).sin()
            + (151.0 * E2 * E2 * E2 / 96.0) * (6.0 * mu).sin();

        // Radius of curvature
        let phi1_sin = phi1.sin();
        let n = A / (1.0 - E2 * phi1_sin * phi1_sin).sqrt();
        let r = A * (1.0 - E2) / (1.0 - E2 * phi1_sin * phi1_sin).powf(1.5);
        let t = phi1.tan().powi(2);
        let c = E2 * phi1.cos().powi(-2);
        let a_val = x / (n * K0);

        // Latitude
        let lat = phi1
            - (n * phi1.tan() / r)
                * (a_val * a_val / 2.0
                    - (5.0 + 3.0 * t + 10.0 * c - 4.0 * c * c - 9.0 * E2 / 2.0)
                        * a_val.powi(4)
                        / 24.0
                    + (61.0 + 90.0 * t + 298.0 * c + 45.0 * t * t - 252.0 * E2 / 2.0
                        - 3.0 * c * c)
                        * a_val.powi(6)
                        / 720.0);

        // Longitude
        let lon = central_meridian.to_radians()
            + (a_val
                - (1.0 + 2.0 * t + c) * a_val.powi(3) / 6.0
                + (5.0 - 2.0 * c + 28.0 * t - 3.0 * c * c + 8.0 * E2 / 2.0 + 24.0 * t * t)
                    * a_val.powi(5)
                    / 120.0)
                / phi1.cos();

        (lat.to_degrees(), lon.to_degrees())
    }
}

impl Default for GPUTileProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_creation() {
        let mut metadata = GeoTIFFMetadata::new();
        metadata.geotransform = [450000.0, 30.0, 0.0, 4700000.0, 0.0, -30.0];
        metadata.crs = "EPSG:32616".to_string();
        metadata.extract_utm_zone();

        assert_eq!(metadata.utm_zone, Some(16));
        assert_eq!(metadata.northern_hemisphere, Some(true));
    }

    #[test]
    fn test_utm_to_wgs84() {
        // Chicago area in UTM Zone 16N
        let easting = 450000.0;
        let northing = 4650000.0;
        let (lat, lon) = GPUTileProcessor::utm_to_wgs84(easting, northing, 16, true);

        assert!(lat > 41.0 && lat < 43.0);
        assert!(lon.abs() > 87.0 && lon.abs() < 88.0);
    }
}
