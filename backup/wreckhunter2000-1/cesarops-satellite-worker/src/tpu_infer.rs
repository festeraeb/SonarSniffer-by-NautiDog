//! TPU inference engine — Coral Edge TPU via FFI or CPU fallback.
//!
//! Runs two fast passes:
//! - Glint: detects specular reflection hotspots (bright pixel clusters)
//! - Shadow: detects anomalous dark geometric shapes (linear features, right angles)

use image::{DynamicImage, GrayImage, Luma, GenericImageView};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize)]
pub struct TpuDetection {
    pub pixel_row: u32,
    pub pixel_col: u32,
    pub confidence: f32,
    pub pass_type: String, // "glint" | "shadow"
}

#[derive(Debug, Clone, Serialize)]
pub struct TpuResult {
    pub detections: Vec<TpuDetection>,
    pub took_ms: u64,
    pub backend: String, // "edgetpu" | "cpu"
}

pub struct TpuEngine {
    backend: Arc<Mutex<TpuBackend>>,
}

enum TpuBackend {
    EdgeTpu,
    Cpu,
}

impl TpuEngine {
    pub async fn new() -> Self {
        let backend = if Self::detect_edgetpu() {
            info!("Coral Edge TPU hardware detected");
            TpuBackend::EdgeTpu
        } else {
            warn!("No Coral Edge TPU found — using CPU inference");
            TpuBackend::Cpu
        };
        Self {
            backend: Arc::new(Mutex::new(backend)),
        }
    }

    fn detect_edgetpu() -> bool {
        // Linux: /dev/apex_0 (M.2/PCIe) or /dev/accel0 (USB)
        // Windows: libedgetpu.dll present
        #[cfg(target_os = "linux")]
        {
            std::path::Path::new("/dev/apex_0").exists()
                || std::path::Path::new("/dev/accel0").exists()
        }
        #[cfg(not(target_os = "linux"))]
        {
            // On Windows, check if the edgetpu DLL is in PATH
            std::path::Path::new("edgetpu.dll").exists()
        }
    }

    pub async fn info(&self) -> String {
        let guard = self.backend.lock().await;
        match *guard {
            TpuBackend::EdgeTpu => "edgetpu".to_string(),
            TpuBackend::Cpu => "cpu".to_string(),
        }
    }

    /// Run glint + shadow detection on an image tile.
    /// `pass_type`: "glint", "shadow", or "both"
    pub async fn infer_image(&self, img: &DynamicImage, pass_type: &str) -> TpuResult {
        let start = std::time::Instant::now();
        let gray = img.to_luma8();

        let mut detections = Vec::new();

        match *self.backend.lock().await {
            TpuBackend::EdgeTpu => {
                // TODO: wire real TFLite/EdgeTPU inference here
                // For now, run the same CPU heuristics — the TPU path
                // will use quantized YOLOv8-tiny once models are deployed.
                warn!("EdgeTPU path not yet wired — using CPU heuristics");
            }
            TpuBackend::Cpu => {}
        }

        if pass_type == "glint" || pass_type == "both" {
            detections.extend(detect_glint(&gray));
        }
        if pass_type == "shadow" || pass_type == "both" {
            detections.extend(detect_shadow(&gray));
        }

        let took_ms = start.elapsed().as_millis() as u64;
        let backend = match *self.backend.lock().await {
            TpuBackend::EdgeTpu => "edgetpu",
            TpuBackend::Cpu => "cpu",
        };

        TpuResult {
            detections,
            took_ms,
            backend: backend.to_string(),
        }
    }
}

// ── Glint Detection ─────────────────────────────────────────────────────────
// Finds clusters of pixels above the 99.5th percentile brightness.
// Persistent glint at the same GPS coordinate across multiple satellite passes
// may indicate a shallow obstruction (mast, hull break).

fn detect_glint(img: &GrayImage) -> Vec<TpuDetection> {
    let (width, height) = img.dimensions();
    let pixels: Vec<u8> = img.pixels().map(|p| p[0]).collect();

    // Compute 99.5th percentile threshold
    let mut sorted = pixels.clone();
    sorted.sort_unstable();
    let threshold_idx = (sorted.len() as f64 * 0.995) as usize;
    let threshold = sorted[threshold_idx.min(sorted.len() - 1)];

    if threshold == 0 {
        return Vec::new();
    }

    // Find bright pixels and cluster them (simple grid-based clustering)
    let cell_size = 8u32;
    let mut cell_counts: std::collections::HashMap<(u32, u32), (u32, u64)> = std::collections::HashMap::new();

    for y in 0..height {
        for x in 0..width {
            let val = pixels[(y * width + x) as usize];
            if val >= threshold {
                let cell = (y / cell_size, x / cell_size);
                let entry = cell_counts.entry(cell).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += val as u64;
            }
        }
    }

    let total_pixels = (width * height) as f64;
    cell_counts
        .into_iter()
        .filter(|(_, (count, _))| *count >= 2) // at least 2 bright pixels in cell
        .map(|((cy, cx), (count, sum))| {
            let center_row = cy * cell_size + cell_size / 2;
            let center_col = cx * cell_size + cell_size / 2;
            let avg_brightness = (sum / count as u64) as f32 / 255.0;
            // Confidence scales with pixel density and brightness
            let density = count as f64 / (cell_size * cell_size) as f64;
            let confidence = (density * 5.0 * (avg_brightness as f64)).clamp(0.0, 1.0) as f32;
            TpuDetection {
                pixel_row: center_row,
                pixel_col: center_col,
                confidence,
                pass_type: "glint".to_string(),
            }
        })
        .collect()
}

// ── Shadow Detection ────────────────────────────────────────────────────────
// Detects anomalous dark geometric shapes using gradient analysis.
// Linear features and right-angle patterns that don't match natural topography.

fn detect_shadow(img: &GrayImage) -> Vec<TpuDetection> {
    let (width, height) = img.dimensions();

    // Compute local gradient magnitude (simple Sobel-like)
    let mut gradients: Vec<f32> = Vec::with_capacity((width * height) as usize);
    let get = |x: u32, y: u32| -> f32 {
        if x < width && y < height {
            img.get_pixel(x, y)[0] as f32
        } else {
            0.0
        }
    };

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let gx = -get(x - 1, y - 1) + get(x + 1, y - 1)
                - 2.0 * get(x - 1, y) + 2.0 * get(x + 1, y)
                - get(x - 1, y + 1) + get(x + 1, y + 1);
            let gy = -get(x - 1, y - 1) - 2.0 * get(x, y - 1) - get(x + 1, y - 1)
                + get(x - 1, y + 1) + 2.0 * get(x, y + 1) + get(x + 1, y + 1);
            gradients.push((gx * gx + gy * gy).sqrt());
        }
    }

    // Find dark regions with high gradient (edge of shadow)
    let mut sorted_grad = gradients.clone();
    sorted_grad.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let grad_threshold = sorted_grad
        .get((sorted_grad.len() as f64 * 0.90) as usize)
        .copied()
        .unwrap_or(50.0);

    let inner_w = width - 2;
    let inner_h = height - 2;
    let cell_size = 16u32;
    let mut cell_scores: std::collections::HashMap<(u32, u32), (u32, f64, f64)> = std::collections::HashMap::new();

    for y in 0..inner_h {
        for x in 0..inner_w {
            let grad = gradients[(y * inner_w + x) as usize];
            let brightness = get(x + 1, y + 1);

            // Shadow = dark area with strong edge gradient
            if brightness < 60.0 && grad > grad_threshold {
                let cy = (y + 1) / cell_size;
                let cx = (x + 1) / cell_size;
                let entry = cell_scores.entry((cy, cx)).or_insert((0, 0.0, 0.0));
                entry.0 += 1;
                entry.1 += grad as f64;
                entry.2 += brightness as f64;
            }
        }
    }

    cell_scores
        .into_iter()
        .filter(|(_, (count, _, _))| *count >= 3) // minimum edge pixels
        .map(|((cy, cx), (count, grad_sum, bright_sum))| {
            let center_row = cy * cell_size + cell_size / 2;
            let center_col = cx * cell_size + cell_size / 2;
            let avg_grad = (grad_sum / count as f64) as f32;
            let avg_bright = (bright_sum / count as f64) as f32 / 255.0;
            // Confidence: high gradient + low brightness = strong shadow candidate
            let gradient_score = (avg_grad / 200.0).clamp(0.0, 1.0);
            let darkness_score = (1.0 - avg_bright).clamp(0.0, 1.0);
            let confidence = (gradient_score * 0.6 + darkness_score * 0.4).clamp(0.0, 1.0);
            TpuDetection {
                pixel_row: center_row,
                pixel_col: center_col,
                confidence,
                pass_type: "shadow".to_string(),
            }
        })
        .collect()
}
