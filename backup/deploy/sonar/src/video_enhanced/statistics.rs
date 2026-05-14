//! Dataset statistics computation for adaptive processing.
//!
//! Two-pass approach:
//! 1. **Pass 1** (this module): Analyze raw data to compute percentiles, histograms, gaps
//! 2. **Pass 2** (processing.rs): Apply corrections using computed statistics

use crate::garmin_rsd_parser::Ping;
use crate::video_enhanced::SonarProcessingParams;
use std::collections::HashMap;

/// Dataset statistics computed from raw ping data.
#[derive(Debug, Clone)]
pub struct DatasetStatistics {
    /// Number of pings in dataset
    pub total_pings: usize,
    
    /// Primary channel (most pings)
    pub primary_channel: u32,
    
    /// Maximum samples in any ping
    pub max_samples: usize,
    
    /// Raw intensity statistics (before TVG)
    pub raw_min: f32,
    pub raw_max: f32,
    pub raw_mean: f32,
    pub raw_stddev: f32,
    
    /// Percentile values for adaptive range
    pub percentile_floor: f32,  // e.g., P₀.₁
    pub percentile_ceiling: f32, // e.g., P₉₉.₉
    
    /// Histogram (256 bins) for equalization
    pub histogram: [u32; 256],
    
    /// Detected data gaps (ping_index, sample_start, sample_end)
    pub gaps: Vec<(usize, usize, usize)>,
}

/// Compute dataset statistics from pings.
pub fn compute_dataset_statistics(
    pings: &[Ping],
    params: &SonarProcessingParams,
) -> anyhow::Result<DatasetStatistics> {
    use anyhow::Context;
    
    if pings.is_empty() {
        anyhow::bail!("Cannot compute statistics on empty ping dataset");
    }
    
    // Find dominant channel
    let mut channel_counts: HashMap<u32, usize> = HashMap::new();
    for ping in pings {
        *channel_counts.entry(ping.channel).or_insert(0) += 1;
    }
    let primary_channel = *channel_counts
        .iter()
        .max_by_key(|(_, &count)| count)
        .context("No channels found")?
        .0;
    
    // Filter to primary channel
    let primary_pings: Vec<&Ping> = pings
        .iter()
        .filter(|p| p.channel == primary_channel)
        .collect();
    
    // Collect all samples from primary channel
    let mut all_samples = Vec::new();
    let mut max_samples = 0usize;
    
    for ping in &primary_pings {
        max_samples = max_samples.max(ping.samples.len());
        for &sample in &ping.samples {
            all_samples.push(sample as f32);
        }
    }
    
    if all_samples.is_empty() {
        anyhow::bail!("No samples found in primary channel");
    }
    
    // Compute basic statistics
    let raw_min = all_samples.iter().copied().fold(f32::MAX, f32::min);
    let raw_max = all_samples.iter().copied().fold(f32::MIN, f32::max);
    let raw_mean = all_samples.iter().sum::<f32>() / all_samples.len() as f32;
    
    let variance = all_samples
        .iter()
        .map(|&x| (x - raw_mean).powi(2))
        .sum::<f32>()
        / all_samples.len() as f32;
    let raw_stddev = variance.sqrt();
    
    // Compute percentiles (for adaptive range)
    let mut sorted = all_samples.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    
    let floor_idx = ((params.floor_percentile / 100.0) * sorted.len() as f32) as usize;
    let ceiling_idx = ((params.ceiling_percentile / 100.0) * sorted.len() as f32) as usize;
    
    let percentile_floor = sorted[floor_idx.min(sorted.len() - 1)];
    let percentile_ceiling = sorted[ceiling_idx.min(sorted.len() - 1)];
    
    // Build histogram (256 bins)
    let mut histogram = [0u32; 256];
    for &sample in &all_samples {
        let normalized = ((sample - raw_min) / (raw_max - raw_min)).clamp(0.0, 1.0);
        let bin = (normalized * 255.0) as usize;
        histogram[bin.min(255)] += 1;
    }
    
    // Detect gaps (consecutive zero or very low samples)
    let gaps = detect_gaps(&primary_pings, params.gap_threshold_samples);
    
    Ok(DatasetStatistics {
        total_pings: primary_pings.len(),
        primary_channel,
        max_samples,
        raw_min,
        raw_max,
        raw_mean,
        raw_stddev,
        percentile_floor,
        percentile_ceiling,
        histogram,
        gaps,
    })
}

/// Detect data gaps in ping samples.
///
/// Returns: Vec of (ping_index, gap_start_sample, gap_end_sample)
fn detect_gaps(pings: &[&Ping], threshold: usize) -> Vec<(usize, usize, usize)> {
    let mut gaps = Vec::new();
    
    for (ping_idx, ping) in pings.iter().enumerate() {
        let mut gap_start = None;
        
        for (sample_idx, &sample) in ping.samples.iter().enumerate() {
            let is_gap = sample < 5; // Very low intensity
            
            if is_gap && gap_start.is_none() {
                gap_start = Some(sample_idx);
            } else if !is_gap && gap_start.is_some() {
                let start = gap_start.unwrap();
                let length = sample_idx - start;
                if length >= threshold {
                    gaps.push((ping_idx, start, sample_idx - 1));
                }
                gap_start = None;
            }
        }
        
        // Handle gap extending to end of ping
        if let Some(start) = gap_start {
            let length = ping.samples.len() - start;
            if length >= threshold {
                gaps.push((ping_idx, start, ping.samples.len() - 1));
            }
        }
    }
    
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn make_test_ping(channel: u32, samples: Vec<u16>) -> Ping {
        Ping {
            file_offset: 0,
            sequence: 0,
            timestamp_ms: 0,
            latitude: 0.0,
            longitude: 0.0,
            depth_m: 0.0,
            depth_ft: 0.0,
            altitude_m: 0.0,
            temp_c: Some(20.0),
            beam_angle_deg: 0.0,
            channel,
            sample_count: samples.len(),
            sonar_offset: 0,
            sonar_size: samples.len() * 2,
            sample_format: "u16".to_string(),
            heading_deg: None,
            pitch_deg: None,
            roll_deg: None,
            hardware_gain: None,
            samples,
        }
    }
    
    #[test]
    fn test_primary_channel_selection() {
        let pings = vec![
            make_test_ping(1, vec![100; 10]),
            make_test_ping(1, vec![100; 10]),
            make_test_ping(2, vec![100; 10]),
            make_test_ping(1, vec![100; 10]),
        ];
        
        let params = SonarProcessingParams::default();
        let stats = compute_dataset_statistics(&pings, &params).unwrap();
        
        assert_eq!(stats.primary_channel, 1);
        assert_eq!(stats.total_pings, 3); // 3 pings on channel 1
    }
    
    #[test]
    fn test_gap_detection() {
        let ping = make_test_ping(
            1,
            vec![
                100, 100, 100,   // Normal
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // Gap (11 samples)
                100, 100, 100,   // Normal
            ],
        );
        
        let pings = vec![&ping];
        let gaps = detect_gaps(&pings, 10);
        
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0], (0, 3, 13)); // ping 0, samples 3-13
    }
}
