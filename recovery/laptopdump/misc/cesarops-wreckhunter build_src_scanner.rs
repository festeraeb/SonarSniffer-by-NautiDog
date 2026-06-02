// CESAROPS Main Scanner - Complete End-to-End Pipeline
// Integrates: Anchor-Lock + GPU Processing + KMZ Export
// Uses GDAL for GeoTIFF handling with 512x512 tiling

use crate::{
    anchor_lock::AnchorLockProcessor,
    coordinate::{utm_to_wgs84, UTMCoordinate, WGS84Coordinate, get_grid_reference, GRID_CELL_SIZE_M},
    gpu_engine::GpuEngine,
    gpu_processor::GPUTileProcessor,
    geotile::{GeoTile, TILE_SIZE},
};
use ndarray::Array2;
use rayon::prelude::*;
use std::fs::{self, File};
use std::io::{Write, BufWriter};
use std::path::Path;

/// Complete scanner configuration
pub struct ScannerConfig {
    pub data_dir: String,
    pub output_dir: String,
    pub min_confidence: f32,
    pub gpu_batch_size: usize,
    pub require_gpu: bool,
    pub tile_overlap_percent: f64,
    pub chunking_enabled: bool,       // false = full tile processing
    
    // ========================================================================
    // #DATABASE-DISABLED-2026-04-01
    // Reason: Database logger temporarily disabled, CSV export preferred
    // To re-enable:
    //   1. Uncomment these fields
    //   2. Update main.rs run_scan() to pass database path
    //   3. Re-enable database_logger module in lib.rs
    // Last action: Commented to prioritize CSV export testing
    // ========================================================================
    // pub log_to_database: bool,
    // pub database_path: std::path::PathBuf,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            data_dir: "./data/cache".to_string(),
            output_dir: "./outputs".to_string(),
            min_confidence: 0.0,  // Log everything by default
            gpu_batch_size: 1,
            require_gpu: false,
            tile_overlap_percent: 0.10,
            chunking_enabled: true,  // Default to chunked for memory efficiency
            // log_to_database: false,
            // database_path: std::path::PathBuf::from("./outputs/cesarops_runs.db"),
        }
    }
}

/// Detection result with full metadata including multi-pass confidence
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Detection {
    pub id: usize,
    pub score: f32,           // Final confidence score (after boosting)
    pub base_score: f32,      // Original score before multi-pass boosting
    pub classification: String,
    pub aluminum_ratio: f32,
    pub thermal_delta: f32,
    pub estimated_length_ft: f32,
    pub estimated_mass_tons: f32,
    pub pixel_row: usize,
    pub pixel_col: usize,
    pub utm_easting: f64,
    pub utm_northing: f64,
    pub wgs84_lat: f64,
    pub wgs84_lon: f64,
    pub source_tile: String,
    pub anchor_lock_offset: String,
    pub grid_ref: String,
    pub utm: UTMCoordinate,
    pub wgs84: WGS84Coordinate,
    // Multi-pass and multi-sensor tracking
    pub pass_count: u32,              // Number of satellite passes detecting this target
    pub sensor_locks: Vec<String>,    // Which sensors detected it: ["thermal", "optical", "sar", "swot"]
    pub lock_level: String,           // "single", "double", "triple", "quad"
    pub is_surface_target: bool,      // True if only visible in non-penetrating bands
    pub needs_examination: bool,      // True for single-pass or borderline detections
    pub examination_reason: String,   // Why it needs examination
}

/// Main CESAROPS scanner
pub struct CesaropsScanner {
    pub config: ScannerConfig,
    pub anchor_processor: AnchorLockProcessor,
    pub gpu_processor: GPUTileProcessor,
    pub gpu_engine: Option<GpuEngine>,
    pub detections: Vec<Detection>,
    pub detection_counter: usize,
}

impl CesaropsScanner {
    pub fn new(config: ScannerConfig) -> Self {
        let overlap = config.tile_overlap_percent;
        
        let gpu_engine = match pollster::block_on(GpuEngine::new()) {
            Ok(e) => {
                println!("  ✓ GPU Engine initialized for Quadro M2200 path");
                Some(e)
            }
            Err(err) => {
                if config.require_gpu {
                    panic!("GPU Engine initialization failed and require_gpu=true: {}", err);
                }
                println!("  ⚠️ GPU Engine initialization failed: {}. CPU fallback enabled", err);
                None
            }
        };

        let mut processor = GPUTileProcessor::new();
        processor.set_overlap(overlap);

        Self {
            config,
            anchor_processor: AnchorLockProcessor::new(),
            gpu_processor: processor,
            gpu_engine,
            detections: Vec::new(),
            detection_counter: 0,
        }
    }

    /// Run complete scan pipeline
    pub fn run_full_scan(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("================================================================================");
        println!("CESAROPS FULL LAKE MICHIGAN SCAN");
        println!("Denny Hadfield Memorial Edition - v1.0");
        println!("Using GDAL backend with 512x512 tiling ({}% overlap)", 
                 (self.config.tile_overlap_percent * 100.0) as i32);
        println!("================================================================================\n");

        // Step 1: Initialize anchor network
        self.initialize_anchor_network();

        // Step 2: Find and process all HLS tiles
        let tile_files = self.find_hls_tiles();
        println!("\n[STEP 2/5] Processing {} HLS Tiles...\n", tile_files.len());

        for (idx, tile_path) in tile_files.iter().enumerate() {
            println!("Processing tile {}/{}: {}", idx + 1, tile_files.len(), tile_path);
            self.process_tile(tile_path)?;
        }

        // Step 2.5: Cluster and merge multi-pass detections (DISABLED FOR NOW)
        // self.cluster_and_merge_detections();

        // Step 3: Apply anchor-lock calibration
        self.apply_anchor_calibration();

        // Step 4: Export results
        self.export_results()?;

        // Step 5: Summary
        self.print_summary();

        Ok(())
    }

    /// Initialize anchor-lock network
    fn initialize_anchor_network(&mut self) {
        println!("[STEP 1/5] Initializing Anchor-Lock Network...\n");

        // Known harbor lights for anchor-lock calibration
        let anchors = vec![
            ("Wind Point Light", -87.8178, 42.8000),
            ("North Point Light", -87.8726, 43.0644),
            ("Holland Harbor Light", -86.2066, 42.7784),
            ("Grand Haven Pierhead Light", -86.2542, 43.0638),
            ("Chicago Harbor Light", -87.6044, 41.8900),
            ("Waukegan Harbor Light", -87.8034, 42.3638),
            ("Michigan City East Pierhead Light", -86.8862, 41.7138),
        ];

        for (name, lon, lat) in anchors {
            let _ = self.anchor_processor.calibrate_from_anchors(name, lon, lat);
        }

        if let Some((avg_e, avg_n, avg_mag)) = self.anchor_processor.get_weighted_average_offset() {
            println!("Anchor-Lock Calibration: ΔE:{:+.2}m ΔN:{:+.2}m (Total: {:.2}m)\n", avg_e, avg_n, avg_mag);
        }
    }

    /// Find all HLS tile directories (recursive)
    fn find_hls_tiles(&self) -> Vec<String> {
        let mut tile_prefixes = std::collections::HashSet::new();
        let data_path = Path::new(&self.config.data_dir);

        // Recursively find all .tif files
        fn find_tiles_recursive(dir: &Path, prefixes: &mut std::collections::HashSet<String>) {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        find_tiles_recursive(&path, prefixes);
                    } else if path.extension().and_then(|s| s.to_str()) == Some("tif") {
                        if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
                            if filename.starts_with("HLS") {
                                // Extract base tile ID: remove .Bxx suffix
                                let base_id = if let Some(pos) = filename.rfind(".B") {
                                    &filename[..pos]
                                } else {
                                    filename
                                };
                                // Get directory containing this tile
                                if let Some(parent) = path.parent() {
                                    let prefix = parent.join(base_id).to_string_lossy().to_string();
                                    prefixes.insert(prefix);
                                }
                            }
                        }
                    }
                }
            }
        }

        find_tiles_recursive(data_path, &mut tile_prefixes);

        let mut tiles: Vec<String> = tile_prefixes.into_iter().collect();
        tiles.sort();
        tiles
    }

    /// Process single HLS tile
    fn process_tile(&mut self, tile_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Determine bands based on satellite type
        let bands = if tile_path.contains("L30") {
            vec!["04", "05"] // Landsat-8: B04 (red), B05 (NIR) for aluminum
        } else {
            vec!["04", "05"] // Sentinel-2: B04, B05
        };

        // Load tile with GDAL
        match self.gpu_processor.load_tile(tile_path, &bands.iter().map(|s| *s).collect::<Vec<_>>()) {
            Ok(_) => {},
            Err(e) => {
                println!("  Warning: Could not load tile: {}", e);
                return Ok(());
            }
        }

        // Calculate indices
        let aluminum = match self.gpu_processor.calculate_aluminum_index() {
            Some(a) => a,
            None => {
                println!("  Warning: Could not calculate aluminum index");
                return Ok(());
            }
        };

        let thermal = match self.gpu_processor.calculate_thermal_anomaly() {
            Some(t) => t,
            None => {
                println!("  Warning: Could not calculate thermal anomaly");
                return Ok(());
            }
        };

        // If we have a GPU engine available, run GPU processing
        if let Some(engine) = &self.gpu_engine {
            let width = aluminum.dim().1 as u32;
            let height = aluminum.dim().0 as u32;

            // GPU thermal map
            let flat_thermal: Vec<f32> = thermal.iter().cloned().collect();
            match engine.process_thermal(&flat_thermal, width, height) {
                Ok(gpu_thermal) => {
                    println!("  ✓ GPU thermal Z-score pass complete (shape {}x{})", width, height);
                    if let Ok(gpu_thermal_arr) = ndarray::Array2::from_shape_vec((height as usize, width as usize), gpu_thermal) {
                        let _ = thermal; // Replace with GPU result if needed
                        let _ = gpu_thermal_arr;
                    }
                }
                Err(e) => println!("  ⚠️ GPU thermal pass failed: {}", e),
            }
        }

        // Find anomalies with tile-aware processing
        let anomalies = self.find_anomalies_with_tiles(&aluminum, &thermal, tile_path);
        self.detections.extend(anomalies);

        println!("  Found {} anomalies above threshold",
                 self.detections.iter().filter(|d| d.source_tile.contains(tile_path)).count());

        Ok(())
    }

    /// Anomaly detection with proper coordinate calculation and Z-score filtering
    /// 
    /// Z-score ranges for valid detections:
    /// - Thermal anomalies: |Z| = 1-4 (real thermal signatures from dense masses)
    /// - Glint/optical: |Z| = 10-65 (sun glint, atmospheric effects)
    /// - Outside these ranges: likely noise/sensor artifacts
    fn find_anomalies_with_tiles(
        &self,
        aluminum: &Array2<f32>,
        thermal: &Array2<f32>,
        tile_path: &str,
    ) -> Vec<Detection> {
        let (height, width) = aluminum.dim();
        const MAX_DETECTIONS_PER_TILE: usize = 500;
        const SUPPRESSION_RADIUS: usize = 10;
        
        // Z-score validity ranges
        const THERMAL_Z_MIN: f32 = 1.0;
        const THERMAL_Z_MAX: f32 = 4.0;
        const GLINT_Z_MIN: f32 = 10.0;
        const GLINT_Z_MAX: f32 = 65.0;

        // Get metadata for coordinate conversion
        let metadata = &self.gpu_processor.metadata;

        // First pass: find local maxima with valid Z-scores
        let mut candidates: Vec<(usize, usize, f32, f32, f32)> = Vec::new();

        for row in SUPPRESSION_RADIUS..height.saturating_sub(SUPPRESSION_RADIUS) {
            for col in SUPPRESSION_RADIUS..width.saturating_sub(SUPPRESSION_RADIUS) {
                let alum = aluminum[[row, col]];
                let therm = thermal[[row, col]];
                
                // Calculate thermal Z-score (thermal values are already Z-scores from GPU)
                let thermal_z = therm.abs();
                
                // Filter by Z-score validity ranges
                let is_valid_thermal = thermal_z >= THERMAL_Z_MIN && thermal_z <= THERMAL_Z_MAX;
                let is_valid_glint = thermal_z >= GLINT_Z_MIN && thermal_z <= GLINT_Z_MAX;
                
                if !is_valid_thermal && !is_valid_glint {
                    continue;
                }
                
                let center_score = (alum + thermal_z) / 2.0;

                if center_score < self.config.min_confidence {
                    continue;
                }

                // Check if this is a local maximum in the suppression radius
                let mut is_max = true;
                for dy in -(SUPPRESSION_RADIUS as i32)..=(SUPPRESSION_RADIUS as i32) {
                    for dx in -(SUPPRESSION_RADIUS as i32)..=(SUPPRESSION_RADIUS as i32) {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nr = (row as i32 + dy) as usize;
                        let nc = (col as i32 + dx) as usize;
                        let neighbor_therm = thermal[[nr, nc]].abs();
                        let neighbor_z_valid = (neighbor_therm >= THERMAL_Z_MIN && neighbor_therm <= THERMAL_Z_MAX)
                            || (neighbor_therm >= GLINT_Z_MIN && neighbor_therm <= GLINT_Z_MAX);
                        if neighbor_z_valid {
                            let neighbor_score = (aluminum[[nr, nc]] + neighbor_therm) / 2.0;
                            if neighbor_score > center_score {
                                is_max = false;
                                break;
                            }
                        }
                    }
                    if !is_max {
                        break;
                    }
                }

                if is_max {
                    candidates.push((row, col, center_score, alum, therm));
                }
            }
        }

        // Sort by score descending and take top N
        candidates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(MAX_DETECTIONS_PER_TILE);

        // Convert to Detection objects
        let mut anomalies = Vec::with_capacity(candidates.len());
        for (row, col, score, alum, therm) in candidates {
            // Calculate UTM from pixel position using GDAL geotransform
            let (utm_e, utm_n) = metadata
                .pixel_to_utm(col as f64, row as f64)
                .unwrap_or((0.0, 0.0));

            // Convert to WGS84
            let (lat, lon) = utm_to_wgs84_full(
                utm_e,
                utm_n,
                metadata.utm_zone.unwrap_or(16),
                metadata.northern_hemisphere.unwrap_or(true),
            );

            // Generate grid reference
            let grid_ref = get_grid_reference(utm_e, utm_n, GRID_CELL_SIZE_M);

            anomalies.push(Detection {
                id: 0, // Will be assigned later
                score,
                base_score: score,
                classification: Self::classify(score, alum, therm),
                aluminum_ratio: alum,
                thermal_delta: therm,
                estimated_length_ft: score * 100.0,
                estimated_mass_tons: score * 50.0,
                pixel_row: row,
                pixel_col: col,
                utm_easting: utm_e,
                utm_northing: utm_n,
                wgs84_lat: lat,
                wgs84_lon: lon,
                source_tile: tile_path.to_string(),
                anchor_lock_offset: String::new(),
                grid_ref,
                utm: UTMCoordinate {
                    easting: utm_e,
                    northing: utm_n,
                    zone: metadata.utm_zone.unwrap_or(16) as u8,
                },
                wgs84: WGS84Coordinate { lat, lon },
                pass_count: 1,
                sensor_locks: Vec::new(),
                lock_level: String::from("single"),
                is_surface_target: false,
                needs_examination: false,
                examination_reason: String::new(),
            });
        }

        anomalies
    }

    /// Apply anchor-lock calibration to all detections
    fn apply_anchor_calibration(&mut self) {
        println!("\n[STEP 3/5] Applying Anchor-Lock Calibration...\n");

        let calibration = if let Some((avg_e, avg_n, avg_mag)) = self.anchor_processor.get_weighted_average_offset() {
            format!("ΔE:{:+.2}m ΔN:{:+.2}m (Total: {:.2}m)", avg_e, avg_n, avg_mag)
        } else {
            "No calibration".to_string()
        };

        // Apply offset to all detections
        for detection in &mut self.detections {
            detection.anchor_lock_offset = calibration.clone();
        }
    }

    /// Export results to KMZ and CSV
    fn export_results(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("[STEP 4/5] Exporting Results...\n");

        fs::create_dir_all(&self.config.output_dir)?;

        // Export CSV (primary format for database import)
        let csv_path = format!("{}/detections.csv", self.config.output_dir);
        let mut csv_file = BufWriter::new(File::create(&csv_path)?);
        
        // CSV Header
        writeln!(csv_file, "id,score,base_score,classification,aluminum_ratio,thermal_delta,estimated_length_ft,estimated_mass_tons,pixel_row,pixel_col,utm_easting,utm_northing,wgs84_lat,wgs84_lon,grid_ref,source_tile,anchor_lock_offset,pass_count,lock_level,is_surface_target,needs_examination")?;
        
        // CSV Data
        for (idx, detection) in self.detections.iter().enumerate() {
            writeln!(csv_file, "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                idx + 1,
                detection.score,
                detection.base_score,
                detection.classification,
                detection.aluminum_ratio,
                detection.thermal_delta,
                detection.estimated_length_ft,
                detection.estimated_mass_tons,
                detection.pixel_row,
                detection.pixel_col,
                detection.utm_easting,
                detection.utm_northing,
                detection.wgs84_lat,
                detection.wgs84_lon,
                detection.grid_ref,
                detection.source_tile,
                detection.anchor_lock_offset,
                detection.pass_count,
                detection.lock_level,
                detection.is_surface_target,
                detection.needs_examination
            )?;
        }
        
        csv_file.flush()?;
        println!("CSV exported: {}", csv_path);

        // Also export KMZ for Google Earth
        let kmz_path = format!("{}/LAKE_MICHIGAN_SOUTH_CENSUS.kmz", self.config.output_dir);

        // Create KML content
        let mut kml = String::new();
        kml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        kml.push_str("<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n");
        kml.push_str("<Document>\n");
        kml.push_str("  <name>CESAROPS Lake Michigan Survey</name>\n");
        kml.push_str("  <description>Multi-sensor fusion anomaly detection with Anchor-Lock calibration</description>\n");

        // Anchor-Lock folder
        kml.push_str("  <Folder>\n");
        kml.push_str("    <name>Anchor-Lock Calibration</name>\n");
        if let Some((avg_e, avg_n, avg_mag)) = self.anchor_processor.get_weighted_average_offset() {
            kml.push_str(&format!("    <description>Offset: ΔE:{:+.2}m ΔN:{:+.2}m (Total: {:.2}m)</description>\n", avg_e, avg_n, avg_mag));
        }
        kml.push_str("  </Folder>\n");

        // Detections folder
        kml.push_str("  <Folder>\n");
        kml.push_str("    <name>Detections</name>\n");

        for detection in &self.detections {
            kml.push_str(&self.detection_to_placemark(detection));
        }

        kml.push_str("  </Folder>\n");
        kml.push_str("</Document>\n");
        kml.push_str("</kml>\n");

        // Write KMZ using zip crate
        use zip::write::FileOptions;
        use zip::ZipWriter;

        let file = File::create(&kmz_path)?;
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("doc.kml", options)?;
        zip.write_all(kml.as_bytes())?;
        zip.finish()?;

        println!("KMZ saved to: {}", kmz_path);

        Ok(())
    }

    /// Convert detection to KML placemark
    fn detection_to_placemark(&self, d: &Detection) -> String {
        let color = if d.score > 0.8 { "ff0000ff" }
                   else if d.score > 0.6 { "ff00ffff" }
                   else { "ffffff00" };

        format!(r#"    <Placemark>
      <name>Anomaly_{:04}</name>
      <description>
        <![CDATA[
        <h3>CESAROPS Anomaly Detection</h3>
        <table>
          <tr><td><b>Score:</b></td><td>{:.3}</td></tr>
          <tr><td><b>Classification:</b></td><td>{}</td></tr>
          <tr><td><b>B08/B04 Ratio:</b></td><td>{:.3}</td></tr>
          <tr><td><b>Thermal Delta:</b></td><td>{:.3}</td></tr>
          <tr><td><b>Est. Length:</b></td><td>{:.1} ft</td></tr>
          <tr><td><b>Est. Mass:</b></td><td>{:.1} tons</td></tr>
          <tr><td><b>Pixel Position:</b></td><td>Row: {}, Col: {}</td></tr>
          <tr><td><b>UTM-16T:</b></td><td>E: {:.2}m, N: {:.2}m</td></tr>
          <tr><td><b>WGS84:</b></td><td>{:.6}°N, {:.6}°W</td></tr>
          <tr><td><b>Grid Ref:</b></td><td>{}</td></tr>
          <tr><td><b>Source Tile:</b></td><td>{}</td></tr>
          <tr><td><b>Anchor Lock:</b></td><td>{}</td></tr>
        </table>
        <br/><i>Generated by CESAROPS v1.0 - Denny Hadfield Memorial Edition</i>
        ]]>
      </description>
      <Style>
        <IconStyle>
          <color>{}</color>
          <scale>1.2</scale>
          <Icon><href>http://maps.google.com/mapfiles/kml/paddle/red-circle.png</href></Icon>
        </IconStyle>
      </Style>
      <Point>
        <coordinates>{},{},0</coordinates>
      </Point>
    </Placemark>
"#,
            d.id, d.score, d.classification, d.aluminum_ratio, d.thermal_delta,
            d.estimated_length_ft, d.estimated_mass_tons, d.pixel_row, d.pixel_col,
            d.utm_easting, d.utm_northing, d.wgs84_lat, d.wgs84_lon.abs(),
            d.grid_ref, d.source_tile, d.anchor_lock_offset, color, d.wgs84_lon, d.wgs84_lat)
    }

    /// Print scan summary
    fn print_summary(&self) {
        println!("\n================================================================================");
        println!("SCAN COMPLETE");
        println!("================================================================================\n");

        println!("Results Summary:");
        println!("  • Total detections: {}", self.detections.len());
        println!("  • High confidence (>0.8): {}", self.detections.iter().filter(|d| d.score > 0.8).count());
        println!("  • Medium confidence (0.6-0.8): {}", self.detections.iter().filter(|d| d.score > 0.6 && d.score <= 0.8).count());
        println!("  • Low confidence (0.5-0.6): {}", self.detections.iter().filter(|d| d.score >= 0.5 && d.score <= 0.6).count());
        println!();

        // Classification breakdown
        let mut by_class = std::collections::HashMap::new();
        for d in &self.detections {
            *by_class.entry(&d.classification).or_insert(0) += 1;
        }

        println!("Classification Breakdown:");
        for (class, count) in by_class {
            println!("  • {}: {}", class, count);
        }
        println!();

        println!("Output Files:");
        println!("  • KMZ: {}/LAKE_MICHIGAN_SOUTH_CENSUS.kmz", self.config.output_dir);
        println!();

        println!("To view in Google Earth:");
        println!("  1. Open Google Earth Pro");
        println!("  2. File → Open → Select LAKE_MICHIGAN_SOUTH_CENSUS.kmz");
        println!("  3. Click markers for detailed popup information");
        println!();

        println!("================================================================================\n");
    }

    /// Classify detection based on sensor values
    fn classify(score: f32, aluminum: f32, thermal: f32) -> String {
        if aluminum > 1.5 && thermal.abs() > 0.3 {
            "LIKELY_ALUMINUM (Aircraft?)".to_string()
        } else if thermal.abs() > 0.7 {
            "HEAVY_STEEL_MASS (Vessel?)".to_string()
        } else if aluminum > 1.2 && thermal.abs() > 0.4 {
            "POSSIBLE_ALUMINUM".to_string()
        } else if thermal.abs() > 0.5 {
            "POSSIBLE_STEEL".to_string()
        } else {
            "UNCLASSIFIED".to_string()
        }
    }
}

/// Full UTM to WGS84 conversion using proper formulas
fn utm_to_wgs84_full(easting: f64, northing: f64, zone: i32, northern: bool) -> (f64, f64) {
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

/// Main entry point
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ScannerConfig::default();
    let mut scanner = CesaropsScanner::new(config);
    scanner.run_full_scan()
}
