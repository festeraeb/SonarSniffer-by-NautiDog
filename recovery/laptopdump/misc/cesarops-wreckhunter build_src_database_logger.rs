// CESAROPS Database Logger
// Logs EVERY detection, EVERY run, EVERY condition for scientific analysis
// No filtering, no assumptions - raw data for post-processing

use rusqlite::{Connection, Result};
use std::path::Path;
use chrono::Utc;
use crate::scanner::Detection;

/// Database logger for comprehensive detection tracking
pub struct DatabaseLogger {
    conn: Connection,
}

/// Scan run metadata
pub struct ScanRunInfo {
    pub scan_type: String,
    pub input_directory: String,
    pub output_directory: String,
    pub min_confidence: f32,
    pub tile_overlap_percent: f64,
    pub gpu_name: String,
    pub gpu_vendor: String,
    pub cpu_cores: usize,
    pub system_ram_gb: f32,
}

/// Tile processing metadata
pub struct TileInfo {
    pub tile_path: String,
    pub tile_prefix: String,
    pub satellite_type: String,
    pub acquisition_date: String,
    pub acquisition_time: String,
    pub mgrs_grid: String,
    pub bands_processed: Vec<String>,
    pub width_pixels: usize,
    pub height_pixels: usize,
    pub pixel_size_meters: f32,
    pub geotransform: [f64; 6],
    pub crs: String,
    pub utm_zone: i32,
    pub load_time_seconds: f32,
    pub gpu_upload_time_seconds: f32,
    pub gpu_compute_time_seconds: f32,
    pub total_processing_time_seconds: f32,
    pub thermal_mean: f32,
    pub thermal_stddev: f32,
    pub thermal_valid_pixels: usize,
    pub thermal_total_pixels: usize,
    pub raw_anomaly_count: usize,
    pub top_anomaly_z_score: f32,
}

impl DatabaseLogger {
    /// Create or open database
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        
        // Initialize schema
        Self::initialize_schema(&conn)?;
        
        Ok(Self { conn })
    }

    /// Initialize database schema
    fn initialize_schema(conn: &Connection) -> Result<()> {
        // Read and execute SQL schema file
        let schema_sql = include_str!("../cesarops_database.sql");
        
        // Execute each statement
        conn.execute_batch(schema_sql)?;
        
        Ok(())
    }

    /// Start a new scan run
    pub fn start_scan_run(&self, info: &ScanRunInfo) -> Result<i64> {
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        
        self.conn.execute(
            "INSERT INTO scan_runs (
                run_timestamp, scan_type, input_directory, output_directory,
                min_confidence, tile_overlap_percent, require_gpu,
                gpu_name, gpu_vendor, cpu_cores, system_ram_gb,
                total_tiles_processed, total_detections
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            (
                timestamp,
                &info.scan_type,
                &info.input_directory,
                &info.output_directory,
                info.min_confidence,
                info.tile_overlap_percent,
                false,
                &info.gpu_name,
                &info.gpu_vendor,
                info.cpu_cores as i32,
                info.system_ram_gb,
                0,
                0,
            ),
        )?;
        
        Ok(self.conn.last_insert_rowid())
    }

    /// Log a processed tile
    pub fn log_tile(&self, run_id: i64, info: &TileInfo) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO tiles_processed (
                run_id, tile_path, tile_prefix, satellite_type,
                acquisition_date, acquisition_time, mgrs_grid, bands_processed,
                width_pixels, height_pixels, pixel_size_meters, geotransform,
                crs, utm_zone, load_time_seconds, gpu_upload_time_seconds,
                gpu_compute_time_seconds, total_processing_time_seconds,
                thermal_mean, thermal_stddev, thermal_valid_pixels,
                thermal_total_pixels, raw_anomaly_count, top_anomaly_z_score
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
            (
                run_id,
                &info.tile_path,
                &info.tile_prefix,
                &info.satellite_type,
                &info.acquisition_date,
                &info.acquisition_time,
                &info.mgrs_grid,
                serde_json::to_string(&info.bands_processed).unwrap_or_default(),
                info.width_pixels as i32,
                info.height_pixels as i32,
                info.pixel_size_meters,
                serde_json::to_string(&info.geotransform).unwrap_or_default(),
                &info.crs,
                info.utm_zone,
                info.load_time_seconds,
                info.gpu_upload_time_seconds,
                info.gpu_compute_time_seconds,
                info.total_processing_time_seconds,
                info.thermal_mean,
                info.thermal_stddev,
                info.thermal_valid_pixels as i32,
                info.thermal_total_pixels as i32,
                info.raw_anomaly_count as i32,
                info.top_anomaly_z_score,
            ),
        )?;
        
        Ok(self.conn.last_insert_rowid())
    }

    /// Log a raw detection (EVERY detection, no filtering)
    pub fn log_detection(&self, run_id: i64, tile_id: i64, detection: &Detection, 
                         thermal_z: f32, would_be_filtered: bool, filter_reason: &str) -> Result<i64> {
        let raw_classification = Self::classify_raw(detection.aluminum_ratio, thermal_z);
        let is_valid_thermal = thermal_z >= 1.0 && thermal_z <= 4.0;
        let is_valid_glint = thermal_z >= 10.0 && thermal_z <= 65.0;
        
        self.conn.execute(
            "INSERT INTO raw_detections (
                tile_id, run_id, pixel_row, pixel_col,
                utm_easting, utm_northing, utm_zone, wgs84_lat, wgs84_lon, grid_ref,
                aluminum_ratio, thermal_z_score, base_score, raw_classification,
                is_valid_thermal_z, is_valid_glint_z, is_outside_ranges,
                would_be_filtered, filter_reason,
                detected_by_thermal, detected_by_optical,
                needs_examination, examination_reason, examination_priority
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
            (
                tile_id,
                run_id,
                detection.pixel_row as i32,
                detection.pixel_col as i32,
                detection.utm_easting,
                detection.utm_northing,
                detection.utm.zone as i32,
                detection.wgs84_lat,
                detection.wgs84_lon,
                &detection.grid_ref,
                detection.aluminum_ratio,
                thermal_z,
                detection.base_score,
                raw_classification,
                is_valid_thermal as i32,
                is_valid_glint as i32,
                (!is_valid_thermal && !is_valid_glint) as i32,
                would_be_filtered as i32,
                filter_reason,
                (thermal_z.abs() >= 1.0 && thermal_z.abs() <= 4.0) as i32,
                (detection.aluminum_ratio > 1.2) as i32,
                (would_be_filtered || detection.score < 0.6) as i32,
                if would_be_filtered { "Outside Z-score range" } else { "Low confidence" },
                if would_be_filtered { "medium" } else { "low" },
            ),
        )?;
        
        Ok(self.conn.last_insert_rowid())
    }

    /// Classify detection based on raw sensor values
    fn classify_raw(aluminum: f32, thermal_z: f32) -> String {
        let thermal_abs = thermal_z.abs();
        
        if aluminum > 1.5 && thermal_abs > 0.3 {
            "LIKELY_ALUMINUM".to_string()
        } else if thermal_abs > 0.7 {
            "HEAVY_STEEL_MASS".to_string()
        } else if aluminum > 1.2 {
            "POSSIBLE_ALUMINUM".to_string()
        } else if thermal_abs > 0.5 {
            "POSSIBLE_STEEL".to_string()
        } else {
            "UNCLASSIFIED".to_string()
        }
    }

    /// Update scan run statistics
    pub fn update_run_stats(&self, run_id: i64, tiles: i32, detections: i32, time_seconds: f32) -> Result<()> {
        self.conn.execute(
            "UPDATE scan_runs SET 
                total_tiles_processed = ?1,
                total_detections = ?2,
                processing_time_seconds = ?3
            WHERE run_id = ?4",
            (tiles, detections, time_seconds, run_id),
        )?;
        Ok(())
    }

    /// Log GPU/system metrics
    pub fn log_system_metrics(&self, run_id: i64, tile_id: Option<i64>,
                              gpu_temp: f32, gpu_util: f32, gpu_memory_mb: f32,
                              cpu_util: f32, stage: &str, duration: f32) -> Result<()> {
        self.conn.execute(
            "INSERT INTO system_metrics (
                run_id, tile_id, gpu_temperature_c, gpu_utilization_percent,
                gpu_memory_used_mb, cpu_utilization_percent,
                processing_stage, stage_duration_seconds
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                run_id,
                tile_id.unwrap_or(0),
                gpu_temp,
                gpu_util,
                gpu_memory_mb,
                cpu_util,
                stage,
                duration,
            ),
        )?;
        Ok(())
    }

    /// Get all detections for analysis
    pub fn get_all_detections(&self, run_id: i64) -> Result<Vec<crate::scanner::Detection>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM raw_detections WHERE run_id = ?1"
        )?;
        
        let detections = stmt.query_map([run_id], |row| {
            Ok(Detection {
                id: row.get(0)?,
                pixel_row: row.get(3)?,
                pixel_col: row.get(4)?,
                utm_easting: row.get(5)?,
                utm_northing: row.get(6)?,
                wgs84_lat: row.get(8)?,
                wgs84_lon: row.get(9)?,
                grid_ref: row.get(10)?,
                aluminum_ratio: row.get(11)?,
                thermal_delta: row.get(12)?,
                score: row.get(13)?,
                base_score: row.get(13)?,
                classification: row.get(14)?,
                // ... fill in rest
                estimated_length_ft: 0.0,
                estimated_mass_tons: 0.0,
                source_tile: String::new(),
                anchor_lock_offset: String::new(),
                utm: crate::coordinate::UTMCoordinate {
                    easting: 0.0,
                    northing: 0.0,
                    zone: 16,
                },
                wgs84: crate::coordinate::WGS84Coordinate {
                    lat: 0.0,
                    lon: 0.0,
                },
                pass_count: 1,
                sensor_locks: Vec::new(),
                lock_level: String::from("single"),
                is_surface_target: false,
                needs_examination: false,
                examination_reason: String::new(),
            })
        })?;
        
        detections.collect()
    }

    /// Export detections to KML
    pub fn export_to_kml(&self, output_path: &str, min_confidence: f32) -> Result<()> {
        use std::fs::File;
        use std::io::Write;
        
        let mut kml = String::new();
        kml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        kml.push_str("<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n");
        kml.push_str("<Document>\n");
        kml.push_str("  <name>CESAROPS Detections</name>\n");
        
        let mut stmt = self.conn.prepare(
            "SELECT wgs84_lon, wgs84_lat, aluminum_ratio, thermal_z_score, 
                    base_score, raw_classification, grid_ref, acquisition_date
             FROM v_detections_full
             WHERE base_score >= ?1
             ORDER BY base_score DESC"
        )?;
        
        kml.push_str("  <Folder>\n");
        kml.push_str("    <name>All Detections</name>\n");
        
        let mut count = 0;
        let detections = stmt.query_map([min_confidence], |row| {
            Ok((
                row.get::<_, f64>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, f32>(2)?,
                row.get::<_, f32>(3)?,
                row.get::<_, f32>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?;
        
        for det in detections {
            if let Ok((lon, lat, alum, therm, score, class, grid, date)) = det {
                count += 1;
                let color = if score > 0.8 { "ff0000ff" }
                           else if score > 0.6 { "ff00ffff" }
                           else { "ffffff00" };
                
                kml.push_str(&format!("    <Placemark>\n"));
                kml.push_str(&format!("      <name>Detection_{:05}</name>\n", count));
                kml.push_str("      <description>\n");
                kml.push_str("        <![CDATA[\n");
                kml.push_str(&format!("          <h3>CESAROPS Detection</h3>\n"));
                kml.push_str(&format!("          <p><b>Score:</b> {:.3}</p>\n", score));
                kml.push_str(&format!("          <p><b>Classification:</b> {}</p>\n", class));
                kml.push_str(&format!("          <p><b>Aluminum Ratio:</b> {:.3}</p>\n", alum));
                kml.push_str(&format!("          <p><b>Thermal Z:</b> {:.3}</p>\n", therm));
                kml.push_str(&format!("          <p><b>Grid Ref:</b> {}</p>\n", grid));
                kml.push_str(&format!("          <p><b>Date:</b> {}</p>\n", date));
                kml.push_str("        ]]>");
                kml.push_str("      </description>\n");
                kml.push_str(&format!("      <Point>\n"));
                kml.push_str(&format!("        <coordinates>{},{},0</coordinates>\n", lon, lat));
                kml.push_str("      </Point>\n");
                kml.push_str("    </Placemark>\n");
            }
        }
        
        kml.push_str("  </Folder>\n");
        kml.push_str("</Document>\n");
        kml.push_str("</kml>\n");
        
        let mut file = File::create(output_path)?;
        file.write_all(kml.as_bytes())?;
        
        println!("Exported {} detections to {}", count, output_path);
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_database_creation() {
        let db_path = Path::new("test_cesarops.db");
        
        // Create database
        let logger = DatabaseLogger::new(db_path).unwrap();
        
        // Verify tables exist
        let mut stmt = logger.conn.prepare(
            "SELECT name FROM sqlite_master WHERE type='table'"
        ).unwrap();
        
        let tables: Vec<String> = stmt.query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        
        assert!(tables.contains(&"scan_runs".to_string()));
        assert!(tables.contains(&"tiles_processed".to_string()));
        assert!(tables.contains(&"raw_detections".to_string()));
        assert!(tables.contains(&"clustered_targets".to_string()));
        
        // Cleanup
        fs::remove_file(db_path).unwrap();
    }
}
