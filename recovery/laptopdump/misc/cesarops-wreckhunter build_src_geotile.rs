// GeoTile Container with GDAL-based Tiling Support
// Implements 512x512 tiling with configurable overlap to prevent math drift
// Preserves geotransform and CRS for all tile operations

use serde::{Deserialize, Serialize};

/// Standard tile size for processing - prevents math precision issues
pub const TILE_SIZE: usize = 512;
/// Default overlap between tiles (10% of tile size)
pub const DEFAULT_OVERLAP_PERCENT: f64 = 0.10;

/// Simple GeoTile container preserving affine geotransform and CRS.
/// Affine uses GDAL geotransform convention:
/// [top_left_x, pixel_width, rot_x, top_left_y, rot_y, pixel_height]
/// 
/// For north-up images: [top_left_x, pixel_width, 0, top_left_y, 0, -pixel_height]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoTile {
    pub width: usize,
    pub height: usize,
    /// Row-major flat buffer: index = row * width + col
    pub data: Vec<f32>,
    /// GDAL-style geotransform: [gt0, gt1, gt2, gt3, gt4, gt5]
    /// World X = gt0 + col*gt1 + row*gt2
    /// World Y = gt3 + col*gt4 + row*gt5
    pub geotransform: [f64; 6],
    /// CRS string, e.g. "EPSG:32616" (UTM Zone 16N) or "EPSG:4326"
    pub crs: String,
    /// Optional band metadata
    pub band_index: usize,
    pub band_name: String,
}

impl GeoTile {
    /// Create a new GeoTile from raw data (row-major order)
    pub fn new(
        width: usize,
        height: usize,
        data: Vec<f32>,
        geotransform: [f64; 6],
        crs: &str,
    ) -> Self {
        assert_eq!(data.len(), width * height, "Data length must match width*height");
        Self {
            width,
            height,
            data,
            geotransform,
            crs: crs.to_string(),
            band_index: 0,
            band_name: String::new(),
        }
    }

    /// Create a GeoTile with band metadata
    pub fn with_band(
        width: usize,
        height: usize,
        data: Vec<f32>,
        geotransform: [f64; 6],
        crs: &str,
        band_index: usize,
        band_name: &str,
    ) -> Self {
        Self {
            width,
            height,
            data,
            geotransform,
            crs: crs.to_string(),
            band_index,
            band_name: band_name.to_string(),
        }
    }

    /// Get pixel value at (row, col) with bounds checking
    pub fn get(&self, row: usize, col: usize) -> Option<f32> {
        if row < self.height && col < self.width {
            Some(self.data[row * self.width + col])
        } else {
            None
        }
    }

    /// Set pixel value at (row, col)
    pub fn set(&mut self, row: usize, col: usize, value: f32) -> bool {
        if row < self.height && col < self.width {
            self.data[row * self.width + col] = value;
            true
        } else {
            false
        }
    }

    /// Get reference to underlying data slice (suitable for GPU upload)
    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }

    /// Get mutable reference to underlying data
    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        &mut self.data
    }

    /// Convert pixel (row, col) to world coordinates (x, y)
    /// Returns (y, x) = (latitude-like, longitude-like) for geographic CRS
    /// Returns (northing, easting) for projected CRS like UTM
    pub fn pixel_to_xy(&self, row: usize, col: usize) -> (f64, f64) {
        let gt = self.geotransform;
        // Add 0.5 to get pixel center
        let x = gt[0] + (col as f64 + 0.5) * gt[1] + (row as f64 + 0.5) * gt[2];
        let y = gt[3] + (col as f64 + 0.5) * gt[4] + (row as f64 + 0.5) * gt[5];
        (y, x)
    }

    /// Convert pixel (row, col) to UTM coordinates directly
    /// Assumes geotransform is already in UTM or converts via CRS
    pub fn pixel_to_utm(&self, row: usize, col: usize) -> (f64, f64) {
        let (y, x) = self.pixel_to_xy(row, col);
        (x, y) // Return (easting, northing)
    }

    /// Convert world coordinates (x, y) back to pixel (row, col)
    /// Returns fractional pixel coordinates for sub-pixel precision
    pub fn xy_to_pixel(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let gt = self.geotransform;
        // Solve: x = gt0 + col*gt1 + row*gt2
        //        y = gt3 + col*gt4 + row*gt5
        let a = gt[1];
        let b = gt[2];
        let c = gt[4];
        let d = gt[5];
        let det = a * d - b * c;
        
        if det.abs() < 1e-12 {
            // Degenerate case: assume no rotation/shear
            if a.abs() < 1e-12 || d.abs() < 1e-12 {
                return None;
            }
            let col = (x - gt[0]) / a - 0.5;
            let row = (y - gt[3]) / d - 0.5;
            return Some((row, col));
        }
        
        let dx = x - gt[0];
        let dy = y - gt[3];
        let col = (d * dx - b * dy) / det - 0.5;
        let row = (-c * dx + a * dy) / det - 0.5;
        Some((row, col))
    }

    /// Extract a rectangular chunk with adjusted geotransform
    /// 
    /// # Arguments
    /// * `col_start` - Starting column (x) in pixels
    /// * `row_start` - Starting row (y) in pixels
    /// * `width` - Width of chunk in pixels
    /// * `height` - Height of chunk in pixels
    /// 
    /// # Returns
    /// New GeoTile with adjusted geotransform preserving world coordinates
    pub fn chunk(
        &self,
        col_start: usize,
        row_start: usize,
        width: usize,
        height: usize,
    ) -> Option<GeoTile> {
        if col_start >= self.width || row_start >= self.height {
            return None;
        }

        let col_end = (col_start + width).min(self.width);
        let row_end = (row_start + height).min(self.height);

        let new_width = col_end - col_start;
        let new_height = row_end - row_start;

        // Copy pixel data
        let mut chunk_data = Vec::with_capacity(new_width * new_height);
        for row in row_start..row_end {
            let src_offset = row * self.width + col_start;
            chunk_data.extend_from_slice(&self.data[src_offset..src_offset + new_width]);
        }

        // Compute new geotransform for the chunk
        // Shift top-left corner by the pixel offset
        let gt = self.geotransform;
        let new_gt = [
            gt[0] + (col_start as f64) * gt[1] + (row_start as f64) * gt[2], // new top-left X
            gt[1], // pixel width unchanged
            gt[2], // rotation X unchanged
            gt[3] + (col_start as f64) * gt[4] + (row_start as f64) * gt[5], // new top-left Y
            gt[4], // rotation Y unchanged
            gt[5], // pixel height unchanged
        ];

        Some(GeoTile {
            width: new_width,
            height: new_height,
            data: chunk_data,
            geotransform: new_gt,
            crs: self.crs.clone(),
            band_index: self.band_index,
            band_name: self.band_name.clone(),
        })
    }

    /// Generate overlapping tile grid for processing
    /// 
    /// This creates a grid of 512x512 tiles with configurable overlap
    /// to prevent edge artifacts and math precision drift.
    /// 
    /// # Arguments
    /// * `overlap_percent` - Overlap between tiles as percentage (0.0-1.0)
    /// 
    /// # Returns
    /// Vector of (col_start, row_start, width, height) for each tile
    pub fn generate_tile_grid(&self, overlap_percent: f64) -> Vec<(usize, usize, usize, usize)> {
        let overlap_pixels = ((TILE_SIZE as f64) * overlap_percent).round() as usize;
        let stride = TILE_SIZE - overlap_pixels;
        
        let mut tiles = Vec::new();
        
        let num_cols = ((self.width + stride - 1) / stride).max(1);
        let num_rows = ((self.height + stride - 1) / stride).max(1);
        
        for row_idx in 0..num_rows {
            for col_idx in 0..num_cols {
                let col_start = col_idx * stride;
                let row_start = row_idx * stride;
                
                // Calculate actual tile size (may be smaller at edges)
                let tile_width = (TILE_SIZE).min(self.width - col_start);
                let tile_height = (TILE_SIZE).min(self.height - row_start);
                
                tiles.push((col_start, row_start, tile_width, tile_height));
            }
        }
        
        tiles
    }

    /// Extract all tiles for processing with overlap
    /// 
    /// Returns a vector of GeoTiles, each 512x512 (or smaller at edges)
    /// with proper geotransform preservation.
    pub fn extract_tiles(&self, overlap_percent: f64) -> Vec<GeoTile> {
        let tile_coords = self.generate_tile_grid(overlap_percent);
        
        tile_coords
            .into_iter()
            .filter_map(|(col, row, w, h)| self.chunk(col, row, w, h))
            .collect()
    }

    /// Write a JSON sidecar with metadata
    pub fn write_sidecar_json(&self, path: &std::path::Path) -> std::io::Result<()> {
        let meta = serde_json::json!({
            "crs": self.crs,
            "geotransform": self.geotransform,
            "width": self.width,
            "height": self.height,
            "band_index": self.band_index,
            "band_name": self.band_name,
            "data_length": self.data.len(),
        });
        std::fs::write(path, serde_json::to_string_pretty(&meta)?)
    }

    /// Calculate statistics (min, max, mean, stddev)
    pub fn statistics(&self) -> (f32, f32, f32, f32) {
        if self.data.is_empty() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        let mut sum = 0.0f32;
        
        for &val in &self.data {
            if val.is_finite() {
                min = min.min(val);
                max = max.max(val);
                sum += val;
            }
        }
        
        let mean = sum / self.data.len() as f32;
        
        let variance = self.data.iter()
            .filter(|&&v| v.is_finite())
            .map(|&v| (v - mean).powi(2))
            .sum::<f32>() / self.data.len() as f32;
        
        let stddev = variance.sqrt();
        
        (min, max, mean, stddev)
    }

    /// Normalize data to 0-1 range
    pub fn normalize(&mut self) {
        let (min, max, _, _) = self.statistics();
        let range = max - min;
        
        if range > 1e-6 {
            for val in &mut self.data {
                *val = (*val - min) / range;
            }
        }
    }

    /// Apply Z-score normalization
    pub fn z_score_normalize(&mut self) {
        let (_, _, mean, stddev) = self.statistics();
        
        if stddev > 1e-6 {
            for val in &mut self.data {
                *val = (*val - mean) / stddev;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_creation() {
        let data = vec![1.0f32; 512 * 512];
        let gt = [450000.0, 30.0, 0.0, 4700000.0, 0.0, -30.0];
        let tile = GeoTile::new(512, 512, data, gt, "EPSG:32616");
        
        assert_eq!(tile.width, 512);
        assert_eq!(tile.height, 512);
        assert_eq!(tile.data.len(), 512 * 512);
    }

    #[test]
    fn test_pixel_to_xy() {
        let data = vec![0.0f32; 100 * 100];
        let gt = [100.0, 1.0, 0.0, 200.0, 0.0, -1.0];
        let tile = GeoTile::new(100, 100, data, gt, "EPSG:4326");
        
        // Top-left pixel center
        let (y, x) = tile.pixel_to_xy(0, 0);
        assert!((x - 100.5).abs() < 0.01);
        assert!((y - 199.5).abs() < 0.01);
    }

    #[test]
    fn test_chunk_preserves_geotransform() {
        let mut data = vec![0.0f32; 100 * 100];
        data[50 * 100 + 50] = 42.0;
        let gt = [1000.0, 1.0, 0.0, 2000.0, 0.0, -1.0];
        let tile = GeoTile::new(100, 100, data, gt, "EPSG:32616");
        
        // Extract chunk containing the special pixel
        let chunk = tile.chunk(45, 45, 20, 20).expect("Chunk failed");
        
        // Pixel (5, 5) in chunk should be pixel (50, 50) in original
        assert_eq!(chunk.get(5, 5), Some(42.0));
        
        // Verify geotransform shift
        let (orig_y, orig_x) = tile.pixel_to_xy(50, 50);
        let (chunk_y, chunk_x) = chunk.pixel_to_xy(5, 5);
        
        assert!((orig_x - chunk_x).abs() < 1e-6);
        assert!((orig_y - chunk_y).abs() < 1e-6);
    }

    #[test]
    fn test_tile_grid_generation() {
        let data = vec![0.0f32; 1000 * 1000];
        let gt = [0.0, 1.0, 0.0, 1000.0, 0.0, -1.0];
        let tile = GeoTile::new(1000, 1000, data, gt, "EPSG:32616");
        
        let tiles = tile.generate_tile_grid(0.1); // 10% overlap
        
        // Should generate multiple overlapping tiles
        assert!(!tiles.is_empty());
        
        // First tile should start at origin
        assert_eq!(tiles[0], (0, 0, 512, 512));
    }

    #[test]
    fn test_statistics() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let gt = [0.0, 1.0, 0.0, 0.0, 0.0, -1.0];
        let tile = GeoTile::new(5, 1, data, gt, "EPSG:4326");
        
        let (min, max, mean, stddev) = tile.statistics();
        
        assert!((min - 1.0).abs() < 1e-5);
        assert!((max - 5.0).abs() < 1e-5);
        assert!((mean - 3.0).abs() < 1e-5);
        assert!(stddev > 1.0);
    }
}
