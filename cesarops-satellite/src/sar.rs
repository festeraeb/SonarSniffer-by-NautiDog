//! SAR DBSCAN temporal persistence.
//!
//! Ports `sar_temporal_persistence.py`:
//!   `group_by_orbit`        → [`group_by_orbit`]      (ascending / descending)
//!   `run_dbscan(eps, min)`  → [`run_dbscan`]          (simple 2-D DBSCAN)
//!   `calculate_persistence` → [`calculate_persistence`] (per-cluster counts)
//!
//! The Python reference clusters flattened pixel intensities (`data.reshape(-1, 1)`)
//! and counts label frequencies as a crude persistence proxy.  Here we keep the
//! DBSCAN + persistence semantics but operate on 2-D detection points
//! `(lat, lon)` so the result is a set of [`SarCluster`]s usable by the fusion
//! stage.  `run_dbscan` is a faithful, self-contained DBSCAN (the Python code
//! used `sklearn.cluster.DBSCAN`).

use crate::types::SarCluster;
use std::collections::HashMap;
// ── Orbit grouping ────────────────────────────────────────────────────────────

/// A single SAR detection point with an orbit direction tag.
#[derive(Debug, Clone)]
pub struct SarPoint {
    pub lat: f64,
    pub lon: f64,
    /// "ascending" | "descending" (any other value is bucketed under itself).
    pub orbit: String,
}

/// Group SAR points by orbit direction.
///
/// Mirrors Python `group_by_orbit`, which seeds {'ascending': [], 'descending': []}.
/// Points whose orbit is neither still get their own bucket so nothing is dropped.
pub fn group_by_orbit(points: &[SarPoint]) -> HashMap<String, Vec<SarPoint>> {
    let mut grouped: HashMap<String, Vec<SarPoint>> = HashMap::new();
    grouped.insert("ascending".into(), Vec::new());
    grouped.insert("descending".into(), Vec::new());
    for p in points {
        grouped.entry(p.orbit.clone()).or_default().push(p.clone());
    }
    grouped
}

// ── DBSCAN ────────────────────────────────────────────────────────────────────

/// DBSCAN noise label (matches sklearn's `-1`).
pub const DBSCAN_NOISE: i64 = -1;

/// Run DBSCAN over 2-D points, returning a label per input point.
///
/// `eps` is the neighbourhood radius (same units as the point coordinates);
/// `min_samples` is the minimum number of points (including the point itself)
/// required for a core point.  Noise points get label [`DBSCAN_NOISE`].
///
/// This is a faithful reimplementation of the standard DBSCAN algorithm used by
/// `sklearn.cluster.DBSCAN` in `sar_temporal_persistence.py::run_dbscan`
/// (default `eps=0.5, min_samples=5`).
pub fn run_dbscan(points: &[(f64, f64)], eps: f64, min_samples: usize) -> Vec<i64> {
    let n = points.len();
    let mut labels = vec![DBSCAN_NOISE; n];
    let mut visited = vec![false; n];
    let eps2 = eps * eps;

    let region_query = |idx: usize| -> Vec<usize> {
        let (px, py) = points[idx];
        (0..n)
            .filter(|&j| {
                let (qx, qy) = points[j];
                let dx = px - qx;
                let dy = py - qy;
                dx * dx + dy * dy <= eps2
            })
            .collect()
    };

    let mut cluster_id: i64 = -1;
    for i in 0..n {
        if visited[i] {
            continue;
        }
        visited[i] = true;
        let mut neighbors = region_query(i);
        if neighbors.len() < min_samples {
            // Stays noise for now (may later be absorbed as a border point).
            continue;
        }
        cluster_id += 1;
        labels[i] = cluster_id;

        // Expand the cluster (BFS over the neighbour frontier).
        let mut k = 0;
        while k < neighbors.len() {
            let j = neighbors[k];
            if !visited[j] {
                visited[j] = true;
                let j_neighbors = region_query(j);
                if j_neighbors.len() >= min_samples {
                    // Core point — append its neighbours to the frontier.
                    for &nb in &j_neighbors {
                        if !neighbors.contains(&nb) {
                            neighbors.push(nb);
                        }
                    }
                }
            }
            // Border point (or core) — assign to cluster if currently noise.
            if labels[j] == DBSCAN_NOISE {
                labels[j] = cluster_id;
            }
            k += 1;
        }
    }
    labels
}

// ── Persistence ───────────────────────────────────────────────────────────────

/// Per-cluster member counts, excluding the noise label.
///
/// Mirrors Python `calculate_persistence`:
///   unique, counts = np.unique(labels, return_counts=True)
///   {label: count for label, count if label != -1}
pub fn calculate_persistence(labels: &[i64]) -> HashMap<i64, usize> {
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for &lbl in labels {
        if lbl != DBSCAN_NOISE {
            *counts.entry(lbl).or_insert(0) += 1;
        }
    }
    counts
}

// ── High-level cluster builder ────────────────────────────────────────────────

/// Cluster SAR detection points and emit [`SarCluster`]s.
///
/// Runs DBSCAN over `(lon, lat)` points, computes each cluster's centroid and a
/// normalised persistence score (`member_count / total_points`), and returns one
/// [`SarCluster`] per non-noise cluster.  `orbit` tags the source orbit group.
pub fn cluster_sar_points(
    points: &[SarPoint],
    eps: f64,
    min_samples: usize,
    orbit: &str,
) -> Vec<SarCluster> {
    if points.is_empty() {
        return vec![];
    }
    // DBSCAN over (lon, lat) — order is arbitrary but kept (x=lon, y=lat).
    let coords: Vec<(f64, f64)> = points.iter().map(|p| (p.lon, p.lat)).collect();
    let labels = run_dbscan(&coords, eps, min_samples);
    let persistence = calculate_persistence(&labels);
    let total = points.len() as f64;

    let mut clusters: Vec<SarCluster> = Vec::new();
    let mut ids: Vec<i64> = persistence.keys().copied().collect();
    ids.sort_unstable();
    for cid in ids {
        let members: Vec<usize> = labels
            .iter()
            .enumerate()
            .filter(|(_, &l)| l == cid)
            .map(|(i, _)| i)
            .collect();
        let n = members.len();
        if n == 0 {
            continue;
        }
        let lat = members.iter().map(|&i| points[i].lat).sum::<f64>() / n as f64;
        let lon = members.iter().map(|&i| points[i].lon).sum::<f64>() / n as f64;
        let frac = n as f64 / total;
        clusters.push(SarCluster {
            cluster_id: cid,
            lat,
            lon,
            persistence: frac,
            // Confidence proxy: same as persistence fraction unless overridden.
            confidence: frac,
            n_points: n,
            orbit: orbit.to_string(),
            props: HashMap::new(),
        });
    }
    clusters
}

// ── Knob-driven orchestration ─────────────────────────────────────────────────

/// Full SAR temporal-persistence pass over a set of detection points using the
/// pipeline [`Knobs`] (`dbscan_eps`, `dbscan_min_samples`) instead of hardcoded
/// values.
///
/// Groups the points by orbit (`group_by_orbit`), runs DBSCAN persistence
/// clustering per orbit group, and returns all resulting [`SarCluster`]s.
pub fn run_sar_persistence(points: &[SarPoint], knobs: &crate::types::Knobs) -> Vec<SarCluster> {
    let eps = if knobs.dbscan_eps > 0.0 { knobs.dbscan_eps } else { 0.5 };
    let min_samples = if knobs.dbscan_min_samples > 0 { knobs.dbscan_min_samples } else { 5 };

    let grouped = group_by_orbit(points);
    let mut out: Vec<SarCluster> = Vec::new();
    // Stable orbit order for deterministic output.
    let mut orbits: Vec<&String> = grouped.keys().collect();
    orbits.sort();
    for orbit in orbits {
        let pts = &grouped[orbit];
        if pts.is_empty() {
            continue;
        }
        out.extend(cluster_sar_points(pts, eps, min_samples, orbit));
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbscan_two_well_separated_clusters() {
        // Two tight blobs far apart; with min_samples=3 and small eps each blob
        // forms its own cluster and there is no noise.
        let mut pts: Vec<(f64, f64)> = Vec::new();
        // Blob A around (0,0)
        for i in 0..5 {
            pts.push((0.0 + i as f64 * 0.01, 0.0));
        }
        // Blob B around (10,10)
        for i in 0..5 {
            pts.push((10.0 + i as f64 * 0.01, 10.0));
        }
        let labels = run_dbscan(&pts, 0.5, 3);
        let persistence = calculate_persistence(&labels);
        assert_eq!(persistence.len(), 2, "should find exactly two clusters");
        // No noise.
        assert!(labels.iter().all(|&l| l != DBSCAN_NOISE));
        // Both clusters have 5 members.
        for (_, &count) in persistence.iter() {
            assert_eq!(count, 5);
        }
    }

    #[test]
    fn dbscan_marks_isolated_point_as_noise() {
        // One dense blob + one far-away lone point → the lone point is noise.
        let mut pts: Vec<(f64, f64)> = Vec::new();
        for i in 0..6 {
            pts.push((0.0 + i as f64 * 0.01, 0.0));
        }
        pts.push((50.0, 50.0)); // isolated
        let labels = run_dbscan(&pts, 0.5, 3);
        assert_eq!(*labels.last().unwrap(), DBSCAN_NOISE, "isolated point is noise");
        let persistence = calculate_persistence(&labels);
        assert_eq!(persistence.len(), 1, "only one real cluster");
    }

    #[test]
    fn dbscan_all_noise_when_min_samples_too_high() {
        // 4 points but min_samples=10 → everything is noise, no clusters.
        let pts = vec![(0.0, 0.0), (0.01, 0.0), (0.0, 0.01), (0.01, 0.01)];
        let labels = run_dbscan(&pts, 0.5, 10);
        assert!(labels.iter().all(|&l| l == DBSCAN_NOISE));
        assert!(calculate_persistence(&labels).is_empty());
    }

    #[test]
    fn group_by_orbit_buckets() {
        let pts = vec![
            SarPoint { lat: 1.0, lon: 1.0, orbit: "ascending".into() },
            SarPoint { lat: 2.0, lon: 2.0, orbit: "descending".into() },
            SarPoint { lat: 3.0, lon: 3.0, orbit: "ascending".into() },
        ];
        let g = group_by_orbit(&pts);
        assert_eq!(g["ascending"].len(), 2);
        assert_eq!(g["descending"].len(), 1);
    }

    #[test]
    fn cluster_sar_points_emits_clusters() {
        let mut pts: Vec<SarPoint> = Vec::new();
        for i in 0..5 {
            pts.push(SarPoint { lat: 45.0 + i as f64 * 0.001, lon: -81.0, orbit: "ascending".into() });
        }
        let clusters = cluster_sar_points(&pts, 0.5, 3, "ascending");
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].n_points, 5);
        assert!((clusters[0].persistence - 1.0).abs() < 1e-9, "all points in one cluster → persistence 1.0");
        assert!((clusters[0].lon + 81.0).abs() < 1e-9);
    }

    #[test]
    fn run_sar_persistence_uses_knobs() {
        // Two orbit groups, each a tight blob → one cluster per orbit.
        let mut pts: Vec<SarPoint> = Vec::new();
        for i in 0..5 {
            pts.push(SarPoint { lat: 45.0 + i as f64 * 0.0005, lon: -81.0, orbit: "ascending".into() });
        }
        for i in 0..5 {
            pts.push(SarPoint { lat: 44.0 + i as f64 * 0.0005, lon: -82.0, orbit: "descending".into() });
        }
        let mut knobs = crate::types::Knobs::default();
        knobs.dbscan_eps = 0.5;
        knobs.dbscan_min_samples = 3;
        let clusters = run_sar_persistence(&pts, &knobs);
        assert_eq!(clusters.len(), 2, "one cluster per orbit group");
        // Orbit tags preserved.
        assert!(clusters.iter().any(|c| c.orbit == "ascending"));
        assert!(clusters.iter().any(|c| c.orbit == "descending"));
    }
}
