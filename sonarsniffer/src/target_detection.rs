use serde::{Deserialize, Serialize};
// Assuming these are available in the crate context
// use crate::garmin_rsd_parser::ParseResult; 

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DetectionMode {
    Off,
    Basic,
    Advanced,
}

impl DetectionMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "basic" => DetectionMode::Basic,
            "advanced" => DetectionMode::Advanced,
            _ => DetectionMode::Off,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionSettings {
    pub mode: DetectionMode,
    pub min_size: u32,
    pub max_size: u32,
    pub sensitivity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub ping_index: usize,
    pub sample_start: usize,
    pub sample_end: usize,
    pub intensity: f64,
    pub estimated_depth_m: Option<f64>,
    pub longitude: f64,
    pub latitude: f64,
    pub classification: String,
    pub size_class: String,
    pub blob_area: f64,
    pub width_m: f64,
    pub length_m: f64,
    pub avg_intensity: f64,
    pub confidence: f64,
    pub depth_m: f64,
    pub range_m: f64,
    pub channel: u32,
    pub channel_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionSummary {
    pub total: usize,
    pub total_detections: usize,
    pub fish_count: usize,
    pub structure_count: usize,
    pub debris_count: usize,
    pub wreck_count: usize,
    pub detections: Vec<Detection>,
}

/// Internal helper to represent a cluster of high-intensity samples
struct BlobCandidate {
    ping_indices: Vec<usize>,
    sample_indices: Vec<usize>,
    avg_intensity: f64,
    max_intensity: f64,
}

/// Classifies a blob based on its area (number of samples)
fn classify_blob(area: f64) -> (String, String) {
    if area < 4.0 {
        ("Fish".to_string(), "Small".to_string())
    } else if area < 1000.0 {
        ("Structure".to_string(), "Medium".to_string())
    } else if area < 10000.0 {
        ("Debris".to_string(), "Large".to_string())
    } else {
        ("Wreck".to_string(), "Huge".to_string())
    }
}

pub fn detect_targets(
    parse: &ParseResult, 
    settings: &DetectionSettings
) -> DetectionSummary {
    if settings.mode == DetectionMode::Off {
        return DetectionSummary {
            total: 0, total_detections: 0, fish_count: 0,
            structure_count: 0, debris_count: 0, wreck_count: 0,
            detections: Vec::new(),
        };
    }

    // 1. Thresholding: Find samples above the noise floor
    // We use sensitivity to adjust the threshold relative to the mean intensity
    let mut all_samples = Vec::new();
    let mut mean_intensity = 0.0;
    let mut count = 0;

    for ping in &parse.pings {
        for &val in &ping.samples {
            mean_intensity += val as f64;
            count += 1;
        }
    }
    
    if count == 0 {
        return DetectionSummary { total: 0, total_detections: 0, fish_count: 0, structure_count: 0, debris_count: 0, wreck_count: 0, detections: Vec::new() };
    }
    mean_intensity /= count as f64;
    
    // Threshold is mean + (sensitivity factor * standard deviation proxy)
    let threshold = mean_intensity * (1.0 + settings.sensitivity as f64);

    // 2. Connected Component Labeling (Simplified for 1D-per-ping stream)
    // We group adjacent high-intensity samples within and across pings
    let mut detections = Vec::new();
    let mut visited = vec![false; parse.pings.len() * 1024]; // Assuming max samples per ping for simplicity, or use actual length

    // For a real implementation, we iterate through the data to find contiguous blocks
    // Here we simulate the extraction of blobs from the ParseResult
    for (p_idx, ping) in parse.pings.iter().enumerate() {
        let mut s_idx = 0;
        while s_idx < ping.samples.len() {
            if (ping.samples[s_idx] as f64) > threshold {
                let start_s = s_idx;
                let mut current_intensity_sum = 0.0;
                let mut max_i = 0.0;

                while s_idx < ping.samples.len() && (ping.samples[s_idx] as f64) > threshold {
                    let val = ping.samples[s_idx] as f64;
                    current_intensity_sum += val;
                    if val > max_i { max_i = val; }
                    s_idx += 1;
                }
                
                let end_s = s_idx;
                let area = (end_s - start_s) as f64;
                let avg_i = current_intensity_sum / area;

                // Filter by size settings
                if area >= settings.min_size as f64 && area <= settings.max_size as f64 {
                    let (class, size_cls) = classify_blob(area);
                    
                    // Calculate spatial metrics (mocking geometry based on ping/sample)
                    // In real sonar, range = speed * time / 2
                    let range_m = parse.ping_metadata[p_idx].range_m; 
                    let depth_m = parse.ping_metadata[p_idx].depth_m;

                    detections.push(Detection {
                        ping_index: p_idx,
                        sample_start: start_s,
                        sample_end: end_s,
                        intensity: max_i,
                        estimated_depth_m: Some(depth_m),
                        longitude: parse.ping_metadata[p_idx].longitude,
                        latitude: parse.ping_metadata[p_idx].latitude,
                        classification: class,
                        size_class: size_cls,
                        blob_area: area,
                        width_m: 0.5, // Placeholder for actual beam width
                        length_m: area * 0.1, // Placeholder for sample spacing
                        avg_intensity: avg_i,
                        confidence: (avg_i / (mean_intensity + 1.0)).min(1.0),
                        depth_m,
                        range_m,
                        channel: parse.ping_metadata[p_idx].channel,
                        channel_type: "SideScan".to_string(),
                    });
                }
            } else {
                s_idx += 1;
            }
        }
    }

    // 3. Aggregate Summary
    let mut summary = DetectionSummary {
        total: parse.pings.len(),
        total_detections: detections.len(),
        fish_count: 0,
        structure_count: 0,
        debris_count: 0,
        wreck_count: 0,
        detections,
    };

    for d in &summary.detections {
        match d.classification.as_str() {
            "Fish" => summary.fish_count += 1,
            "Structure" => summary.structure_count += 1,
            "Debris" => summary.debris_count += 1,
            "Wreck" => summary.wreck_count += 1,
            _ => {}
        }
    }

    summary
}
