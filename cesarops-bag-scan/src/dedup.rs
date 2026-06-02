//! Spatial deduplication of detections.
//!
//! Ported from `bag_wreck_detector.py::SpatialDeduplicator`:
//!   * sort by confidence (highest first)
//!   * greedily keep a detection, absorbing every not-yet-used detection
//!     within `merge_radius_m`
//!   * distance via UTM easting/northing when available, else a haversine-style
//!     lat/lon approximation.
//!
//! Operates on the final contract-bearing [`WreckDetection`]s so both physical
//! wrecks and masked regions are de-duplicated together (the same wreck can
//! surface as both a physical anomaly and a masked region).

use crate::types::WreckDetection;

/// Merge nearby detections, keeping the highest-confidence one from each group.
/// `merge_radius_m` from `SpatialDeduplicator.merge_radius_m` (default 200).
pub fn deduplicate(detections: Vec<WreckDetection>, merge_radius_m: f64) -> Vec<WreckDetection> {
    if detections.is_empty() {
        return detections;
    }

    // Sort by confidence descending (stable so equal-confidence order is kept).
    let mut sorted = detections;
    sorted.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));

    let n = sorted.len();
    let mut used = vec![false; n];
    let mut kept = Vec::new();

    for i in 0..n {
        if used[i] {
            continue;
        }
        let mut group_count = 1usize;
        for j in (i + 1)..n {
            if used[j] {
                continue;
            }
            let dist = distance_m(&sorted[i], &sorted[j]);
            if dist <= merge_radius_m {
                used[j] = true;
                group_count += 1;
            }
        }
        used[i] = true;

        // Keep the best (sorted[i]); annotate how many it absorbed.
        let mut best = sorted[i].clone();
        if let serde_json::Value::Object(ref mut map) = best.metadata {
            map.insert("merged_from".into(), serde_json::json!(group_count));
        } else {
            best.metadata = serde_json::json!({ "merged_from": group_count });
        }
        kept.push(best);
    }

    kept
}

/// Approximate distance between two detections in meters.
/// Ported from `SpatialDeduplicator._distance_m`.
fn distance_m(a: &WreckDetection, b: &WreckDetection) -> f64 {
    if a.easting > 0.0 && b.easting > 0.0 {
        let de = a.easting - b.easting;
        let dn = a.northing - b.northing;
        (de * de + dn * dn).sqrt()
    } else {
        let dlat = (a.latitude - b.latitude) * 111_000.0;
        let dlon = (a.longitude - b.longitude) * 111_000.0 * a.latitude.to_radians().cos();
        (dlat * dlat + dlon * dlon).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ObjectType, WreckDetection};

    fn det(id: &str, easting: f64, northing: f64, conf: f64) -> WreckDetection {
        WreckDetection {
            id: id.into(),
            signature_type: "physical_wreck".into(),
            latitude: 45.0,
            longitude: -84.0,
            easting,
            northing,
            size_sq_feet: 1000.0,
            size_meters: 30.0,
            depth_meters: 30.0,
            height_above_floor_m: 4.0,
            long_side_ft: 100.0,
            short_side_ft: 30.0,
            confidence: conf,
            object_type: ObjectType::Wreck,
            heading_deg: 0.0,
            heading_alt_deg: 180.0,
            cell_count: 50,
            bag_file: "f.bag".into(),
            survey_id: "f".into(),
            metadata: serde_json::Value::Null,
        }
    }

    #[test]
    fn merges_within_radius_keeps_highest_confidence() {
        let dets = vec![
            det("a", 500_000.0, 5_000_000.0, 0.6),
            det("b", 500_050.0, 5_000_000.0, 0.9), // 50m away, higher conf
            det("c", 600_000.0, 5_000_000.0, 0.7), // far away
        ];
        let out = deduplicate(dets, 200.0);
        assert_eq!(out.len(), 2);
        // The highest-confidence detection in the merged group is "b".
        let merged = out.iter().find(|d| d.easting < 550_000.0).unwrap();
        assert_eq!(merged.id, "b");
        assert_eq!(merged.confidence, 0.9);
    }

    #[test]
    fn keeps_distinct_detections() {
        let dets = vec![
            det("a", 500_000.0, 5_000_000.0, 0.6),
            det("b", 500_500.0, 5_000_000.0, 0.6), // 500m away > 200m radius
        ];
        let out = deduplicate(dets, 200.0);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn empty_input() {
        assert!(deduplicate(Vec::new(), 200.0).is_empty());
    }
}
