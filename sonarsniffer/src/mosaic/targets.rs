use crate::garmin_rsd_parser::{Ping, ParseResult};
// use nauticuvs::filters::curvelet_denoising; // Will be activated via actual nauticuvs integration
use image::{ImageBuffer, Rgba};
use std::path::Path;

pub struct AcousticTarget {
    pub start_ping: usize,
    pub end_ping: usize,
    pub center_lat: f64,
    pub center_lon: f64,
}

impl AcousticTarget {
    pub fn extract_and_denoise_snippet(&self, parse_res: &ParseResult, channel_id: u32, out_path: &Path) {
        let pings: Vec<&Ping> = parse_res.pings.iter()
            .filter(|p| p.channel == channel_id)
            .collect();
            
        if self.start_ping >= pings.len() || self.end_ping >= pings.len() {
            return;
        }
        
        let width = pings[self.start_ping].samples.len();
        let height = self.end_ping - self.start_ping + 1;
        
        // Collect raw matrix
        let mut raw_matrix: Vec<f32> = Vec::with_capacity(width * height);
        for i in self.start_ping..=self.end_ping {
            let row = &pings[i].samples;
            for &s in row {
                raw_matrix.push(s as f32);
            }
        }
        
        // --- High-Resolution Curvelet Pass ---
        // (Placeholder for strictly integrating Nauticuvs curves)
        // let clean_matrix = curvelet_denoising(&raw_matrix, width, height, 2);
        
        let max_val = raw_matrix.iter().fold(0.0f32, |a, &b| a.max(b));
        
        let mut img = ImageBuffer::new(width as u32, height as u32);
        for y in 0..height {
            for x in 0..width {
                let v = raw_matrix[y * width + x];
                let norm = if max_val > 0.0 { (v / max_val * 255.0).min(255.0) as u8 } else { 0 };
                img.put_pixel(x as u32, y as u32, Rgba([norm, norm, norm, 255]));
            }
        }
        
        let _ = img.save(out_path);
    }
}
