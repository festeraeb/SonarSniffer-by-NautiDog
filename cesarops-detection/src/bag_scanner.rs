//! BAG (Bathymetric Attributed Grid) wreck scanner.
//!
//! Ports the Python detector in `pipelines/bag/bag_wreck_detector.py` to Rust.
//! Reads NOAA HDF5 BAG files, computes a Gaussian-blurred background, finds cells
//! that protrude above the seafloor, clusters them with BFS, and returns
//! `WreckDetection` records with approximate WGS84 coordinates.

use std::collections::VecDeque;
use std::f64::consts::PI;

use hdf5::File as Hdf5File;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Metadata extracted from a BAG file.
#[derive(Debug, Clone)]
pub struct BagInfo {
    pub filepath: String,
    pub survey_id: String,
    pub rows: usize,
    pub cols: usize,
    pub sw_easting: f64,
    pub sw_northing: f64,
    pub ne_easting: f64,
    pub ne_northing: f64,
    pub resolution_m: f64,
    pub nodata_value: f32,
    pub valid_cell_count: usize,
    pub depth_min: f64,
    pub depth_max: f64,
}

/// Broad classification of a detected object.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectType {
    Wreck,
    Debris,
    Obstruction,
    Unknown,
}

/// A single wreck/obstruction detection result.
#[derive(Debug, Clone)]
pub struct WreckDetection {
    pub id: String,
    pub latitude: f64,
    pub longitude: f64,
    pub easting: f64,
    pub northing: f64,
    pub depth_meters: f64,
    pub size_meters: f64,
    pub height_above_floor: f64,
    pub confidence: f64,
    pub object_type: ObjectType,
    pub bag_file: String,
    pub survey_id: String,
    pub cell_count: usize,
}

/// Errors that can occur during BAG scanning.
#[derive(Debug)]
pub enum BagScanError {
    Hdf5Error(String),
    NoElevationDataset,
    InsufficientData,
    ParseError(String),
}

impl std::fmt::Display for BagScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BagScanError::Hdf5Error(msg) => write!(f, "HDF5 error: {}", msg),
            BagScanError::NoElevationDataset => write!(f, "No BAG_root/elevation dataset found"),
            BagScanError::InsufficientData => write!(f, "Insufficient valid data cells"),
            BagScanError::ParseError(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

impl std::error::Error for BagScanError {}

// ─────────────────────────────────────────────────────────────────────────────
// Scanner
// ─────────────────────────────────────────────────────────────────────────────

/// BAG wreck detector.
pub struct BagScanner {
    /// Minimum height above the background to flag a cell as anomalous (meters).
    pub min_height_m: f64,
    /// Minimum cluster size (cells) to emit a detection.
    pub min_cluster_cells: usize,
}

impl Default for BagScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl BagScanner {
    pub fn new() -> Self {
        BagScanner {
            min_height_m: 0.5,
            min_cluster_cells: 3,
        }
    }

    /// Scan a BAG file and return a list of wreck detections.
    pub fn scan(&self, path: &str) -> Result<Vec<WreckDetection>, BagScanError> {
        // ── Step 1: Read HDF5 ────────────────────────────────────────────────
        let file = Hdf5File::open(path)
            .map_err(|e| BagScanError::Hdf5Error(e.to_string()))?;

        let elev_ds = file
            .dataset("BAG_root/elevation")
            .map_err(|_| BagScanError::NoElevationDataset)?;

        let shape = elev_ds.shape();
        if shape.len() < 2 {
            return Err(BagScanError::InsufficientData);
        }
        let orig_rows = shape[0];
        let orig_cols = shape[1];

        let raw_elev: Vec<f32> = elev_ds
            .read_raw::<f32>()
            .map_err(|e| BagScanError::Hdf5Error(e.to_string()))?;

        const NODATA: f32 = 1_000_000.0;

        // Read metadata XML (best-effort; fall back to defaults on any error)
        let xml = read_xml_metadata(&file);
        let (sw_e, sw_n, ne_e, ne_n, resolution_m) = parse_georef(&xml);

        let survey_id = derive_survey_id(path);

        // ── Downsample if necessary ───────────────────────────────────────────
        let total_cells = orig_rows * orig_cols;
        let step = if total_cells > 10_000_000 {
            let s = ((total_cells as f64 / 10_000_000.0).sqrt().ceil() as usize).max(2);
            s
        } else {
            1
        };

        let rows = (orig_rows + step - 1) / step;
        let cols = (orig_cols + step - 1) / step;
        let eff_resolution = resolution_m * step as f64;

        // Build f64 elevation grid (NaN for nodata)
        let mut elev: Vec<f64> = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            let orig_r = r * step;
            for c in 0..cols {
                let orig_c = c * step;
                let v = raw_elev[orig_r * orig_cols + orig_c];
                if (v - NODATA).abs() < 1.0 || v.is_nan() {
                    elev.push(f64::NAN);
                } else {
                    elev.push(v as f64);
                }
            }
        }

        // Gather stats
        let valid_vals: Vec<f64> = elev.iter().copied().filter(|v| !v.is_nan()).collect();
        if valid_vals.len() < self.min_cluster_cells {
            return Err(BagScanError::InsufficientData);
        }
        let valid_cell_count = valid_vals.len();
        let depth_min = valid_vals.iter().cloned().fold(f64::INFINITY, f64::min);
        let depth_max = valid_vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let global_mean: f64 = valid_vals.iter().sum::<f64>() / valid_vals.len() as f64;

        let _bag_info = BagInfo {
            filepath: path.to_string(),
            survey_id: survey_id.clone(),
            rows,
            cols,
            sw_easting: sw_e,
            sw_northing: sw_n,
            ne_easting: ne_e,
            ne_northing: ne_n,
            resolution_m: eff_resolution,
            nodata_value: NODATA,
            valid_cell_count,
            depth_min,
            depth_max,
        };

        // ── Step 2: Background (Gaussian-blurred seafloor) ───────────────────
        let sigma_cells = (100.0 / eff_resolution).clamp(2.0, 30.0);
        // Fill NaN with global mean before blur
        let filled: Vec<f64> = elev.iter().map(|&v| if v.is_nan() { global_mean } else { v }).collect();
        let background = gaussian_approx_2d(&filled, rows, cols, sigma_cells);

        // ── Step 3: Anomaly mask ─────────────────────────────────────────────
        // Wrecks are shallower (less negative) than the blurred seafloor.
        let height_above: Vec<f64> = elev
            .iter()
            .zip(background.iter())
            .map(|(&e, &b)| if e.is_nan() { f64::NAN } else { e - b })
            .collect();

        let anomaly: Vec<bool> = height_above
            .iter()
            .map(|&h| !h.is_nan() && h >= self.min_height_m)
            .collect();

        // ── Step 4: BFS clustering (8-connectivity) ──────────────────────────
        let clusters = bfs_cluster(&anomaly, rows, cols);

        // ── Step 5 & 6: Build detections ─────────────────────────────────────
        let mut detections = Vec::new();
        let mut det_idx = 0usize;

        for cluster_cells in &clusters {
            if cluster_cells.len() < self.min_cluster_cells {
                continue;
            }

            let cell_count = cluster_cells.len();

            // Centroid
            let sum_r: f64 = cluster_cells.iter().map(|&(r, _)| r as f64).sum();
            let sum_c: f64 = cluster_cells.iter().map(|&(_, c)| c as f64).sum();
            let cen_r = sum_r / cell_count as f64;
            let cen_c = sum_c / cell_count as f64;
            let cen_ri = cen_r.round() as usize;
            let cen_ci = cen_c.round() as usize;

            // Max height above
            let max_height: f64 = cluster_cells
                .iter()
                .map(|&(r, c)| height_above[r * cols + c])
                .filter(|v| !v.is_nan())
                .fold(f64::NEG_INFINITY, f64::max);

            let size_meters = (cell_count as f64).sqrt() * eff_resolution;

            // Depth at centroid (clamp to valid index)
            let cen_depth = if cen_ri < rows && cen_ci < cols {
                elev[cen_ri * cols + cen_ci]
            } else {
                f64::NAN
            };

            // Pixel → UTM
            let easting = sw_e + cen_c * eff_resolution;
            // Row 0 = northernmost in BAG
            let northing = ne_n - cen_r * eff_resolution;

            // UTM → WGS84 (rough approximation, zone 16N / EPSG:26916 default)
            // TODO: replace with a proper `proj` crate transform for production use.
            let (latitude, longitude) = utm_to_wgs84_approx(easting, northing);

            let confidence = (max_height / 5.0).clamp(0.0, 1.0);

            let id = format!("{}-{:04}", survey_id, det_idx);
            det_idx += 1;

            detections.push(WreckDetection {
                id,
                latitude,
                longitude,
                easting,
                northing,
                depth_meters: cen_depth,
                size_meters,
                height_above_floor: max_height,
                confidence,
                object_type: ObjectType::Wreck,
                bag_file: path.to_string(),
                survey_id: survey_id.clone(),
                cell_count,
            });
        }

        Ok(detections)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// HDF5 metadata helpers
// ─────────────────────────────────────────────────────────────────────────────

fn read_xml_metadata(file: &Hdf5File) -> String {
    let meta_ds = match file.dataset("BAG_root/metadata") {
        Ok(ds) => ds,
        Err(_) => return String::new(),
    };
    // Try reading as raw bytes
    if let Ok(raw) = meta_ds.read_raw::<u8>() {
        return String::from_utf8_lossy(&raw).into_owned();
    }
    String::new()
}

/// Parse corner coordinates and resolution from BAG XML metadata.
/// Returns `(sw_easting, sw_northing, ne_easting, ne_northing, resolution_m)`.
/// Falls back to `(0.0, 0.0, 0.0, 0.0, 1.0)` on any parse failure.
fn parse_georef(xml: &str) -> (f64, f64, f64, f64, f64) {
    let corners = extract_corners(xml).unwrap_or((0.0, 0.0, 0.0, 0.0));
    let resolution = extract_resolution(xml).unwrap_or(1.0);
    (corners.0, corners.1, corners.2, corners.3, resolution)
}

/// Extract SW/NE corner coordinates from `<gml:coordinates>SW_E,SW_N NE_E,NE_N</gml:coordinates>`.
pub fn extract_corners(xml: &str) -> Option<(f64, f64, f64, f64)> {
    let tag = "<gml:coordinates";
    let start = xml.find(tag)?;
    let gt = xml[start..].find('>')?;
    let content_start = start + gt + 1;
    let end_tag = "</gml:coordinates>";
    let content_end = xml[content_start..].find(end_tag)?;
    let content = xml[content_start..content_start + content_end].trim();

    let parts: Vec<&str> = content.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let sw: Vec<&str> = parts[0].split(',').collect();
    let ne: Vec<&str> = parts[1].split(',').collect();
    if sw.len() < 2 || ne.len() < 2 {
        return None;
    }
    let sw_e = sw[0].trim().parse::<f64>().ok()?;
    let sw_n = sw[1].trim().parse::<f64>().ok()?;
    let ne_e = ne[0].trim().parse::<f64>().ok()?;
    let ne_n = ne[1].trim().parse::<f64>().ok()?;
    Some((sw_e, sw_n, ne_e, ne_n))
}

/// Extract cell resolution from `<gco:Measure uom="m">X.X</gco:Measure>`.
pub fn extract_resolution(xml: &str) -> Option<f64> {
    let tag = r#"<gco:Measure uom="m">"#;
    let start = xml.find(tag)?;
    let value_start = start + tag.len();
    let end = xml[value_start..].find("</gco:Measure>")?;
    xml[value_start..value_start + end].trim().parse::<f64>().ok()
}

fn derive_survey_id(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("UNKNOWN")
        .to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// Approximate UTM→WGS84 (zone 16N default, for Great Lakes region)
// TODO: replace with a proper `proj` crate call for production use.
// ─────────────────────────────────────────────────────────────────────────────

fn utm_to_wgs84_approx(easting: f64, northing: f64) -> (f64, f64) {
    let lat_deg = northing / 111_320.0;
    let lat_rad = lat_deg * PI / 180.0;
    let central_meridian = -87.0_f64; // zone 16N
    let lon_deg = central_meridian + (easting - 500_000.0) / (111_320.0 * lat_rad.cos());
    (lat_deg, lon_deg)
}

// ─────────────────────────────────────────────────────────────────────────────
// Box filter / Gaussian approximation
// ─────────────────────────────────────────────────────────────────────────────

/// Single-pass 1D box filter with border clamping.
fn box_filter_1d(data: &[f64], _width: usize, radius: usize) -> Vec<f64> {
    let n = data.len();
    if n == 0 || radius == 0 {
        return data.to_vec();
    }
    let mut out = vec![0.0f64; n];
    for i in 0..n {
        let lo = i.saturating_sub(radius);
        let hi = (i + radius).min(n - 1);
        let count = hi - lo + 1;
        let sum: f64 = data[lo..=hi].iter().sum();
        out[i] = sum / count as f64;
    }
    out
}

/// 2D Gaussian approximation via 3 passes of a separable box filter.
/// `sigma` is in grid cells.
pub fn gaussian_approx_2d(grid: &[f64], rows: usize, cols: usize, sigma: f64) -> Vec<f64> {
    let radius = ((sigma * 0.866) as usize).max(1);
    let win = 2 * radius + 1;

    let mut current = grid.to_vec();

    for _ in 0..3 {
        // Row pass
        let mut after_rows = vec![0.0f64; rows * cols];
        for r in 0..rows {
            let row_slice = &current[r * cols..(r + 1) * cols];
            let filtered = box_filter_1d(row_slice, win, radius);
            after_rows[r * cols..(r + 1) * cols].copy_from_slice(&filtered);
        }

        // Column pass
        let mut after_cols = vec![0.0f64; rows * cols];
        for c in 0..cols {
            let col_vec: Vec<f64> = (0..rows).map(|r| after_rows[r * cols + c]).collect();
            let filtered = box_filter_1d(&col_vec, win, radius);
            for r in 0..rows {
                after_cols[r * cols + c] = filtered[r];
            }
        }
        current = after_cols;
    }
    current
}

// ─────────────────────────────────────────────────────────────────────────────
// BFS clustering (8-connectivity)
// ─────────────────────────────────────────────────────────────────────────────

fn bfs_cluster(mask: &[bool], rows: usize, cols: usize) -> Vec<Vec<(usize, usize)>> {
    let mut visited = vec![false; rows * cols];
    let mut clusters: Vec<Vec<(usize, usize)>> = Vec::new();

    const DIRS: [(i32, i32); 8] = [
        (-1, -1), (-1, 0), (-1, 1),
        ( 0, -1),           ( 0, 1),
        ( 1, -1), ( 1, 0), ( 1, 1),
    ];

    for start_r in 0..rows {
        for start_c in 0..cols {
            let idx = start_r * cols + start_c;
            if !mask[idx] || visited[idx] {
                continue;
            }
            // BFS from (start_r, start_c)
            let mut cluster = Vec::new();
            let mut queue = VecDeque::new();
            queue.push_back((start_r, start_c));
            visited[idx] = true;

            while let Some((r, c)) = queue.pop_front() {
                cluster.push((r, c));
                for &(dr, dc) in &DIRS {
                    let nr = r as i32 + dr;
                    let nc = c as i32 + dc;
                    if nr < 0 || nc < 0 || nr as usize >= rows || nc as usize >= cols {
                        continue;
                    }
                    let ni = nr as usize * cols + nc as usize;
                    if mask[ni] && !visited[ni] {
                        visited[ni] = true;
                        queue.push_back((nr as usize, nc as usize));
                    }
                }
            }
            clusters.push(cluster);
        }
    }
    clusters
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a flat grid and run the full detection pipeline internally.
    fn run_detection_on_grid(
        elev: &[f64],
        rows: usize,
        cols: usize,
        resolution_m: f64,
        scanner: &BagScanner,
    ) -> Vec<WreckDetection> {
        const FAKE_SW_E: f64 = 500_000.0;
        const FAKE_NE_N: f64 = 4_800_000.0;

        let valid_vals: Vec<f64> = elev.iter().copied().filter(|v| !v.is_nan()).collect();
        if valid_vals.len() < scanner.min_cluster_cells {
            return vec![];
        }
        let global_mean: f64 = valid_vals.iter().sum::<f64>() / valid_vals.len() as f64;

        let sigma_cells = (100.0 / resolution_m).clamp(2.0, 30.0);
        let filled: Vec<f64> = elev.iter().map(|&v| if v.is_nan() { global_mean } else { v }).collect();
        let background = gaussian_approx_2d(&filled, rows, cols, sigma_cells);

        let height_above: Vec<f64> = elev
            .iter()
            .zip(background.iter())
            .map(|(&e, &b)| if e.is_nan() { f64::NAN } else { e - b })
            .collect();

        let anomaly: Vec<bool> = height_above
            .iter()
            .map(|&h| !h.is_nan() && h >= scanner.min_height_m)
            .collect();

        let clusters = bfs_cluster(&anomaly, rows, cols);

        let survey_id = "TEST".to_string();
        let mut detections = Vec::new();
        let mut det_idx = 0usize;

        for cluster_cells in &clusters {
            if cluster_cells.len() < scanner.min_cluster_cells {
                continue;
            }
            let cell_count = cluster_cells.len();
            let sum_r: f64 = cluster_cells.iter().map(|&(r, _)| r as f64).sum();
            let sum_c: f64 = cluster_cells.iter().map(|&(_, c)| c as f64).sum();
            let cen_r = sum_r / cell_count as f64;
            let cen_c = sum_c / cell_count as f64;
            let cen_ri = cen_r.round() as usize;
            let cen_ci = cen_c.round() as usize;

            let max_height: f64 = cluster_cells
                .iter()
                .map(|&(r, c)| height_above[r * cols + c])
                .filter(|v| !v.is_nan())
                .fold(f64::NEG_INFINITY, f64::max);

            let size_meters = (cell_count as f64).sqrt() * resolution_m;
            let cen_depth = if cen_ri < rows && cen_ci < cols {
                elev[cen_ri * cols + cen_ci]
            } else {
                f64::NAN
            };

            let easting = FAKE_SW_E + cen_c * resolution_m;
            let northing = FAKE_NE_N - cen_r * resolution_m;
            let (latitude, longitude) = utm_to_wgs84_approx(easting, northing);
            let confidence = (max_height / 5.0).clamp(0.0, 1.0);
            let id = format!("{}-{:04}", survey_id, det_idx);
            det_idx += 1;

            detections.push(WreckDetection {
                id,
                latitude,
                longitude,
                easting,
                northing,
                depth_meters: cen_depth,
                size_meters,
                height_above_floor: max_height,
                confidence,
                object_type: ObjectType::Wreck,
                bag_file: "TEST.bag".to_string(),
                survey_id: survey_id.clone(),
                cell_count,
            });
        }
        detections
    }

    #[test]
    fn synthetic_wreck_detected() {
        // 100×100 grid at -30m depth; 5×5 block centred at (50,50) raised to -25m (5m bump).
        let rows = 100usize;
        let cols = 100usize;
        let resolution_m = 4.0;
        let mut elev = vec![-30.0f64; rows * cols];

        for r in 48..53 {
            for c in 48..53 {
                elev[r * cols + c] = -25.0;
            }
        }

        let scanner = BagScanner { min_height_m: 0.5, min_cluster_cells: 3 };
        let detections = run_detection_on_grid(&elev, rows, cols, resolution_m, &scanner);

        assert!(
            !detections.is_empty(),
            "Expected at least one detection for a 5m bump"
        );
        // The dominant detection should have height_above_floor close to 5.0
        let max_det = detections
            .iter()
            .max_by(|a, b| a.height_above_floor.partial_cmp(&b.height_above_floor).unwrap())
            .unwrap();
        assert!(
            max_det.height_above_floor > 1.0,
            "Detection height_above_floor should be > 1.0, got {}",
            max_det.height_above_floor
        );
    }

    #[test]
    fn flat_bottom_no_detections() {
        // Perfectly flat grid → background == grid → height_above == 0 everywhere.
        let rows = 50usize;
        let cols = 50usize;
        let elev = vec![-30.0f64; rows * cols];
        let scanner = BagScanner { min_height_m: 0.5, min_cluster_cells: 3 };
        let detections = run_detection_on_grid(&elev, rows, cols, 4.0, &scanner);
        assert!(
            detections.is_empty(),
            "Expected zero detections on flat seafloor, got {}",
            detections.len()
        );
    }

    #[test]
    fn parse_xml_corners() {
        let xml = r#"<?xml version="1.0"?>
        <some:Root>
          <gml:coordinates>316000.5,4824000.0 317000.5,4825000.0</gml:coordinates>
          <gco:Measure uom="m">2.5</gco:Measure>
        </some:Root>"#;

        let corners = extract_corners(xml).expect("Should parse corners");
        assert!((corners.0 - 316_000.5).abs() < 1e-6, "sw_e mismatch");
        assert!((corners.1 - 4_824_000.0).abs() < 1e-6, "sw_n mismatch");
        assert!((corners.2 - 317_000.5).abs() < 1e-6, "ne_e mismatch");
        assert!((corners.3 - 4_825_000.0).abs() < 1e-6, "ne_n mismatch");

        let res = extract_resolution(xml).expect("Should parse resolution");
        assert!((res - 2.5).abs() < 1e-9, "resolution mismatch");
    }
}
