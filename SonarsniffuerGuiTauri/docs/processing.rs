//! Main signal processing pipeline.
//!
//! Applies the full enhancement stack:
//! Raw → TVG → Log Compression → Filtering → Histogram Eq → Colormap

use crate::garmin_rsd_parser::Ping;
use crate::video_enhanced::{
    filters, statistics::DatasetStatistics, tvg, ColorLUT, SonarProcessingParams,
};
use std::collections::HashMap;

/// Processed frame data ready for video encoding.
#[derive(Debug, Clone)]
pub struct ProcessedFrame {
    /// RGB pixels (width × height × 3)
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Intermediate processing result (one frame).
#[derive(Debug)]
struct IntermediateFrame {
    /// Grayscale intensities (0.0-1.0 normalized)
    intensities: Vec<f32>,
    width: usize,
    height: usize,
}

/// Apply full processing pipeline to ping dataset.
///
/// Returns a vector of processed frames ready for encoding.
pub fn apply_processing_pipeline(
    pings: &[Ping],
    params: &SonarProcessingParams,
    stats: &DatasetStatistics,
) -> anyhow::Result<Vec<ProcessedFrame>> {
    use anyhow::Context;
    
    // Filter to primary channel
    let primary_pings: Vec<&Ping> = pings
        .iter()
        .filter(|p| p.channel == stats.primary_channel)
        .collect();
    
    if primary_pings.is_empty() {
        anyhow::bail!("No pings found for primary channel {}", stats.primary_channel);
    }
    
    // Determine frame dimensions
    let width = stats.max_samples;
    let height = params.video_height as usize;
    let total_frames = (primary_pings.len() + height - 1) / height;
    
    // Precompute TVG LUT (if enabled)
    let tvg_lut = tvg::precompute_tvg_lut(width, params);
    
    // Generate colormap LUT
    let colormap_lut = params.colormap.generate_lut();
    
    // Determine dynamic range
    let (floor_db, ceiling_db) = if params.use_adaptive_range {
        // Convert percentile values to dB
        let floor = if stats.percentile_floor > 0.0 {
            20.0 * stats.percentile_floor.log10()
        } else {
            params.noise_floor_db
        };
        let ceiling = if stats.percentile_ceiling > 0.0 {
            20.0 * stats.percentile_ceiling.log10()
        } else {
            params.signal_ceiling_db
        };
        (floor, ceiling)
    } else {
        (params.noise_floor_db, params.signal_ceiling_db)
    };
    
    // Process frames
    let mut frames = Vec::with_capacity(total_frames);
    
    for frame_idx in 0..total_frames {
        let ping_start = frame_idx * height;
        let ping_end = (ping_start + height).min(primary_pings.len());
        let frame_pings = &primary_pings[ping_start..ping_end];
        
        // Build intermediate frame (TVG + log compression)
        let intermediate = build_intermediate_frame(
            frame_pings,
            width,
            height,
            &tvg_lut,
            params,
            floor_db,
            ceiling_db,
        )?;
        
        // Apply filtering
        let filtered = filters::apply_filters(
            &intermediate.intensities,
            intermediate.width,
            intermediate.height,
            params,
        );
        
        // Apply histogram equalization (if enabled)
        let enhanced = if params.histogram_equalization {
            histogram_equalize(&filtered, intermediate.width, intermediate.height)
        } else {
            filtered
        };
        
        // Apply CLAHE (if enabled)
        let enhanced = if params.clahe_enabled {
            apply_clahe(
                &enhanced,
                intermediate.width,
                intermediate.height,
                params.clahe_tile_size,
                params.clahe_clip_limit,
            )
        } else {
            enhanced
        };
        
        // Apply colormap and convert to RGB
        let rgb_pixels = apply_colormap(&enhanced, &colormap_lut);
        
        frames.push(ProcessedFrame {
            pixels: rgb_pixels,
            width: intermediate.width as u32,
            height: intermediate.height as u32,
        });
    }
    
    Ok(frames)
}

/// Build intermediate frame with TVG correction and log compression.
fn build_intermediate_frame(
    pings: &[&Ping],
    width: usize,
    height: usize,
    tvg_lut: &[f32],
    params: &SonarProcessingParams,
    floor_db: f32,
    ceiling_db: f32,
) -> anyhow::Result<IntermediateFrame> {
    let mut intensities = vec![0.0f32; width * height];
    
    for (row, ping) in pings.iter().enumerate() {
        // Apply TVG correction
        let corrected = tvg::apply_tvg_lut(&ping.samples, tvg_lut);
        
        // Apply logarithmic compression
        let compressed = if params.log_compression {
            log_compress(&corrected, floor_db, ceiling_db)
        } else {
            // Linear normalization
            let max_val = corrected.iter().copied().fold(0.0f32, f32::max).max(1.0);
            corrected.iter().map(|&v| v / max_val).collect()
        };
        
        // Write to frame buffer (pad or truncate to width)
        for col in 0..width {
            let value = if col < compressed.len() {
                compressed[col]
            } else {
                0.0 // Pad with zeros
            };
            intensities[row * width + col] = value;
        }
    }
    
    // Fill remaining rows (if frame not full)
    // Leave as zeros (will appear as noise floor after colormap)
    
    Ok(IntermediateFrame {
        intensities,
        width,
        height: pings.len(),
    })
}

/// Apply logarithmic compression: 20*log10(value) scaled to [0, 1].
fn log_compress(values: &[f32], floor_db: f32, ceiling_db: f32) -> Vec<f32> {
    const EPSILON: f32 = 1e-10;
    
    values
        .iter()
        .map(|&v| {
            let db = 20.0 * (v + EPSILON).log10();
            ((db - floor_db) / (ceiling_db - floor_db)).clamp(0.0, 1.0)
        })
        .collect()
}

/// Apply global histogram equalization.
fn histogram_equalize(data: &[f32], width: usize, height: usize) -> Vec<f32> {
    // Build histogram (256 bins)
    let mut hist = [0u32; 256];
    for &val in data {
        let bin = (val * 255.0).clamp(0.0, 255.0) as usize;
        hist[bin] += 1;
    }
    
    // Compute CDF
    let total = (width * height) as f32;
    let mut cdf = [0.0f32; 256];
    let mut cumsum = 0u32;
    for i in 0..256 {
        cumsum += hist[i];
        cdf[i] = cumsum as f32 / total;
    }
    
    // Apply equalization
    data.iter()
        .map(|&val| {
            let bin = (val * 255.0).clamp(0.0, 255.0) as usize;
            cdf[bin]
        })
        .collect()
}

/// Apply CLAHE (Contrast-Limited Adaptive Histogram Equalization).
///
/// Simplified implementation: divide image into tiles, equalize each tile, interpolate.
fn apply_clahe(data: &[f32], width: usize, height: usize, tile_size: usize, clip_limit: f32) -> Vec<f32> {
    // For simplicity, just return global histogram equalization
    // Full CLAHE requires bilinear interpolation between tiles (complex)
    // TODO: Implement proper CLAHE if needed
    histogram_equalize(data, width, height)
}

/// Apply colormap LUT to normalized intensities.
fn apply_colormap(intensities: &[f32], lut: &ColorLUT) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(intensities.len() * 3);
    
    for &intensity in intensities {
        let idx = (intensity * 255.0).clamp(0.0, 255.0) as usize;
        let (r, g, b) = lut[idx.min(255)];
        rgb.push(r);
        rgb.push(g);
        rgb.push(b);
    }
    
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_log_compress() {
        let values = vec![1.0, 10.0, 100.0, 1000.0];
        let compressed = log_compress(&values, -60.0, 60.0);
        
        // Should be monotonically increasing
        assert!(compressed[0] < compressed[1]);
        assert!(compressed[1] < compressed[2]);
        assert!(compressed[2] < compressed[3]);
        
        // Should be in [0, 1]
        for &v in &compressed {
            assert!(v >= 0.0 && v <= 1.0);
        }
    }
    
    #[test]
    fn test_histogram_equalize() {
        // Uniform distribution should remain relatively uniform
        let data: Vec<f32> = (0..256).map(|i| i as f32 / 255.0).collect();
        let eq = histogram_equalize(&data, 16, 16);
        
        // Check range
        let min = eq.iter().copied().fold(f32::MAX, f32::min);
        let max = eq.iter().copied().fold(f32::MIN, f32::max);
        assert!(min >= 0.0 && max <= 1.0);
    }
}
