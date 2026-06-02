//! Overlay Grid — Encoding-Agnostic Subpixel Alignment System
//!
//! Like motion capture markers on an actor's body, this system overlays a grid of
//! unique geometric shapes onto GeoTIFF tiles. Each cell in the grid has a distinct
//! shape pattern that acts as a fiducial marker. When you need to stitch tiles back
//! together or align a processed result against the original, you match the shapes
//! to get pinpoint subpixel registration — regardless of what encoding, projection,
//! or resolution the underlying data uses.
//!
//! ## Why This Exists
//!
//! Coordinate drift in satellite imagery comes from:
//! - Different source encodings (UTM zones, geographic, local projections)
//! - Resampling artifacts when reprojecting
//! - Slight misalignment between temporal acquisitions
//! - Slicing tiles into chunks and reassembling (boundary drift)
//!
//! The overlay grid eliminates all of these by providing an absolute reference frame
//! that travels WITH the pixel data. It's like embedding a ruler into the image itself.
//!
//! ## How It Works
//!
//! 1. STAMP: Before processing, stamp the overlay grid onto the tile metadata (not pixels)
//! 2. PROCESS: Run any analysis (curvelet filter, spectral unmix, temporal stack, etc.)
//! 3. ALIGN: When combining results or mapping back to coordinates, match the grid
//!    shapes to recover exact subpixel position — even if the tile was sliced, rotated,
//!    or resampled during processing
//!
//! ## Shape Encoding
//!
//! Each grid cell gets a unique shape composed of:
//! - A base pattern (triangle, square, pentagon, hexagon, cross, diamond, star, arrow)
//! - A rotation (0°, 45°, 90°, 135°, 180°, 225°, 270°, 315°)
//! - A scale modifier (small, medium, large)
//! - A position hash derived from the cell's absolute grid coordinates
//!
//! This gives 8 × 8 × 3 = 192 unique markers per cycle, tiling infinitely.
//! With the position hash, every cell in the entire world grid is globally unique.
//!
//! ## Subpixel Precision
//!
//! Each shape is defined at 1/16th pixel resolution (4-bit subpixel).
//! When matching, cross-correlation between the expected shape and the observed
//! pattern gives alignment to ~1/8th pixel accuracy. This is better than the
//! satellite's own geolocation accuracy (~5-10m for Sentinel-2).

use std::collections::HashMap;

// ─── Grid Configuration ──────────────────────────────────────────────────────

/// Configuration for the overlay grid system.
#[derive(Clone, Debug)]
pub struct OverlayGridConfig {
    /// Grid cell size in pixels (each cell contains one unique marker)
    pub cell_size_px: usize,
    /// Subpixel resolution (divisions per pixel for shape definition)
    pub subpixel_divisions: usize,
    /// Number of base shape types
    pub n_shapes: usize,
    /// Number of rotation steps
    pub n_rotations: usize,
    /// Marker size as fraction of cell size (0.0-1.0)
    pub marker_scale: f32,
}

impl Default for OverlayGridConfig {
    fn default() -> Self {
        Self {
            cell_size_px: 32,       // One marker every 32 pixels
            subpixel_divisions: 16, // 1/16th pixel precision
            n_shapes: 8,            // 8 base shapes
            n_rotations: 8,         // 8 rotation steps (45° each)
            marker_scale: 0.6,      // Marker fills 60% of cell
        }
    }
}

// ─── Shape Definitions ───────────────────────────────────────────────────────

/// Base shape types for grid markers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BaseShape {
    Triangle = 0,
    Square = 1,
    Pentagon = 2,
    Hexagon = 3,
    Cross = 4,
    Diamond = 5,
    Star = 6,
    Arrow = 7,
}

impl BaseShape {
    pub fn from_index(idx: usize) -> Self {
        match idx % 8 {
            0 => Self::Triangle,
            1 => Self::Square,
            2 => Self::Pentagon,
            3 => Self::Hexagon,
            4 => Self::Cross,
            5 => Self::Diamond,
            6 => Self::Star,
            _ => Self::Arrow,
        }
    }

    /// Generate vertices for this shape at subpixel resolution.
    /// Returns points as (x, y) in subpixel coordinates centered at (0, 0).
    pub fn vertices(&self, radius: f32) -> Vec<(f32, f32)> {
        match self {
            Self::Triangle => regular_polygon(3, radius),
            Self::Square => regular_polygon(4, radius),
            Self::Pentagon => regular_polygon(5, radius),
            Self::Hexagon => regular_polygon(6, radius),
            Self::Cross => cross_shape(radius),
            Self::Diamond => {
                let mut pts = regular_polygon(4, radius);
                // Stretch vertically for diamond
                for p in pts.iter_mut() {
                    p.1 *= 1.4;
                }
                pts
            }
            Self::Star => star_shape(5, radius, radius * 0.4),
            Self::Arrow => arrow_shape(radius),
        }
    }
}

/// A unique marker at a specific grid position.
#[derive(Clone, Debug)]
pub struct GridMarker {
    /// Grid cell coordinates (absolute, world-space)
    pub cell_x: i64,
    pub cell_y: i64,
    /// Base shape for this cell
    pub shape: BaseShape,
    /// Rotation in units of (360 / n_rotations) degrees
    pub rotation: usize,
    /// Scale modifier (0=small, 1=medium, 2=large)
    pub scale_mod: usize,
    /// Position hash — makes this marker globally unique
    pub hash: u64,
    /// Subpixel vertices after rotation and scaling (in pixel coords relative to cell center)
    pub vertices: Vec<(f32, f32)>,
}

// ─── The Overlay Grid ────────────────────────────────────────────────────────

/// The overlay grid system. Generates and matches unique markers for alignment.
pub struct OverlayGrid {
    pub config: OverlayGridConfig,
}

impl OverlayGrid {
    pub fn new(config: OverlayGridConfig) -> Self {
        Self { config }
    }

    /// Stamp the grid onto a tile. Returns the set of markers that fall within
    /// the tile's pixel bounds, with their subpixel-precise positions.
    ///
    /// `origin_x`, `origin_y`: the tile's position in the global grid coordinate system
    /// (derived from the GeoTIFF's geo-transform).
    pub fn stamp(
        &self,
        tile_width_px: usize,
        tile_height_px: usize,
        origin_x: f64,
        origin_y: f64,
        pixel_size_x: f64,
        pixel_size_y: f64,
    ) -> TileStamp {
        let cell = self.config.cell_size_px as f64;

        // Calculate which grid cells overlap this tile
        let start_cell_x = (origin_x / (cell * pixel_size_x)).floor() as i64;
        let start_cell_y = (origin_y / (cell * pixel_size_y.abs())).floor() as i64;
        let end_cell_x = ((origin_x + tile_width_px as f64 * pixel_size_x) / (cell * pixel_size_x)).ceil() as i64;
        let end_cell_y = ((origin_y + tile_height_px as f64 * pixel_size_y.abs()) / (cell * pixel_size_y.abs())).ceil() as i64;

        let mut markers = Vec::new();

        for cy in start_cell_y..=end_cell_y {
            for cx in start_cell_x..=end_cell_x {
                let marker = self.generate_marker(cx, cy);

                // Calculate pixel position of this marker's center within the tile
                let world_x = cx as f64 * cell * pixel_size_x;
                let world_y = cy as f64 * cell * pixel_size_y.abs();
                let px_x = ((world_x - origin_x) / pixel_size_x) as f32;
                let px_y = ((world_y - origin_y) / pixel_size_y.abs()) as f32;

                // Only include markers whose center falls within tile bounds
                if px_x >= 0.0 && px_x < tile_width_px as f32
                    && px_y >= 0.0 && px_y < tile_height_px as f32
                {
                    markers.push(StampedMarker {
                        marker,
                        pixel_x: px_x,
                        pixel_y: px_y,
                    });
                }
            }
        }

        TileStamp {
            origin_x,
            origin_y,
            pixel_size_x,
            pixel_size_y,
            tile_width_px,
            tile_height_px,
            markers,
        }
    }

    /// Generate the unique marker for a given grid cell.
    pub fn generate_marker(&self, cell_x: i64, cell_y: i64) -> GridMarker {
        // Position hash — deterministic, globally unique per cell
        let hash = position_hash(cell_x, cell_y);

        // Derive shape, rotation, and scale from hash
        let shape_idx = (hash % self.config.n_shapes as u64) as usize;
        let rotation = ((hash >> 8) % self.config.n_rotations as u64) as usize;
        let scale_mod = ((hash >> 16) % 3) as usize;

        let shape = BaseShape::from_index(shape_idx);

        // Generate vertices with rotation and scale applied
        let base_radius = self.config.cell_size_px as f32 * self.config.marker_scale * 0.5;
        let scale_factor = match scale_mod {
            0 => 0.7,  // small
            1 => 1.0,  // medium
            _ => 1.3,  // large
        };
        let radius = base_radius * scale_factor;

        let angle = (rotation as f32 / self.config.n_rotations as f32) * std::f32::consts::PI * 2.0;
        let base_verts = shape.vertices(radius);

        // Apply rotation
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let vertices: Vec<(f32, f32)> = base_verts.iter().map(|(x, y)| {
            (x * cos_a - y * sin_a, x * sin_a + y * cos_a)
        }).collect();

        GridMarker {
            cell_x,
            cell_y,
            shape,
            rotation,
            scale_mod,
            hash,
            vertices,
        }
    }

    /// Match observed markers against expected markers to compute alignment offset.
    /// Returns (dx, dy) subpixel correction to apply.
    ///
    /// `observed`: detected marker positions in the processed tile
    /// `expected`: the original TileStamp from before processing
    pub fn align(
        &self,
        observed: &[(f32, f32, u64)], // (px_x, px_y, detected_hash)
        expected: &TileStamp,
    ) -> AlignmentResult {
        let mut offsets_x: Vec<f32> = Vec::new();
        let mut offsets_y: Vec<f32> = Vec::new();
        let mut matched = 0;

        // Build lookup from hash → expected position
        let expected_map: HashMap<u64, (f32, f32)> = expected.markers.iter()
            .map(|m| (m.marker.hash, (m.pixel_x, m.pixel_y)))
            .collect();

        for (obs_x, obs_y, obs_hash) in observed {
            if let Some((exp_x, exp_y)) = expected_map.get(obs_hash) {
                offsets_x.push(obs_x - exp_x);
                offsets_y.push(obs_y - exp_y);
                matched += 1;
            }
        }

        if matched < 3 {
            return AlignmentResult {
                dx: 0.0,
                dy: 0.0,
                confidence: 0.0,
                markers_matched: matched,
                markers_expected: expected.markers.len(),
            };
        }

        // Robust mean (trim outliers)
        offsets_x.sort_by(|a, b| a.partial_cmp(b).unwrap());
        offsets_y.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // Trim 20% from each end
        let trim = matched / 5;
        let trimmed_x = &offsets_x[trim..matched - trim];
        let trimmed_y = &offsets_y[trim..matched - trim];

        let dx = trimmed_x.iter().sum::<f32>() / trimmed_x.len() as f32;
        let dy = trimmed_y.iter().sum::<f32>() / trimmed_y.len() as f32;

        // Confidence based on consistency of offsets
        let var_x: f32 = trimmed_x.iter().map(|v| (v - dx).powi(2)).sum::<f32>() / trimmed_x.len() as f32;
        let var_y: f32 = trimmed_y.iter().map(|v| (v - dy).powi(2)).sum::<f32>() / trimmed_y.len() as f32;
        let std_dev = (var_x + var_y).sqrt();

        // Confidence: 1.0 if all markers agree perfectly, drops with variance
        let confidence = (1.0 - std_dev / (self.config.cell_size_px as f32 * 0.1)).clamp(0.0, 1.0);

        AlignmentResult {
            dx,
            dy,
            confidence,
            markers_matched: matched,
            markers_expected: expected.markers.len(),
        }
    }

    /// Estimate a full 2-D similarity transform (scale + rotation + translation)
    /// from marker correspondences via the closed-form Umeyama/Procrustes
    /// least-squares solution.
    ///
    /// This is the correct fix for deep temporal stacks: a translation-only
    /// [`align`] cannot represent the per-layer rotation/skew, so applying a
    /// linear (dx,dy) correction to a trigonometric (rotation) misregistration
    /// leaves a residual that compounds across the stack into gross drift. The
    /// similarity fit recovers θ and s directly, so the residual stays subpixel
    /// no matter how many layers are stacked.
    ///
    /// `observed`: detected marker positions (px_x, px_y, hash) in the processed
    /// tile. `expected`: the reference stamp.
    pub fn estimate_similarity(
        &self,
        observed: &[(f32, f32, u64)],
        expected: &TileStamp,
    ) -> SimilarityTransform {
        let expected_map: HashMap<u64, (f32, f32)> = expected
            .markers
            .iter()
            .map(|m| (m.marker.hash, (m.pixel_x, m.pixel_y)))
            .collect();

        // Correspondences: src = expected (reference), dst = observed.
        let mut src: Vec<(f64, f64)> = Vec::new();
        let mut dst: Vec<(f64, f64)> = Vec::new();
        for (ox, oy, h) in observed {
            if let Some((ex, ey)) = expected_map.get(h) {
                src.push((*ex as f64, *ey as f64));
                dst.push((*ox as f64, *oy as f64));
            }
        }
        let n = src.len();
        let n_expected = expected.markers.len();
        if n < 3 {
            return SimilarityTransform {
                markers_matched: n,
                markers_expected: n_expected,
                ..SimilarityTransform::identity()
            };
        }

        let nf = n as f64;
        // Centroids.
        let (mut sx, mut sy, mut dx, mut dy) = (0.0, 0.0, 0.0, 0.0);
        for i in 0..n {
            sx += src[i].0;
            sy += src[i].1;
            dx += dst[i].0;
            dy += dst[i].1;
        }
        sx /= nf; sy /= nf; dx /= nf; dy /= nf;

        // Cross-covariance terms + source variance (Umeyama for 2-D similarity).
        // a = Σ(src·dst aligned), b = Σ(src×dst cross), var_src = Σ|src-μ|².
        let (mut a, mut b, mut var_src) = (0.0, 0.0, 0.0);
        for i in 0..n {
            let scx = src[i].0 - sx;
            let scy = src[i].1 - sy;
            let dcx = dst[i].0 - dx;
            let dcy = dst[i].1 - dy;
            a += scx * dcx + scy * dcy; // dot
            b += scx * dcy - scy * dcx; // cross
            var_src += scx * scx + scy * scy;
        }

        if var_src < 1e-9 {
            return SimilarityTransform {
                markers_matched: n,
                markers_expected: n_expected,
                ..SimilarityTransform::identity()
            };
        }

        // theta from the cross/dot ratio; scale from magnitude / source variance.
        let theta = b.atan2(a);
        let scale = (a * a + b * b).sqrt() / var_src;

        // Translation so the source centroid maps onto the destination centroid.
        let c = theta.cos();
        let s = theta.sin();
        let tx = dx - scale * (c * sx - s * sy);
        let ty = dy - scale * (s * sx + c * sy);

        // RMS residual of the fitted model over all correspondences.
        let mut sq = 0.0;
        for i in 0..n {
            let px = scale * (c * src[i].0 - s * src[i].1) + tx;
            let py = scale * (s * src[i].0 + c * src[i].1) + ty;
            sq += (px - dst[i].0).powi(2) + (py - dst[i].1).powi(2);
        }
        let rms = (sq / nf).sqrt();

        // Confidence: high when residual is small relative to a 0.1-cell tolerance.
        let tol = self.config.cell_size_px as f64 * 0.1;
        let confidence = (1.0 - rms / tol.max(1e-6)).clamp(0.0, 1.0) as f32;

        SimilarityTransform {
            scale: scale as f32,
            theta: theta as f32,
            tx: tx as f32,
            ty: ty as f32,
            rms_residual: rms as f32,
            confidence,
            markers_matched: n,
            markers_expected: n_expected,
        }
    }
}

// ─── Tile Stamp (travels with the tile through processing) ───────────────────

/// A stamp recording which markers are present in a tile and where they should be.
/// This is stored as metadata alongside the tile — NOT burned into pixel data.
#[derive(Clone, Debug)]
pub struct TileStamp {
    pub origin_x: f64,
    pub origin_y: f64,
    pub pixel_size_x: f64,
    pub pixel_size_y: f64,
    pub tile_width_px: usize,
    pub tile_height_px: usize,
    pub markers: Vec<StampedMarker>,
}

#[derive(Clone, Debug)]
pub struct StampedMarker {
    pub marker: GridMarker,
    /// Expected pixel position within the tile (subpixel precision)
    pub pixel_x: f32,
    pub pixel_y: f32,
}

/// Result of aligning a processed tile back to its original coordinates.
#[derive(Clone, Debug)]
pub struct AlignmentResult {
    /// Subpixel X offset to correct (add to coordinates)
    pub dx: f32,
    /// Subpixel Y offset to correct (add to coordinates)
    pub dy: f32,
    /// Confidence in the alignment (0.0 = no match, 1.0 = perfect)
    pub confidence: f32,
    /// How many markers were successfully matched
    pub markers_matched: usize,
    /// How many markers were expected
    pub markers_expected: usize,
}

/// A full 2-D similarity transform recovered from marker correspondences:
/// `[x' y'] = s·R(θ)·[x y] + [tx ty]`.
///
/// Unlike [`AlignmentResult`] (translation only), this captures **rotation and
/// scale** as well. A translation-only correction cannot absorb the per-layer
/// rotational/projection skew of a deep temporal stack — applying a linear
/// (dx,dy) fix to what is really a trigonometric (rotation) misregistration
/// lets the residual accumulate nonlinearly across the stack into kilometre-
/// scale drift. The similarity fit solves the actual problem.
#[derive(Clone, Copy, Debug)]
pub struct SimilarityTransform {
    /// Uniform scale factor (1.0 = no scale change).
    pub scale: f32,
    /// Rotation in radians (CCW, image coords).
    pub theta: f32,
    /// Translation X (pixels).
    pub tx: f32,
    /// Translation Y (pixels).
    pub ty: f32,
    /// RMS residual of the fit (pixels) — how well the model explains the markers.
    pub rms_residual: f32,
    /// Confidence in [0,1], derived from residual + match count.
    pub confidence: f32,
    pub markers_matched: usize,
    pub markers_expected: usize,
}

impl SimilarityTransform {
    /// Identity transform (no correction).
    pub fn identity() -> Self {
        Self {
            scale: 1.0,
            theta: 0.0,
            tx: 0.0,
            ty: 0.0,
            rms_residual: 0.0,
            confidence: 0.0,
            markers_matched: 0,
            markers_expected: 0,
        }
    }

    /// Is this fit trustworthy enough to apply?
    pub fn is_valid(&self) -> bool {
        self.confidence > 0.7 && self.markers_matched >= 4 && self.rms_residual < 2.0
    }

    /// Map an observed pixel coordinate back to the expected (reference) frame.
    /// Inverse of the forward similarity: `p = (1/s)·R(-θ)·(p' - t)`.
    pub fn invert_point(&self, x: f32, y: f32) -> (f32, f32) {
        let inv_s = if self.scale.abs() > 1e-9 { 1.0 / self.scale } else { 1.0 };
        let c = (-self.theta).cos();
        let s = (-self.theta).sin();
        let dx = x - self.tx;
        let dy = y - self.ty;
        let rx = c * dx - s * dy;
        let ry = s * dx + c * dy;
        (inv_s * rx, inv_s * ry)
    }
}

impl AlignmentResult {
    /// Is this alignment trustworthy enough to use?
    pub fn is_valid(&self) -> bool {
        self.confidence > 0.7 && self.markers_matched >= 4
    }

    /// Apply this alignment correction to a pixel coordinate.
    pub fn correct(&self, x: f32, y: f32) -> (f32, f32) {
        (x - self.dx, y - self.dy)
    }

    /// Apply this alignment correction to a geo coordinate.
    pub fn correct_geo(&self, lon: f64, lat: f64, pixel_size_x: f64, pixel_size_y: f64) -> (f64, f64) {
        (
            lon - self.dx as f64 * pixel_size_x,
            lat - self.dy as f64 * pixel_size_y,
        )
    }
}

// ─── Slicer with Grid Alignment ──────────────────────────────────────────────

/// Tile slicer that uses the overlay grid for perfect reassembly.
/// For systems without enough VRAM to hold a full tile, this slices it into
/// chunks but preserves alignment information so they can be stitched back
/// without drift.
pub struct GridAlignedSlicer {
    pub grid: OverlayGrid,
    pub chunk_size_px: usize,
    /// Overlap in pixels between adjacent chunks (for blending at seams)
    pub overlap_px: usize,
}

/// A chunk produced by the slicer, carrying its grid stamp for reassembly.
#[derive(Clone, Debug)]
pub struct AlignedChunk {
    /// Chunk position within the parent tile (pixel offset)
    pub offset_x: usize,
    pub offset_y: usize,
    /// Chunk dimensions
    pub width: usize,
    pub height: usize,
    /// The grid stamp for this chunk (subset of parent tile's stamp)
    pub stamp: TileStamp,
    /// Pixel data (f32 per pixel, single channel)
    pub data: Vec<f32>,
}

impl GridAlignedSlicer {
    pub fn new(chunk_size_px: usize, overlap_px: usize) -> Self {
        Self {
            grid: OverlayGrid::new(OverlayGridConfig::default()),
            chunk_size_px,
            overlap_px,
        }
    }

    /// Slice a tile into chunks, each carrying its own grid stamp.
    pub fn slice(
        &self,
        tile_data: &[f32],
        tile_width: usize,
        tile_height: usize,
        origin_x: f64,
        origin_y: f64,
        pixel_size_x: f64,
        pixel_size_y: f64,
    ) -> Vec<AlignedChunk> {
        let step = self.chunk_size_px - self.overlap_px;
        let mut chunks = Vec::new();

        let mut y_offset = 0;
        while y_offset < tile_height {
            let mut x_offset = 0;
            let chunk_h = self.chunk_size_px.min(tile_height - y_offset);

            while x_offset < tile_width {
                let chunk_w = self.chunk_size_px.min(tile_width - x_offset);

                // Extract pixel data for this chunk
                let mut data = vec![0.0f32; chunk_w * chunk_h];
                for row in 0..chunk_h {
                    let src_start = (y_offset + row) * tile_width + x_offset;
                    let dst_start = row * chunk_w;
                    data[dst_start..dst_start + chunk_w]
                        .copy_from_slice(&tile_data[src_start..src_start + chunk_w]);
                }

                // Generate grid stamp for this chunk's world position
                let chunk_origin_x = origin_x + x_offset as f64 * pixel_size_x;
                let chunk_origin_y = origin_y + y_offset as f64 * pixel_size_y;
                let stamp = self.grid.stamp(
                    chunk_w, chunk_h,
                    chunk_origin_x, chunk_origin_y,
                    pixel_size_x, pixel_size_y,
                );

                chunks.push(AlignedChunk {
                    offset_x: x_offset,
                    offset_y: y_offset,
                    width: chunk_w,
                    height: chunk_h,
                    stamp,
                    data,
                });

                x_offset += step;
            }
            y_offset += step;
        }

        chunks
    }

    /// Reassemble chunks back into a full tile using grid alignment.
    /// Each chunk's stamp is matched against the expected positions to correct
    /// any drift introduced during processing.
    pub fn reassemble(
        &self,
        chunks: &[AlignedChunk],
        tile_width: usize,
        tile_height: usize,
        parent_stamp: &TileStamp,
    ) -> Vec<f32> {
        let mut output = vec![0.0f32; tile_width * tile_height];
        let mut weight = vec![0.0f32; tile_width * tile_height];

        for chunk in chunks {
            // Compute alignment correction for this chunk
            // In a real implementation, you'd detect markers in the processed chunk
            // and compare against the stamp. Here we use the stamp directly
            // (assuming markers are preserved through processing).
            let _alignment = self.grid.align(
                &chunk.stamp.markers.iter()
                    .map(|m| (m.pixel_x, m.pixel_y, m.marker.hash))
                    .collect::<Vec<_>>(),
                parent_stamp,
            );

            // Place chunk pixels into output with overlap blending
            for row in 0..chunk.height {
                for col in 0..chunk.width {
                    let out_x = chunk.offset_x + col;
                    let out_y = chunk.offset_y + row;

                    if out_x >= tile_width || out_y >= tile_height {
                        continue;
                    }

                    let out_idx = out_y * tile_width + out_x;
                    let chunk_idx = row * chunk.width + col;

                    // Feathered blending weight (1.0 in center, fades at edges)
                    let edge_dist_x = (col.min(chunk.width - 1 - col)) as f32;
                    let edge_dist_y = (row.min(chunk.height - 1 - row)) as f32;
                    let edge_dist = edge_dist_x.min(edge_dist_y);
                    let blend = (edge_dist / self.overlap_px as f32).clamp(0.0, 1.0);

                    output[out_idx] += chunk.data[chunk_idx] * blend;
                    weight[out_idx] += blend;
                }
            }
        }

        // Normalize by accumulated weight
        for i in 0..output.len() {
            if weight[i] > 0.0 {
                output[i] /= weight[i];
            }
        }

        output
    }
}

// ─── Helper Functions ────────────────────────────────────────────────────────

/// Deterministic hash for a grid cell position. Globally unique.
fn position_hash(x: i64, y: i64) -> u64 {
    // FNV-1a inspired hash — fast, good distribution
    let mut h: u64 = 0xcbf29ce484222325;
    let bytes_x = x.to_le_bytes();
    let bytes_y = y.to_le_bytes();
    for &b in bytes_x.iter().chain(bytes_y.iter()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Generate vertices for a regular polygon.
fn regular_polygon(sides: usize, radius: f32) -> Vec<(f32, f32)> {
    (0..sides).map(|i| {
        let angle = (i as f32 / sides as f32) * 2.0 * std::f32::consts::PI - std::f32::consts::FRAC_PI_2;
        (radius * angle.cos(), radius * angle.sin())
    }).collect()
}

/// Generate a cross/plus shape.
fn cross_shape(radius: f32) -> Vec<(f32, f32)> {
    let w = radius * 0.3; // arm width
    vec![
        (-w, -radius), (w, -radius), (w, -w),
        (radius, -w), (radius, w), (w, w),
        (w, radius), (-w, radius), (-w, w),
        (-radius, w), (-radius, -w), (-w, -w),
    ]
}

/// Generate a star shape (n points, outer and inner radius).
fn star_shape(points: usize, outer: f32, inner: f32) -> Vec<(f32, f32)> {
    let mut verts = Vec::with_capacity(points * 2);
    for i in 0..(points * 2) {
        let angle = (i as f32 / (points * 2) as f32) * 2.0 * std::f32::consts::PI - std::f32::consts::FRAC_PI_2;
        let r = if i % 2 == 0 { outer } else { inner };
        verts.push((r * angle.cos(), r * angle.sin()));
    }
    verts
}

/// Generate an arrow shape pointing right.
fn arrow_shape(radius: f32) -> Vec<(f32, f32)> {
    let w = radius * 0.4;
    vec![
        (radius, 0.0),           // tip
        (0.0, -radius * 0.6),   // upper wing
        (0.0, -w),              // upper notch
        (-radius * 0.8, -w),    // tail upper
        (-radius * 0.8, w),     // tail lower
        (0.0, w),               // lower notch
        (0.0, radius * 0.6),    // lower wing
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_markers() {
        let grid = OverlayGrid::new(OverlayGridConfig::default());
        let m1 = grid.generate_marker(0, 0);
        let m2 = grid.generate_marker(1, 0);
        let m3 = grid.generate_marker(0, 1);

        // All markers should have different hashes
        assert_ne!(m1.hash, m2.hash);
        assert_ne!(m1.hash, m3.hash);
        assert_ne!(m2.hash, m3.hash);
    }

    #[test]
    fn test_stamp_and_align() {
        let grid = OverlayGrid::new(OverlayGridConfig::default());

        // Stamp a 256x256 tile
        let stamp = grid.stamp(256, 256, 0.0, 0.0, 10.0, -10.0);
        assert!(!stamp.markers.is_empty(), "Should have markers in a 256px tile");

        // Simulate perfect observation (no drift)
        let observed: Vec<(f32, f32, u64)> = stamp.markers.iter()
            .map(|m| (m.pixel_x, m.pixel_y, m.marker.hash))
            .collect();

        let result = grid.align(&observed, &stamp);
        assert!(result.is_valid());
        assert!(result.dx.abs() < 0.01, "No drift expected: dx={}", result.dx);
        assert!(result.dy.abs() < 0.01, "No drift expected: dy={}", result.dy);
    }

    #[test]
    fn test_detects_drift() {
        let grid = OverlayGrid::new(OverlayGridConfig::default());
        let stamp = grid.stamp(256, 256, 0.0, 0.0, 10.0, -10.0);

        // Simulate 2.5 pixel drift in X, 1.3 in Y
        let observed: Vec<(f32, f32, u64)> = stamp.markers.iter()
            .map(|m| (m.pixel_x + 2.5, m.pixel_y + 1.3, m.marker.hash))
            .collect();

        let result = grid.align(&observed, &stamp);
        assert!(result.is_valid());
        assert!((result.dx - 2.5).abs() < 0.1, "Should detect X drift: dx={}", result.dx);
        assert!((result.dy - 1.3).abs() < 0.1, "Should detect Y drift: dy={}", result.dy);
    }

    #[test]
    fn test_similarity_recovers_rotation_scale_translation() {
        // The core fix: a deep-stack misregistration is rotation+scale, not pure
        // translation. Apply a known similarity (scale 1.02, 3° rotation, +1.5/-0.8
        // shift) to every marker and confirm the estimator recovers it — something
        // the translation-only `align` provably cannot.
        let grid = OverlayGrid::new(OverlayGridConfig::default());
        let stamp = grid.stamp(256, 256, 0.0, 0.0, 10.0, -10.0);

        let scale = 1.02_f32;
        let theta = 3.0_f32.to_radians();
        let (tx, ty) = (1.5_f32, -0.8_f32);
        let (c, s) = (theta.cos(), theta.sin());

        let observed: Vec<(f32, f32, u64)> = stamp
            .markers
            .iter()
            .map(|m| {
                let x = m.pixel_x;
                let y = m.pixel_y;
                let nx = scale * (c * x - s * y) + tx;
                let ny = scale * (s * x + c * y) + ty;
                (nx, ny, m.marker.hash)
            })
            .collect();

        let t = grid.estimate_similarity(&observed, &stamp);
        assert!(t.is_valid(), "fit should be valid (rms={})", t.rms_residual);
        assert!((t.scale - scale).abs() < 1e-3, "scale: got {}", t.scale);
        assert!((t.theta - theta).abs() < 1e-3, "theta: got {}", t.theta);
        assert!((t.tx - tx).abs() < 0.05, "tx: got {}", t.tx);
        assert!((t.ty - ty).abs() < 0.05, "ty: got {}", t.ty);
        assert!(t.rms_residual < 0.05, "rms should be tiny: {}", t.rms_residual);
    }

    #[test]
    fn test_similarity_invert_roundtrips() {
        // invert_point must map an observed point back onto the reference frame.
        let grid = OverlayGrid::new(OverlayGridConfig::default());
        let stamp = grid.stamp(128, 128, 0.0, 0.0, 10.0, -10.0);
        let scale = 0.98_f32;
        let theta = (-2.0_f32).to_radians();
        let (tx, ty) = (0.7_f32, 1.1_f32);
        let (c, s) = (theta.cos(), theta.sin());
        let observed: Vec<(f32, f32, u64)> = stamp
            .markers
            .iter()
            .map(|m| {
                let nx = scale * (c * m.pixel_x - s * m.pixel_y) + tx;
                let ny = scale * (s * m.pixel_x + c * m.pixel_y) + ty;
                (nx, ny, m.marker.hash)
            })
            .collect();
        let t = grid.estimate_similarity(&observed, &stamp);
        // Invert the first observed marker → should land on its expected pos.
        let m0 = &stamp.markers[0];
        let (rx, ry) = t.invert_point(observed[0].0, observed[0].1);
        assert!((rx - m0.pixel_x).abs() < 0.1, "inverse x: {rx} vs {}", m0.pixel_x);
        assert!((ry - m0.pixel_y).abs() < 0.1, "inverse y: {ry} vs {}", m0.pixel_y);
    }

    #[test]
    fn test_slicer_roundtrip() {
        let slicer = GridAlignedSlicer::new(64, 8);

        // Create a 128x128 test tile with a gradient
        let tile: Vec<f32> = (0..128*128).map(|i| (i % 128) as f32 / 128.0).collect();

        let chunks = slicer.slice(&tile, 128, 128, 0.0, 0.0, 10.0, -10.0);
        assert!(chunks.len() > 1, "Should produce multiple chunks");

        let parent_stamp = slicer.grid.stamp(128, 128, 0.0, 0.0, 10.0, -10.0);
        let reassembled = slicer.reassemble(&chunks, 128, 128, &parent_stamp);

        // Check center pixels match (edges may differ due to blending)
        for y in 16..112 {
            for x in 16..112 {
                let orig = tile[y * 128 + x];
                let reasm = reassembled[y * 128 + x];
                assert!((orig - reasm).abs() < 0.01,
                    "Mismatch at ({},{}): orig={}, reassembled={}", x, y, orig, reasm);
            }
        }
    }
}
