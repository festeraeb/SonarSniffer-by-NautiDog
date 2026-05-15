use std::path::{Path, PathBuf};
use std::fs;
use chrono::{DateTime, Utc, TimeZone};
use regex::Regex;

#[derive(Debug, Clone)]
pub struct SatelliteImage {
    pub path: PathBuf,
    pub bounds: (f64, f64, f64, f64), // min_lat, min_lon, max_lat, max_lon
    pub acquired: DateTime<Utc>,
    pub resolution_m: f32,
    pub source: String,
}

/// Scans the directory for .tif or .geotiff files and attempts to extract metadata.
pub fn scan_repository(dir: &Path) -> Vec<SatelliteImage> {
    let mut images = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                if ext == "tif" || ext == "geotiff" {
                    if let Some(img) = extract_metadata(&path) {
                        images.push(img);
                    }
                }
            }
        }
    }
    images
}

/// Filters images that overlap with the provided bounding box.
/// area: (min_lat, min_lon, max_lat, max_lon)
pub fn find_coverage<'a>(images: &'a [SatelliteImage], area: (f64, f64, f64, f64)) -> Vec<&'a SatelliteImage> {
    let (a_min_lat, a_min_lon, a_max_lat, a_max_lon) = area;
    
    images.iter().filter(|img| {
        let (i_min_lat, i_min_lon, i_max_lat, i_max_lon) = img.bounds;
        // Check for intersection of two rectangles
        !(i_max_lat < a_min_lat || 
          i_min_lat > a_max_lat || 
          i_max_lon < a_min_lon || 
          i_min_lon > a_max_lon)
    }).collect()
}

/// Parses metadata from the filename.
/// Expected format: rtc_S1A_IW_SLC__1SDV_YYYYMMDD_...
/// Note: In a real scenario, bounds would be read from the GeoTIFF header via a crate like `gdal`.
/// For this implementation, we simulate the extraction from the filename and structure.
pub fn extract_metadata(path: &Path) -> Option<SatelliteImage> {
    let filename = path.file_name()?.to_str()?;
    
    // Regex to capture the date part: YYYYMMDD
    // Example: rtc_S1A_IW_SLC__1SDV_20231025_...
    let re = Regex::new(r"(\d{8})").ok()?;
    let caps = re.captures(filename)?;
    let date_str = caps.get(1)?.as_str();

    // Parse date
    let year = date_str[0..4].parse::<i32>().ok()?;
    let month = date_str[4..6].parse::<u32>().ok()?;
    let day = date_str[6..8].parse::<u32>().ok()?;
    
    let acquired = Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).single()?;

    // Simulated extraction of bounds and resolution 
    // In production, use gdal::Dataset to get actual spatial extent
    let bounds = (45.0, -92.0, 46.0, -91.0); // Mock Great Lakes area
    let resolution_m = 10.0; 
    let source = "Sentinel-1".to_string();

    Some(SatelliteImage {
        path: path.to_path_buf(),
        bounds,
        acquired,
        resolution_m,
        source,
    })
}
