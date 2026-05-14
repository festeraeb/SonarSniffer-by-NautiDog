use std::sync::atomic::{AtomicU64, Ordering};

pub struct AtomicF32Array {
    // We just wrap AtomicU64 to represent Z-Buffered max priority overrides based on Slant Range
}

pub struct MosaicGrid {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub resolution: f64,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<AtomicU64>, // packed (sum, weight) as f32 pairs
}

impl MosaicGrid {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64, resolution: f64) -> Self {
        let width = ((max_x - min_x) / resolution).ceil() as usize + 1;
        let height = ((max_y - min_y) / resolution).ceil() as usize + 1;

        let total_pixels = width * height;
        let mut pixels = Vec::with_capacity(total_pixels);
        let initial_val = Self::pack(0.0, 0.0);

        for _ in 0..total_pixels {
            pixels.push(AtomicU64::new(initial_val));
        }

        Self { min_x, min_y, max_x, max_y, resolution, width, height, pixels }
    }

    #[inline(always)]
    fn pack(sum: f32, weight: f32) -> u64 {
        ((sum.to_bits() as u64) << 32) | (weight.to_bits() as u64)
    }

    #[inline(always)]
    fn unpack(bits: u64) -> (f32, f32) {
        let sum = f32::from_bits((bits >> 32) as u32);
        let weight = f32::from_bits(bits as u32);
        (sum, weight)
    }

    #[inline(always)]
    pub fn add_weighted_sample(&self, x: f64, y: f64, intensity: f32, weight: f32) {
        if x < self.min_x || x > self.max_x || y < self.min_y || y > self.max_y {
            return;
        }

        if weight <= 0.0 {
            return;
        }

        let px = ((x - self.min_x) / self.resolution) as usize;
        let py = ((y - self.min_y) / self.resolution) as usize;

        if px < self.width && py < self.height {
            let idx = py * self.width + px;
            let mut current = self.pixels[idx].load(Ordering::Relaxed);
            loop {
                let (sum, w) = Self::unpack(current);
                let new_sum = sum + intensity * weight;
                let new_w = w + weight;
                let new_bits = Self::pack(new_sum, new_w);
                match self.pixels[idx].compare_exchange_weak(current, new_bits, Ordering::Relaxed, Ordering::Relaxed) {
                    Ok(_) => break,
                    Err(e) => current = e,
                }
            }
        }
    }

    pub fn get_normalized_pixel(&self, px: usize, py: usize) -> f32 {
        let idx = py * self.width + px;
        let (sum, weight) = Self::unpack(self.pixels[idx].load(Ordering::Relaxed));
        if weight <= 0.0 { 0.0 } else { sum / weight }
    }

    pub fn bounds_latlon(&self) -> (f64, f64, f64, f64) {
        let (lat1, lon1) = crate::mosaic::projection::meters_to_latlon(self.min_x, self.min_y);
        let (lat2, lon2) = crate::mosaic::projection::meters_to_latlon(self.max_x, self.max_y);
        (
            lat1.min(lat2),
            lon1.min(lon2),
            lat1.max(lat2),
            lon1.max(lon2),
        )
    }

    pub fn build_image(&self, colormap: &str) -> image::RgbaImage {
        let mut img = image::RgbaImage::new(self.width as u32, self.height as u32);
        
        // 1. Gather all non-zero intensities to find a global P2 / P98 stretch
        let mut intensities = Vec::with_capacity(10000);
        for py in 0..self.height {
            for px in 0..self.width {
                let v = self.get_normalized_pixel(px, py);
                if v > 0.0 {
                    intensities.push(v);
                }
            }
        }
        
        let (mut p_min, mut p_max) = (0.0_f32, 255.0_f32); // defaults
        if !intensities.is_empty() {
            // Sample a subset if it's huge
            if intensities.len() > 100_000 {
                let step = intensities.len() / 20000;
                intensities = intensities.into_iter().step_by(step).collect();
            }
            intensities.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
            p_min = intensities[(intensities.len() as f32 * 0.02) as usize];
            p_max = intensities[(intensities.len() as f32 * 0.98) as usize];
        }
        // Avoid division by zero
        if p_max <= p_min {
            p_max = p_min + 1.0;
        }

        // Apply a global TVG-like lift for deeper pixels? 
        // We only have final intensities. We apply a basic Gamma of 0.65 to pop the contrast.
        let gamma = 0.65_f32;

        for y in 0..self.height {
            let img_y = (self.height - 1 - y) as u32;
            for x in 0..self.width {
                let v = self.get_normalized_pixel(x, y);
                if v > 0.0 {
                    // Linear stretch
                    let mut norm = (v - p_min) / (p_max - p_min);
                    norm = norm.clamp(0.0, 1.0);
                    // Gamma correction
                    norm = norm.powf(gamma);

                    let rgb = crate::outputs::apply_colormap(norm, colormap);
                    img.put_pixel(x as u32, img_y, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
                }
            }
        }
        img
    }
}
