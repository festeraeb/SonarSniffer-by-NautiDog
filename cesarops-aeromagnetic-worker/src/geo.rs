//! Grid meta JSON (from Python export) → WGS84 for dipole hits.

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct GridMeta {
    pub width: u32,
    pub height: u32,
    pub transform: [f64; 6],
    #[serde(default)]
    pub source_tif: Option<String>,
}

impl GridMeta {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    /// Rasterio affine: lon = a*col + b*row + c, lat = d*col + e*row + f
    pub fn pixel_to_lon_lat(&self, col: u32, row: u32) -> (f64, f64) {
        let c = col as f64;
        let r = row as f64;
        let lon = self.transform[0] * c + self.transform[1] * r + self.transform[2];
        let lat = self.transform[3] * c + self.transform[4] * r + self.transform[5];
        (lon, lat)
    }
}
