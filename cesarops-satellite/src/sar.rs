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
use ndarray::Array2;
use serde::Serialize;
use std::collections::HashMap;
// ── Orbit grouping ────────────────────────────────────────────────────────────

/// A single SAR detection point with an orbit direction tag.
#[derive(Debug, Clone, Serialize)]
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

// ── Local RTC GeoTIFF extraction (FLEET TOOL 1) ───────────────────────────────

/// Default bright/dark threshold in local σ units.
pub const DEFAULT_SAR_SIGMA: f64 = 3.0;
const SAR_STAT_WINDOW: usize = 50;
const SAR_MIN_CLUSTER_PX: usize = 8;
const SAR_MAX_DIM: usize = 1536;

/// Infer orbit tag from Sentinel-1 filename tokens.
pub fn orbit_from_filename(path: &std::path::Path) -> String {
    let s = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_uppercase();
    if s.contains("1SDV") || s.contains("DESC") {
        "descending".into()
    } else if s.contains("1SSH") || s.contains("ASC") {
        "ascending".into()
    } else {
        "descending".into()
    }
}

fn downsample_factor_for_dim((h, w): (usize, usize), max_dim: usize) -> usize {
    let m = h.max(w);
    if m <= max_dim {
        1
    } else {
        (m + max_dim - 1) / max_dim
    }
}

/// Box-mean downsample (NaN-aware), matching POC `downsample`.
fn downsample_mean(arr: &Array2<f32>, factor: usize) -> Array2<f32> {
    if factor <= 1 {
        return arr.clone();
    }
    let (h, w) = arr.dim();
    let nh = h / factor;
    let nw = w / factor;
    if nh == 0 || nw == 0 {
        return Array2::zeros((0, 0));
    }
    let mut out = Array2::<f32>::from_elem((nh, nw), f32::NAN);
    for r in 0..nh {
        for c in 0..nw {
            let mut sum = 0.0f64;
            let mut n = 0usize;
            for dr in 0..factor {
                for dc in 0..factor {
                    let v = arr[[r * factor + dr, c * factor + dc]];
                    if v.is_finite() {
                        sum += v as f64;
                        n += 1;
                    }
                }
            }
            if n > 0 {
                out[[r, c]] = (sum / n as f64) as f32;
            }
        }
    }
    out
}

/// Separable box mean (finite pixels only; NaN excluded from mean).
fn box_mean(arr: &Array2<f32>, win: usize) -> Array2<f32> {
    let (h, w) = arr.dim();
    if win <= 1 || h == 0 || w == 0 {
        return arr.clone();
    }
    let half = win / 2;
    let mut out = Array2::<f32>::from_elem((h, w), f32::NAN);
    for r in 0..h {
        for c in 0..w {
            let mut sum = 0.0f64;
            let mut n = 0usize;
            for dr in 0..win {
                for dc in 0..win {
                    let rr = r.saturating_sub(half).saturating_add(dr);
                    let cc = c.saturating_sub(half).saturating_add(dc);
                    if rr >= h || cc >= w {
                        continue;
                    }
                    let v = arr[[rr, cc]];
                    if v.is_finite() {
                        sum += v as f64;
                        n += 1;
                    }
                }
            }
            if n >= win * win / 4 {
                out[[r, c]] = (sum / n as f64) as f32;
            }
        }
    }
    out
}

fn box_std(arr: &Array2<f32>, mean: &Array2<f32>, win: usize) -> Array2<f32> {
    let (h, w) = arr.dim();
    let half = win / 2;
    let mut out = Array2::<f32>::from_elem((h, w), f32::NAN);
    for r in 0..h {
        for c in 0..w {
            let m = mean[[r, c]];
            if !m.is_finite() {
                continue;
            }
            let mut var = 0.0f64;
            let mut n = 0usize;
            for dr in 0..win {
                for dc in 0..win {
                    let rr = r.saturating_sub(half).saturating_add(dr);
                    let cc = c.saturating_sub(half).saturating_add(dc);
                    if rr >= h || cc >= w {
                        continue;
                    }
                    let v = arr[[rr, cc]];
                    if v.is_finite() {
                        let d = v as f64 - m as f64;
                        var += d * d;
                        n += 1;
                    }
                }
            }
            if n >= win * win / 4 {
                out[[r, c]] = (var / n as f64).sqrt() as f32;
            }
        }
    }
    out
}

/// Label 4-connected components; returns (centroid_row, centroid_col, pixel_count).
fn connected_centroids(mask: &Array2<bool>, min_pixels: usize) -> Vec<(f64, f64, usize)> {
    let (h, w) = mask.dim();
    let mut seen = vec![false; h * w];
    let mut out = Vec::new();
    for sr in 0..h {
        for sc in 0..w {
            let idx0 = sr * w + sc;
            if seen[idx0] || !mask[[sr, sc]] {
                continue;
            }
            let mut stack = vec![(sr, sc)];
            seen[idx0] = true;
            let mut rs = 0.0f64;
            let mut cs = 0.0f64;
            let mut n = 0usize;
            while let Some((r, c)) = stack.pop() {
                rs += r as f64;
                cs += c as f64;
                n += 1;
                for (nr, nc) in [(r.wrapping_sub(1), c), (r + 1, c), (r, c.wrapping_sub(1)), (r, c + 1)] {
                    if nr >= h || nc >= w {
                        continue;
                    }
                    let idx = nr * w + nc;
                    if !seen[idx] && mask[[nr, nc]] {
                        seen[idx] = true;
                        stack.push((nr, nc));
                    }
                }
            }
            if n >= min_pixels {
                out.push((rs / n as f64, cs / n as f64, n));
            }
        }
    }
    out
}

#[cfg(feature = "gdal")]
struct SarWindowGeo {
    gt: [f64; 6],
    win_x: usize,
    win_y: usize,
    ds_srs_wkt: String,
}

#[cfg(feature = "gdal")]
fn pixel_to_wgs84(geo: &SarWindowGeo, row: f64, col: f64) -> anyhow::Result<(f64, f64)> {
    use gdal::spatial_ref::{AxisMappingStrategy, CoordTransform, SpatialRef};
    let x = geo.gt[0] + col * geo.gt[1] + row * geo.gt[2];
    let y = geo.gt[3] + col * geo.gt[4] + row * geo.gt[5];
    let src = SpatialRef::from_wkt(&geo.ds_srs_wkt)?;
    let mut dst = SpatialRef::from_epsg(4326)?;
    dst.set_axis_mapping_strategy(AxisMappingStrategy::TraditionalGisOrder);
    let ct = CoordTransform::new(&src, &dst)?;
    let mut xs = [x];
    let mut ys = [y];
    let mut zs: [f64; 0] = [];
    ct.transform_coords(&mut xs, &mut ys, &mut zs)?;
    Ok((ys[0], xs[0])) // lat, lon
}

#[cfg(feature = "gdal")]
fn read_sar_window(
    path: &std::path::Path,
    bbox: &crate::types::BBox,
) -> anyhow::Result<(Array2<f32>, SarWindowGeo)> {
    use gdal::Dataset;
    let ds = Dataset::open(path)?;
    let (full_w, full_h) = ds.raster_size();
    let gt = ds.geo_transform()?;
    let win = crate::chip::bbox_to_pixel_window(&ds, &gt, full_w, full_h, bbox)
        .ok_or_else(|| anyhow::anyhow!("bbox outside SAR tile {}", path.display()))?;
    let (wx, wy, ww, wh) = win;
    let band = ds.rasterband(1)?;
    let buf = band.read_as::<f32>((wx as isize, wy as isize), (ww, wh), (ww, wh), None)?;
    let data = buf.data().to_vec();
    let arr = Array2::from_shape_vec((wh, ww), data)?;
    let srs = ds.spatial_ref()?.to_wkt()?;
    Ok((
        arr,
        SarWindowGeo {
            gt,
            win_x: wx,
            win_y: wy,
            ds_srs_wkt: srs,
        },
    ))
}

#[cfg(feature = "gdal")]
pub fn extract_sar_anomalies_local(
    sar_tif_path: &std::path::Path,
    bbox: &crate::types::BBox,
    threshold_sigma: f64,
) -> anyhow::Result<Vec<SarPoint>> {
    let sigma = if threshold_sigma > 0.0 {
        threshold_sigma
    } else {
        DEFAULT_SAR_SIGMA
    };
    let orbit = orbit_from_filename(sar_tif_path);
    let (mut arr, geo) = read_sar_window(sar_tif_path, bbox)?;
    let factor = downsample_factor_for_dim(arr.dim(), SAR_MAX_DIM);
    if factor > 1 {
        arr = downsample_mean(&arr, factor);
    }
    for v in arr.iter_mut() {
        if !v.is_finite() || *v <= 0.0 {
            *v = f32::NAN;
        }
    }
    let mean = box_mean(&arr, SAR_STAT_WINDOW);
    let std = box_std(&arr, &mean, SAR_STAT_WINDOW);
    let (h, w) = arr.dim();
    let mut bright = Array2::<bool>::from_elem((h, w), false);
    let mut dark = Array2::<bool>::from_elem((h, w), false);
    for r in 0..h {
        for c in 0..w {
            let v = arr[[r, c]];
            let m = mean[[r, c]];
            let s = std[[r, c]];
            if !v.is_finite() || !m.is_finite() || !s.is_finite() || s < 1e-6 {
                continue;
            }
            let z = (v - m) / s;
            if z as f64 >= sigma {
                bright[[r, c]] = true;
            } else if z as f64 <= -sigma {
                dark[[r, c]] = true;
            }
        }
    }
    let scale = factor as f64;
    let mut points = Vec::new();
    for (sign, mask) in [("bright", &bright), ("dark", &dark)] {
        for (cr, cc, _n) in connected_centroids(mask, SAR_MIN_CLUSTER_PX) {
            let row_full = geo.win_y as f64 + (cr + 0.5) * scale - 0.5;
            let col_full = geo.win_x as f64 + (cc + 0.5) * scale - 0.5;
            let (lat, lon) = pixel_to_wgs84(&geo, row_full, col_full)?;
            let mut orbit_tag = orbit.clone();
            orbit_tag.push('_');
            orbit_tag.push_str(sign);
            points.push(SarPoint { lat, lon, orbit: orbit_tag });
        }
    }
    Ok(points)
}

/// Full local SAR pass: extract → DBSCAN clusters → JSON report.
#[cfg(feature = "gdal")]
pub fn run_sar_local(
    sar_dir: &std::path::Path,
    bbox: &crate::types::BBox,
    knobs: &crate::types::Knobs,
    known_wrecks: &[(f64, f64)],
    output_dir: &std::path::Path,
    threshold_sigma: f64,
) -> anyhow::Result<serde_json::Value> {
    use crate::chip::haversine_m;
    let tif = std::fs::read_dir(sar_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("tif"))
                .unwrap_or(false)
        })
        .ok_or_else(|| anyhow::anyhow!("no .tif in {}", sar_dir.display()))?;
    let points = match extract_sar_anomalies_local(&tif, bbox, threshold_sigma) {
        Ok(p) => p,
        Err(e) => {
            let hint = if tif
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.contains("SLC") || n.contains("_SLC__"))
                .unwrap_or(false)
            {
                " (file looks like raw SLC — need RTC GeoTIFF, not SLC)"
            } else {
                ""
            };
            anyhow::bail!("SAR open/extract failed for {}: {e}{hint}", tif.display());
        }
    };
    let clusters = run_sar_persistence(&points, knobs);
    let mut near_gt: Vec<serde_json::Value> = Vec::new();
    for (i, (lat, lon)) in known_wrecks.iter().enumerate() {
        let best = clusters
            .iter()
            .map(|c| (c, haversine_m(*lat, *lon, c.lat, c.lon)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        if let Some((c, d)) = best {
            near_gt.push(serde_json::json!({
                "gt_index": i,
                "nearest_cluster_m": d.round(),
                "cluster_lat": c.lat,
                "cluster_lon": c.lon,
                "persistence": c.persistence,
                "within_500m": d <= 500.0,
            }));
        }
    }
    std::fs::create_dir_all(output_dir)?;
    std::fs::write(
        output_dir.join("sar_points.json"),
        serde_json::to_string_pretty(&points)?,
    )?;
    std::fs::write(
        output_dir.join("sar_clusters.json"),
        serde_json::to_string_pretty(&clusters)?,
    )?;
    Ok(serde_json::json!({
        "rc": 0,
        "mode": "rust_native_sar_local",
        "sar_tif": tif.display().to_string(),
        "n_points": points.len(),
        "n_clusters": clusters.len(),
        "near_gt": near_gt,
        "output_dir": output_dir.display().to_string(),
    }))
}

#[cfg(not(feature = "gdal"))]
pub fn extract_sar_anomalies_local(
    _sar_tif_path: &std::path::Path,
    _bbox: &crate::types::BBox,
    _threshold_sigma: f64,
) -> anyhow::Result<Vec<SarPoint>> {
    anyhow::bail!("extract_sar_anomalies_local requires --features gdal")
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
    fn orbit_from_filename_descending() {
        let p = std::path::Path::new("rtc_S1A_IW_SLC__1SDV_20240811.tif");
        assert_eq!(orbit_from_filename(p), "descending");
    }

    #[test]
    fn connected_centroids_finds_blob() {
        let mut m = Array2::<bool>::from_elem((10, 10), false);
        for r in 3..7 {
            for c in 3..7 {
                m[[r, c]] = true;
            }
        }
        let c = connected_centroids(&m, 5);
        assert_eq!(c.len(), 1);
        assert!(c[0].2 >= 16);
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
