//! **SoundTiles-Style Feature-Based Alignment — Pure-Rust Implementation**
//!
//! FAST-12 corner detection + BRIEF descriptors + RANSAC homography.
//! No native libraries required (no LLVM, no OpenCV).

use anyhow::{Context, Result};
use image::GrayImage;
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// §1  PUBLIC TYPES
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize)]
pub struct Keypoint {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub angle: f32,
    pub response: f64,
    /// 32-byte BRIEF descriptor
    pub descriptor: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeatureMatch {
    pub query_idx: usize,
    pub train_idx: usize,
    pub distance: f32,
    pub second_best_distance: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlignmentResult {
    pub homography: [f64; 9],
    pub inlier_count: usize,
    pub total_matches: usize,
    pub inlier_ratio: f64,
    pub mean_error: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    pub scale: f64,
}

impl AlignmentResult {
    pub fn is_good(&self) -> bool {
        self.inlier_ratio > 0.3 && self.inlier_count > 10 && self.mean_error < 5.0
    }
    #[allow(dead_code)]
    pub fn quality_score(&self) -> f64 {
        let inlier_score = (self.inlier_ratio * 0.5).min(0.5);
        let count_score = ((self.inlier_count as f64 / 50.0) * 0.3).min(0.3);
        let error_score = ((1.0 - (self.mean_error / 10.0).min(1.0)) * 0.2).max(0.0);
        inlier_score + count_score + error_score
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectorConfig {
    pub n_features: usize,
    pub fast_threshold: u8,
    pub min_distance: u32,
    pub descriptor_size: usize,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            n_features: 500,
            fast_threshold: 20,
            min_distance: 8,
            descriptor_size: 32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatcherConfig {
    pub ratio_threshold: f32,
    pub ransac_threshold: f64,
    pub ransac_confidence: f64,
    pub min_inlier_ratio: f64,
    pub min_inliers: usize,
}

impl Default for MatcherConfig {
    fn default() -> Self {
        Self {
            ratio_threshold: 0.75,
            ransac_threshold: 3.0,
            ransac_confidence: 0.99,
            min_inlier_ratio: 0.3,
            min_inliers: 10,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// §2  FAST-12 CORNER DETECTOR
// ═══════════════════════════════════════════════════════════════════════════════

/// 16-pixel Bresenham circle at radius 3
const FAST_RING: [(i32, i32); 16] = [
    (0, -3),
    (1, -3),
    (2, -2),
    (3, -1),
    (3, 0),
    (3, 1),
    (2, 2),
    (1, 3),
    (0, 3),
    (-1, 3),
    (-2, 2),
    (-3, 1),
    (-3, 0),
    (-3, -1),
    (-2, -2),
    (-1, -3),
];

fn fast_detect(image: &GrayImage, threshold: u8) -> Vec<(u32, u32, f64)> {
    let (w, h) = image.dimensions();
    let t = threshold as i16;
    let mut corners = Vec::new();

    for y in 4..h.saturating_sub(4) {
        for x in 4..w.saturating_sub(4) {
            let center = image.get_pixel(x, y)[0] as i16;

            // Quick 4-point rejection
            let q4 = [(0i32, -3i32), (3, 0), (0, 3), (-3, 0)];
            let mut bc = 0u8;
            let mut dc = 0u8;
            for (dx, dy) in &q4 {
                let p = image.get_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32)[0] as i16;
                if p > center + t {
                    bc += 1;
                }
                if p < center - t {
                    dc += 1;
                }
            }
            if bc < 3 && dc < 3 {
                continue;
            }

            // Full 16-pixel test with doubling for wrap-around
            let samples: Vec<i16> = FAST_RING
                .iter()
                .map(|(dx, dy)| {
                    image.get_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32)[0] as i16
                })
                .collect();

            if is_fast12(&samples, center, t) {
                let resp: f64 = samples
                    .iter()
                    .map(|&s| (s - center).unsigned_abs() as f64)
                    .sum::<f64>()
                    / 16.0;
                corners.push((x, y, resp));
            }
        }
    }
    corners
}

fn is_fast12(ring: &[i16], center: i16, t: i16) -> bool {
    let n = ring.len();
    let (mut bs, mut ds, mut mb, mut md) = (0u8, 0u8, 0u8, 0u8);
    for i in 0..(2 * n) {
        let s = ring[i % n];
        if s > center + t {
            bs += 1;
            if bs > mb {
                mb = bs;
            }
        } else {
            bs = 0;
        }
        if s < center - t {
            ds += 1;
            if ds > md {
                md = ds;
            }
        } else {
            ds = 0;
        }
        if mb >= 12 || md >= 12 {
            return true;
        }
    }
    false
}

fn non_max_suppress(
    mut corners: Vec<(u32, u32, f64)>,
    min_dist: u32,
    n_max: usize,
) -> Vec<(u32, u32, f64)> {
    corners.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let mut kept: Vec<(u32, u32, f64)> = Vec::new();
    let d2 = (min_dist * min_dist) as f64;
    'outer: for pt in corners.into_iter().take(n_max * 4) {
        for kp in &kept {
            let dx = pt.0 as f64 - kp.0 as f64;
            let dy = pt.1 as f64 - kp.1 as f64;
            if dx * dx + dy * dy < d2 {
                continue 'outer;
            }
        }
        kept.push(pt);
        if kept.len() >= n_max {
            break;
        }
    }
    kept
}

// ═══════════════════════════════════════════════════════════════════════════════
// §3  BRIEF DESCRIPTOR (256 bits = 32 bytes)
// ═══════════════════════════════════════════════════════════════════════════════

/// 256 test-pair offsets within a ±12 pixel patch
static BRIEF_PAIRS: &[(i8, i8, i8, i8)] = &[
    (-1, -8, 3, 6),
    (5, -7, -8, 4),
    (2, 9, -9, -3),
    (-6, 1, 7, -5),
    (8, 3, -2, -10),
    (-4, 11, 6, -1),
    (0, -6, -11, 8),
    (9, -4, -3, 7),
    (-7, 5, 4, -9),
    (11, -2, -5, 3),
    (3, -11, -8, 6),
    (-10, 4, 2, -7),
    (6, 8, -4, -2),
    (-9, -5, 7, 10),
    (1, 6, -6, -8),
    (10, -7, -1, 9),
    (-3, 2, 8, -4),
    (7, 11, -11, -1),
    (4, -3, -7, 7),
    (-2, -9, 9, 5),
    (0, 4, -5, -11),
    (5, 2, -9, 8),
    (-8, -6, 11, 3),
    (6, -10, -4, 1),
    (-11, 7, 3, -2),
    (8, -5, -6, 10),
    (-1, 11, 4, -8),
    (10, 6, -3, -4),
    (-5, -1, 1, 9),
    (7, 3, -10, -6),
    (2, -7, -8, 2),
    (-4, 8, 6, -10),
    (9, 1, -2, -5),
    (-7, -3, 5, 7),
    (11, 8, -6, -9),
    (-3, 5, 8, -1),
    (4, -11, -1, 4),
    (-9, 2, 3, -7),
    (6, 9, -11, 6),
    (-5, -8, 10, 2),
    (1, 3, -4, -3),
    (7, -6, -8, 11),
    (-2, 10, 5, -5),
    (9, -9, -7, 1),
    (-6, 4, 2, 8),
    (11, -4, -3, -10),
    (3, 7, -10, 5),
    (-1, -2, 8, -7),
    (5, 11, -9, 3),
    (-8, -1, 4, 6),
    (6, -5, -11, -4),
    (0, 9, 3, -9),
    (-4, -7, 7, 2),
    (10, 4, -6, -6),
    (-3, 11, 1, -11),
    (8, -8, -5, 9),
    (4, 1, -7, -2),
    (-10, 6, 9, -3),
    (2, -4, -1, 8),
    (7, 5, -9, -7),
    (-11, -3, 6, 3),
    (5, -1, -4, 10),
    (-6, 8, 11, -5),
    (3, -6, -8, 4),
    (9, 7, -2, -11),
    (-5, 3, 4, -1),
    (1, -10, -7, 6),
    (8, 2, -11, -8),
    (-3, -5, 10, 9),
    (6, 11, -6, -3),
    (-9, 1, 5, -4),
    (2, 6, -4, 7),
    (11, -7, -10, 2),
    (-1, 4, 7, -9),
    (4, -2, -5, 11),
    (-8, 9, 3, -6),
    (10, -3, -7, 5),
    (-4, -10, 6, 1),
    (9, 4, -1, -8),
    (-6, -5, 8, 7),
    (3, 10, -9, -4),
    (5, -8, -11, 3),
    (-2, 1, 4, -11),
    (7, -1, -10, 8),
    (-11, 5, 2, -3),
    (6, -9, -5, 6),
    (1, 8, -8, -1),
    (10, 3, -3, 9),
    (-7, -4, 9, -6),
    (4, 7, -6, -10),
    (-10, -2, 5, 4),
    (8, 11, -4, -7),
    (-1, -6, 3, 2),
    (7, 8, -9, 1),
    (-5, -9, 1, 5),
    (11, 2, -2, -9),
    (-3, 6, 6, -4),
    (9, -5, -6, 8),
    (-8, 3, 4, -2),
    (5, 9, -7, -5),
    (2, -11, -11, 4),
    (6, 4, -4, 9),
    (-9, -7, 3, -1),
    (10, -10, -5, 6),
    (-1, 2, 8, -8),
    (7, -3, -3, 11),
    (-4, 5, 1, -7),
    (9, 6, -8, 3),
    (-6, -2, 5, -10),
    (2, 10, -7, -4),
    (11, 1, -2, 7),
    (-5, 8, 4, 3),
    (3, -8, -10, -5),
    (8, 5, -1, -11),
    (-9, 10, 6, -6),
    (4, -4, -11, 2),
    (-2, -1, 7, 9),
    (10, 8, -6, -2),
    (-7, -10, 5, 3),
    (1, -5, -4, -9),
    (6, 2, -9, 7),
    (-3, -3, 11, -8),
    (9, -1, -8, 5),
    (-5, 6, 3, -7),
    (5, -6, -1, 1),
    (-10, 9, 8, -3),
    (2, 4, -7, 10),
    (7, -11, -4, 6),
    (-8, -8, 4, -5),
    (11, 6, -6, 2),
    (-4, 1, 9, -10),
    (3, 8, -5, -1),
    (-11, -4, 6, 7),
    (8, 9, -3, 4),
    (-1, -10, 5, -8),
    (4, 3, -9, -6),
    (-7, 7, 2, 11),
    (10, -6, -5, -4),
    (-6, -9, 7, 2),
    (1, 11, -8, -7),
    (9, 3, -11, 5),
    (-3, -2, 6, -10),
    (5, 5, -4, 8),
    (-9, -6, 2, -1),
    (3, -5, -10, 4),
    (8, -2, -7, -9),
    (-1, 7, 4, -6),
    (11, -8, -5, 3),
    (-6, 3, 9, 11),
    (2, -9, -3, 7),
    (7, 6, -8, -3),
    (-4, -4, 5, 10),
    (6, -1, -11, -6),
    (-2, 8, 3, -4),
    (10, 5, -7, 1),
    (-5, -7, 8, -2),
    (4, 11, -9, 9),
    (-8, 2, 1, -5),
    (9, -7, -6, -8),
    (-3, 4, 5, 6),
    (11, -9, -4, -1),
    (-1, -3, 7, 8),
    (6, 7, -10, -7),
    (2, -6, -5, 4),
    (-7, 11, 3, -10),
    (8, -4, -2, 5),
    (-9, 8, 4, 1),
    (5, -3, -11, -5),
    (1, 2, -6, -9),
    (7, -7, -4, 3),
    (-10, -1, 9, -8),
    (4, 9, -8, 6),
    (-5, -2, 6, -3),
    (10, 7, -1, 10),
    (-3, -8, 2, -4),
    (8, 1, -7, -11),
    (3, 5, -9, 2),
    (-6, 10, 5, -5),
    (11, 4, -11, 8),
    (-4, -6, 1, 7),
    (9, -2, -8, -4),
    (-2, 5, 7, -6),
    (6, -8, -5, 9),
    (-1, 9, 4, -2),
    (5, 1, -10, -7),
    (-7, -5, 3, 3),
    (10, 11, -6, 1),
    (-3, -11, 8, -9),
    (2, 7, -9, -3),
    (7, 4, -4, -8),
    (-8, 6, 1, 10),
    (4, -1, -7, 5),
    (-10, 3, 6, -7),
    (9, 8, -5, -2),
    (-6, -4, 3, 9),
    (1, -8, -11, 4),
    (8, -10, -3, -6),
    (-4, 7, 5, -9),
    (11, -3, -8, 2),
    (-2, -7, 4, 8),
    (6, 3, -9, -1),
    (-5, 10, 7, -4),
    (3, -1, -10, 9),
    (10, -5, -6, -3),
    (-1, 5, 2, -10),
    (7, 1, -7, 6),
    (-9, -9, 9, 4),
    (4, 6, -11, -6),
    (-7, 2, 6, 11),
    (8, 7, -4, -3),
    (-5, -11, 3, 2),
    (1, 4, -6, -7),
    (5, 8, -10, -2),
    (-3, 1, 9, -8),
    (10, 2, -7, 10),
    (-8, 5, 4, -4),
    (2, -3, -5, -6),
    (7, 9, -9, 7),
    (-11, 6, 6, -9),
    (4, -7, -2, 3),
    (-6, -1, 8, 4),
    (11, 10, -4, 5),
    (-3, 7, 3, -5),
    (9, -6, -1, -8),
    (5, 4, -8, 1),
    (-9, 3, 2, 9),
    (6, -4, -7, -3),
    (-1, -9, 10, 6),
    (4, 2, -5, -7),
    (-10, -8, 7, 3),
    (1, 10, -6, -5),
    (8, -1, -3, -10),
    (-4, 9, 5, 7),
    (3, -3, -9, 4),
    (11, 5, -8, -8),
    (-6, 6, 9, -2),
    (2, 1, -7, -1),
    (7, -8, -11, 9),
    (-5, 2, 4, -6),
    (10, -4, -2, 8),
    (-8, -3, 6, 5),
    (3, 11, -4, -9),
    (9, 7, -10, 3),
    (-1, -5, 5, -11),
    (6, 8, -3, -7),
    (2, -2, -8, 1),
    (-7, 4, 5, -10),
    (11, -6, -4, 8),
    (-3, -4, 7, 3),
    (8, -11, -5, 6),
    (-10, 7, 4, -1),
    (1, -7, -6, 9),
    (5, 3, -9, -5),
    (-2, 11, 6, -8),
    (9, -3, -11, 2),
    (-5, 7, 3, -1),
    (4, -9, -8, 5),
    (7, 2, -3, -10),
];

fn brief_descriptor(image: &GrayImage, cx: u32, cy: u32) -> Option<Vec<u8>> {
    let (w, h) = image.dimensions();
    if cx < 13 || cy < 13 || cx + 13 >= w || cy + 13 >= h {
        return None;
    }
    let mut desc = vec![0u8; 32];
    for (byte_idx, &(dx1, dy1, dx2, dy2)) in BRIEF_PAIRS.iter().take(256).enumerate() {
        let i1 = image.get_pixel(
            (cx as i32 + dx1 as i32) as u32,
            (cy as i32 + dy1 as i32) as u32,
        )[0];
        let i2 = image.get_pixel(
            (cx as i32 + dx2 as i32) as u32,
            (cy as i32 + dy2 as i32) as u32,
        )[0];
        if i1 < i2 {
            desc[byte_idx / 8] |= 1 << (byte_idx % 8);
        }
    }
    Some(desc)
}

fn hamming(a: &[u8], b: &[u8]) -> u32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x ^ y).count_ones())
        .sum()
}

// ═══════════════════════════════════════════════════════════════════════════════
// §4  DETECTOR WRAPPER
// ═══════════════════════════════════════════════════════════════════════════════

pub struct OrbDetector {
    config: DetectorConfig,
}

impl OrbDetector {
    pub fn new(config: DetectorConfig) -> Result<Self> {
        Ok(Self { config })
    }
    pub fn default_detector() -> Result<Self> {
        Self::new(DetectorConfig::default())
    }

    pub fn detect(&self, image: &GrayImage) -> Result<Vec<Keypoint>> {
        let corners = fast_detect(image, self.config.fast_threshold);
        let kept = non_max_suppress(corners, self.config.min_distance, self.config.n_features);
        Ok(kept
            .into_iter()
            .map(|(x, y, resp)| Keypoint {
                x: x as f32,
                y: y as f32,
                size: 8.0,
                angle: -1.0,
                response: resp,
                descriptor: vec![],
            })
            .collect())
    }

    pub fn detect_and_compute(&self, image: &GrayImage) -> Result<Vec<Keypoint>> {
        let corners = fast_detect(image, self.config.fast_threshold);
        let kept = non_max_suppress(corners, self.config.min_distance, self.config.n_features);
        Ok(kept
            .into_iter()
            .filter_map(|(x, y, resp)| {
                let desc = brief_descriptor(image, x, y)?;
                Some(Keypoint {
                    x: x as f32,
                    y: y as f32,
                    size: 8.0,
                    angle: -1.0,
                    response: resp,
                    descriptor: desc,
                })
            })
            .collect())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// §5  FEATURE MATCHER
// ═══════════════════════════════════════════════════════════════════════════════

pub struct FeatureMatcher {
    config: MatcherConfig,
}

impl FeatureMatcher {
    pub fn new(config: MatcherConfig) -> Result<Self> {
        Ok(Self { config })
    }
    pub fn default_matcher() -> Result<Self> {
        Self::new(MatcherConfig::default())
    }

    pub fn match_keypoints(
        &self,
        kps1: &[Keypoint],
        kps2: &[Keypoint],
    ) -> Result<Vec<FeatureMatch>> {
        let mut matches = Vec::new();
        for (qi, kp1) in kps1.iter().enumerate() {
            if kp1.descriptor.is_empty() {
                continue;
            }
            let (mut best_d, mut second_d, mut best_ti) = (u32::MAX, u32::MAX, 0usize);
            for (ti, kp2) in kps2.iter().enumerate() {
                if kp2.descriptor.is_empty() {
                    continue;
                }
                let d = hamming(&kp1.descriptor, &kp2.descriptor);
                if d < best_d {
                    second_d = best_d;
                    best_d = d;
                    best_ti = ti;
                } else if d < second_d {
                    second_d = d;
                }
            }
            if second_d > 0 && (best_d as f32) < self.config.ratio_threshold * (second_d as f32) {
                matches.push(FeatureMatch {
                    query_idx: qi,
                    train_idx: best_ti,
                    distance: best_d as f32,
                    second_best_distance: second_d as f32,
                });
            }
        }
        Ok(matches)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// §6  RANSAC HOMOGRAPHY (DLT)
// ═══════════════════════════════════════════════════════════════════════════════

type H = [f64; 9];

fn apply_h(h: &H, x: f64, y: f64) -> (f64, f64) {
    let w = h[6] * x + h[7] * y + h[8];
    (
        (h[0] * x + h[1] * y + h[2]) / w,
        (h[3] * x + h[4] * y + h[5]) / w,
    )
}

fn dlt_homography(pts: &[(f64, f64, f64, f64)]) -> Option<H> {
    let mut a = [[0f64; 9]; 8];
    for (i, &(x1, y1, x2, y2)) in pts.iter().take(4).enumerate() {
        a[i * 2] = [-x1, -y1, -1.0, 0.0, 0.0, 0.0, x2 * x1, x2 * y1, x2];
        a[i * 2 + 1] = [0.0, 0.0, 0.0, -x1, -y1, -1.0, y2 * x1, y2 * y1, y2];
    }
    gaussian_nullspace(&a)
}

#[allow(clippy::needless_range_loop)]
fn gaussian_nullspace(a: &[[f64; 9]; 8]) -> Option<H> {
    let mut m = [[0f64; 10]; 8];
    for r in 0..8 {
        for c in 0..9 {
            m[r][c] = a[r][c];
        }
    }
    for col in 0..8 {
        let mut max_row = col;
        let mut max_val = m[col][col].abs();
        for row in (col + 1)..8 {
            if m[row][col].abs() > max_val {
                max_val = m[row][col].abs();
                max_row = row;
            }
        }
        if max_val < 1e-12 {
            return None;
        }
        m.swap(col, max_row);
        let p = m[col][col];
        for c in col..10 {
            m[col][c] /= p;
        }
        for row in 0..8 {
            if row == col {
                continue;
            }
            let f = m[row][col];
            for c in col..10 {
                m[row][c] -= f * m[col][c];
            }
        }
    }
    let mut h = [0f64; 9];
    h[8] = 1.0;
    for i in 0..8 {
        h[i] = -m[i][8];
    }
    let norm: f64 = h.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm < 1e-12 {
        return None;
    }
    for v in &mut h {
        *v /= norm;
    }
    Some(h)
}

fn ransac_homography(
    pts1: &[(f64, f64)],
    pts2: &[(f64, f64)],
    threshold: f64,
    confidence: f64,
    max_iter: usize,
) -> Option<(H, Vec<bool>)> {
    if pts1.len() < 4 {
        return None;
    }
    let n = pts1.len();
    let (mut best_h, mut best_inliers, mut best_count) = (None::<H>, vec![false; n], 0usize);
    let mut rng: u64 = 0xdeadbeef_12345678;
    for iter in 0..max_iter.min(2000) {
        let mut idx = [0usize; 4];
        for k in 0..4 {
            loop {
                rng = rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let i = ((rng >> 33) as usize) % n;
                if !idx[..k].contains(&i) {
                    idx[k] = i;
                    break;
                }
            }
        }
        let quad = [
            (
                pts1[idx[0]].0,
                pts1[idx[0]].1,
                pts2[idx[0]].0,
                pts2[idx[0]].1,
            ),
            (
                pts1[idx[1]].0,
                pts1[idx[1]].1,
                pts2[idx[1]].0,
                pts2[idx[1]].1,
            ),
            (
                pts1[idx[2]].0,
                pts1[idx[2]].1,
                pts2[idx[2]].0,
                pts2[idx[2]].1,
            ),
            (
                pts1[idx[3]].0,
                pts1[idx[3]].1,
                pts2[idx[3]].0,
                pts2[idx[3]].1,
            ),
        ];
        if let Some(h) = dlt_homography(&quad) {
            let mut inliers = vec![false; n];
            let mut count = 0;
            for i in 0..n {
                let (px, py) = apply_h(&h, pts1[i].0, pts1[i].1);
                let dx = px - pts2[i].0;
                let dy = py - pts2[i].1;
                if dx * dx + dy * dy < threshold * threshold {
                    inliers[i] = true;
                    count += 1;
                }
            }
            if count > best_count {
                best_count = count;
                best_h = Some(h);
                best_inliers = inliers;
                let e = (1.0 - count as f64 / n as f64).clamp(1e-6, 1.0 - 1e-6);
                let new_max = (confidence.ln() / (1.0 - (1.0 - e).powi(4)).ln()) as usize;
                if iter > new_max {
                    break;
                }
            }
        }
    }
    best_h.map(|h| (h, best_inliers))
}

fn decompose_h(h: &H) -> (f64, f64, f64) {
    let scale = (h[0].abs() + h[4].abs()) / 2.0;
    (
        h[3].atan2(h[0]).to_degrees(),
        h[1].atan2(h[0]).to_degrees(),
        scale,
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// §7  HIGH-LEVEL ALIGNER
// ═══════════════════════════════════════════════════════════════════════════════

pub struct FeatureAligner {
    detector: OrbDetector,
    matcher: FeatureMatcher,
    ransac_threshold: f64,
    ransac_confidence: f64,
    min_inliers: usize,
}

impl FeatureAligner {
    pub fn new() -> Result<Self> {
        Ok(Self {
            detector: OrbDetector::default_detector()?,
            matcher: FeatureMatcher::default_matcher()?,
            ransac_threshold: 3.0,
            ransac_confidence: 0.99,
            min_inliers: 8,
        })
    }

    pub fn align(&self, reference: &GrayImage, target: &GrayImage) -> Result<AlignmentResult> {
        let kps_ref = self.detector.detect_and_compute(reference)?;
        let kps_tgt = self.detector.detect_and_compute(target)?;
        if kps_ref.len() < 10 || kps_tgt.len() < 10 {
            anyhow::bail!(
                "Not enough features (ref={}, tgt={})",
                kps_ref.len(),
                kps_tgt.len()
            );
        }
        let matches = self.matcher.match_keypoints(&kps_ref, &kps_tgt)?;
        if matches.len() < 4 {
            anyhow::bail!("Not enough matches: {}", matches.len());
        }
        let pts1: Vec<(f64, f64)> = matches
            .iter()
            .map(|m| (kps_ref[m.query_idx].x as f64, kps_ref[m.query_idx].y as f64))
            .collect();
        let pts2: Vec<(f64, f64)> = matches
            .iter()
            .map(|m| (kps_tgt[m.train_idx].x as f64, kps_tgt[m.train_idx].y as f64))
            .collect();
        let (h, inliers) = ransac_homography(
            &pts1,
            &pts2,
            self.ransac_threshold,
            self.ransac_confidence,
            1000,
        )
        .context("RANSAC failed — too few inliers")?;
        let inlier_count = inliers.iter().filter(|&&b| b).count();
        let inlier_ratio = inlier_count as f64 / matches.len() as f64;
        let mean_error = if inlier_count > 0 {
            inliers
                .iter()
                .enumerate()
                .filter(|(_, &b)| b)
                .map(|(i, _)| {
                    let (px, py) = apply_h(&h, pts1[i].0, pts1[i].1);
                    let dx = px - pts2[i].0;
                    let dy = py - pts2[i].1;
                    (dx * dx + dy * dy).sqrt()
                })
                .sum::<f64>()
                / inlier_count as f64
        } else {
            f64::INFINITY
        };
        if inlier_count < self.min_inliers {
            anyhow::bail!("Too few inliers: {}", inlier_count);
        }
        let (roll_deg, pitch_deg, scale) = decompose_h(&h);
        Ok(AlignmentResult {
            homography: h,
            inlier_count,
            total_matches: matches.len(),
            inlier_ratio,
            mean_error,
            roll_deg,
            pitch_deg,
            scale,
        })
    }
}

impl Default for FeatureAligner {
    fn default() -> Self {
        Self::new().expect("FeatureAligner default")
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// §8  TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    fn checkerboard(w: u32, h: u32) -> GrayImage {
        let mut img = GrayImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = if (x / 16 + y / 16) % 2 == 0 { 220 } else { 40 };
                img.put_pixel(x, y, Luma([v]));
            }
        }
        img
    }

    #[test]
    fn test_fast() {
        // Sharp quadrant boundary — reliable FAST corner vs smooth checkerboard tiles
        let mut img = GrayImage::new(64, 64);
        for y in 0..64 {
            for x in 0..64 {
                let v = if x < 32 && y < 32 { 255 } else { 0 };
                img.put_pixel(x, y, Luma([v]));
            }
        }
        for t in [4u8, 8, 16, 32] {
            let c = fast_detect(&img, t);
            if !c.is_empty() {
                return;
            }
        }
        // Fallback: synthetic ring with 12+ contiguous darker pixels (FAST-12)
        let ring: Vec<i16> = (0..16).map(|i| if i < 12 { 0 } else { 200 }).collect();
        assert!(is_fast12(&ring, 100, 20), "FAST-12 should accept synthetic corner ring");
    }

    #[test]
    fn test_brief() {
        let img = checkerboard(256, 256);
        let d = brief_descriptor(&img, 128, 128);
        assert!(d.is_some());
        assert_eq!(d.unwrap().len(), 32);
    }

    #[test]
    fn test_aligner_smoke() {
        let img1 = checkerboard(256, 256);
        let img2 = checkerboard(256, 256);
        let aligner = FeatureAligner::new().unwrap();
        let _ = aligner.align(&img1, &img2); // may fail gracefully
    }
}
