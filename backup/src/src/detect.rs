use ndarray::Array2;
use crate::config::{DetectionMode, MissionConfig, Thresholds};

/// Represents a detected anomaly.
#[derive(Debug, Clone)]
pub struct Anomaly {
    pub tile_id: String,
    pub center_x: usize,
    pub center_y: usize,
    pub confidence: f32,
    pub value: f32,
    pub area_px: usize,
}

/// Runs the unified detection pipeline on a single tile's processed data.
pub fn detect_anomalies(
    tile_id: &str,
    data: &Array2<f32>,
    config: &MissionConfig,
) -> Vec<Anomaly> {
    let thresholds = &config.thresholds;
    let mut anomalies = Vec::new();

    // 1. Thresholding
    // We create a binary mask of potential anomalies
    let mut mask = Array2::<bool>::zeros(data.shape());
    
    for ((y, x), &val) in data.indexed_iter() {
        let is_anomaly = match config.detection_mode {
            DetectionMode::HydrocarbonSheen => {
                // SWIR/NIR ratio is low for oil
                val < thresholds.max_value && val > thresholds.min_value
            }
            DetectionMode::ThermalSink => {
                // Cold spot: lower than mean
                val < thresholds.min_value
            }
            DetectionMode::ClearWater => {
                // High reflectance in blue/green
                val > thresholds.min_value
            }
            DetectionMode::SedimentPlume => {
                // High turbidity: high NIR/Red ratio
                val > thresholds.min_value
            }
            DetectionMode::SurfaceRipple => {
                // Texture variance: high
                val > thresholds.min_value
            }
            DetectionMode::Glint => {
                // High reflectance in all bands
                val > thresholds.min_value
            }
            DetectionMode::Custom(_) => {
                // Default to min_value threshold
                val > thresholds.min_value
            }
        };
        
        if is_anomaly {
            mask[[y, x]] = true;
        }
    }

    // 2. Connected Component Analysis (Simplified)
    let mut visited = Array2::<bool>::zeros(data.shape());
    
    for y in 0..data.shape()[0] {
        for x in 0..data.shape()[1] {
            if mask[[y, x]] && !visited[[y, x]] {
                // Start new component
                let mut component = Vec::new();
                let mut stack = vec![(x, y)];
                
                while let Some((cx, cy)) = stack.pop() {
                    if cx >= data.shape()[1] || cy >= data.shape()[0] { continue; }
                    if visited[[cy, cx]] || !mask[[cy, cx]] { continue; }
                    
                    visited[[cy, cx]] = true;
                    component.push((cx, cy));
                    
                    // Add neighbors
                    stack.push((cx + 1, cy));
                    stack.push((cx - 1, cy));
                    stack.push((cx, cy + 1));
                    stack.push((cx, cy - 1));
                }

                let area = component.len();
                if area >= thresholds.min_area_px {
                    // Compute center coordinates before moving
                    let cx: usize = component.iter().map(|(x, _)| x).sum::<usize>() / area;
                    let cy: usize = component.iter().map(|(_, y)| y).sum::<usize>() / area;
                    
                    // Compute average value and confidence before creating Anomaly
                    let sum_val: f32 = component.iter().map(|(x, y)| data[[*y, *x]]).sum::<f32>();
                    let avg_val = sum_val / area as f32;
                    
                    let range = thresholds.max_value - thresholds.min_value;
                    let raw_confidence = (avg_val - thresholds.min_value) / range.max(1e-6);
                    let confidence = raw_confidence.clamp(0.0, 1.0);

                    anomalies.push(Anomaly {
                        tile_id: tile_id.to_string(),
                        center_x: cx,
                        center_y: cy,
                        confidence,
                        value: avg_val,
                        area_px: area,
                    });
                }
            }
        }
    }

    anomalies
}
