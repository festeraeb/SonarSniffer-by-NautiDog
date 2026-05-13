use ndarray::Array2;
use crate::config::{DetectionMode, MissionConfig};

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

    // 1. Thresholding — create binary mask using static dimensions
    let dims = data.dim();
    
    // Build mask as Vec<Vec<bool>> since Array2::<bool>::zeros doesn't work with bool
    let mut mask: Vec<Vec<bool>> = vec![vec![false; dims.1]; dims.0];
    
    for ((y, x), &val) in data.indexed_iter() {
        let is_anomaly = match config.detection_mode {
            DetectionMode::HydrocarbonSheen => {
                val < thresholds.max_value && val > thresholds.min_value
            }
            DetectionMode::ThermalSink => {
                val < thresholds.min_value
            }
            DetectionMode::ClearWater => {
                val > thresholds.min_value
            }
            DetectionMode::SedimentPlume => {
                val > thresholds.min_value
            }
            DetectionMode::SurfaceRipple => {
                val > thresholds.min_value
            }
            DetectionMode::Glint => {
                val > thresholds.min_value
            }
            DetectionMode::Custom(_) => {
                val > thresholds.min_value
            }
        };
        
        if is_anomaly {
            mask[y][x] = true;
        }
    }

    // 2. Connected Component Analysis using visited tracking
    let mut visited: Vec<Vec<bool>> = vec![vec![false; dims.1]; dims.0];
    
    for y in 0..dims.0 {
        for x in 0..dims.1 {
            if mask[y][x] && !visited[y][x] {
                // Start new component via iterative flood fill
                let mut component = Vec::new();
                let mut stack: Vec<(usize, usize)> = vec![(x, y)];
                
                while let Some((cx, cy)) = stack.pop() {
                    // Bounds check
                    if cx >= dims.1 || cy >= dims.0 {
                        continue;
                    }
                    if visited[cy][cx] || !mask[cy][cx] {
                        continue;
                    }
                    
                    visited[cy][cx] = true;
                    component.push((cx, cy));
                    
                    // Add neighbors (4-connectivity) with safe underflow handling
                    stack.push((cx + 1, cy));
                    if cx > 0 { stack.push((cx - 1, cy)); }
                    if cy > 0 { stack.push((cx, cy - 1)); }
                    stack.push((cx, cy + 1));
                }

                if component.is_empty() {
                    continue;
                }

                // Compute anomaly properties from the connected component
                let area_px = component.len();
                let center_x: usize = component.iter().map(|(x, _)| x).sum::<usize>() / area_px;
                let center_y: usize = component.iter().map(|(_, y)| y).sum::<usize>() / area_px;
                
                // Compute confidence as proportion of threshold margin at each pixel
                let confidence_sum: f32 = component
                    .iter()
                    .map(|(cx, cy)| {
                        let val = data[[*cy, *cx]];
                        match config.detection_mode {
                            DetectionMode::HydrocarbonSheen => {
                                thresholds.max_value - val
                            }
                            _ => {
                                val - thresholds.min_value
                            }
                        }.max(0.0)
                    })
                    .sum();
                let confidence = confidence_sum / area_px as f32;
                
                // Get average value across the component
                let value: f32 = component
                    .iter()
                    .map(|(cx, cy)| data[[*cy, *cx]])
                    .sum::<f32>() / area_px as f32;

                anomalies.push(Anomaly {
                    tile_id: tile_id.to_string(),
                    center_x,
                    center_y,
                    confidence,
                    value,
                    area_px,
                });
            }
        }
    }

    // 3. Sort by confidence descending
    anomalies.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

    anomalies
}
