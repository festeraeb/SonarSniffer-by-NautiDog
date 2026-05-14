//! Temporal cluster matcher — finds detections at the same location across
//! multiple satellite passes. Persistent features are high-probability targets.
//!
//! Strategy: grid-based spatial clustering with temporal persistence scoring.
//! Detections within ~50m of each other are grouped; clusters with detections
//! from 2+ different runs/tiles are flagged as "investigate further."

use crate::detection_store::DetectionStore;
use serde::Serialize;
use std::collections::HashMap;
use tracing::info;

#[derive(Debug, Clone, Serialize)]
pub struct Cluster {
    pub cluster_id: String,
    pub center_lat: f64,
    pub center_lon: f64,
    pub detection_count: u32,
    pub pass_count: u32, // unique tile_id or run_id count
    pub max_confidence: f32,
    pub first_seen: i64,
    pub last_seen: i64,
}

/// Grid cell key for spatial clustering (~50m resolution at mid-latitudes).
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct GridKey(i32, i32);

/// Find temporally persistent clusters from the detection store.
pub async fn find_clusters(store: &DetectionStore) -> Vec<Cluster> {
    // Fetch all detections (cap at 10k for performance)
    let all_detections = store.query_detections(None, None, None, None, None, 10_000).await;

    if all_detections.is_empty() {
        return Vec::new();
    }

    // Grid-based clustering: ~0.0005° ≈ 50m at mid-latitudes
    let grid_resolution = 0.0005;

    let mut cells: HashMap<GridKey, Vec<&crate::detection_store::DetectionRow>> = HashMap::new();

    for det in &all_detections {
        let gx = (det.lon / grid_resolution).floor() as i32;
        let gy = (det.lat / grid_resolution).floor() as i32;
        cells.entry(GridKey(gx, gy)).or_default().push(det);
    }

    let mut clusters = Vec::new();

    for (key, dets) in &cells {
        if dets.len() < 2 {
            continue; // need at least 2 detections for a cluster
        }

        // Count unique passes (tile_id or run_id)
        let mut unique_passes: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut max_conf = 0.0f32;
        let mut first_seen = i64::MAX;
        let mut last_seen = i64::MIN;
        let mut sum_lat = 0.0f64;
        let mut sum_lon = 0.0f64;

        for det in dets {
            if let Some(ref tid) = det.tile_id {
                unique_passes.insert(tid.clone());
            }
            if let Some(ref rid) = det.run_id {
                unique_passes.insert(rid.clone());
            }
            if det.confidence > max_conf {
                max_conf = det.confidence;
            }
            if det.timestamp < first_seen {
                first_seen = det.timestamp;
            }
            if det.timestamp > last_seen {
                last_seen = det.timestamp;
            }
            sum_lat += det.lat;
            sum_lon += det.lon;
        }

        let count = dets.len() as u32;
        let pass_count = unique_passes.len() as u32;
        let center_lat = sum_lat / count as f64;
        let center_lon = sum_lon / count as f64;

        // Only report clusters with detections from 2+ different passes
        if pass_count < 2 {
            continue;
        }

        let cluster_id = format!("cluster_{}_{}", key.0, key.1);

        clusters.push(Cluster {
            cluster_id,
            center_lat,
            center_lon,
            detection_count: count,
            pass_count,
            max_confidence: max_conf,
            first_seen,
            last_seen,
        });
    }

    // Sort by pass count (most persistent first), then by confidence
    clusters.sort_by(|a, b| {
        b.pass_count
            .cmp(&a.pass_count)
            .then_with(|| b.max_confidence.partial_cmp(&a.max_confidence).unwrap_or(std::cmp::Ordering::Equal))
    });

    info!("Cluster matching: {} clusters found from {} detections", clusters.len(), all_detections.len());
    clusters
}
