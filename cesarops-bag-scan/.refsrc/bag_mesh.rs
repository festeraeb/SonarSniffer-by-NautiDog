//! BAG Uncertainty → 3D Mesh builder
//!
//! # Problem
//!
//! NOAA redacts BAG files by flattening the elevation channel over wrecks, but the
//! **uncertainty channel survives intact**.  The uncertainty field encodes the sonar's
//! measurement confidence — which is physically shaped by the object the multibeam
//! hit.  A protruding wreck creates higher uncertainty at its edges (beam geometry
//! changes, more shadow, worse grazing angle) and lower uncertainty at nadir.
//!
//! The redaction mask also produces **edge artifacts**: the interpolation boundary
//! between real seabed and flattened zone creates a ring of artificially low
//! uncertainty that is NOT from the object — it's an artifact of how the mask was
//! drawn.  We know this because we considered using the outer ring to trace the
//! bounding box, and discovered it's a masking artifact.
//!
//! # Approach
//!
//! 1. Read the uncertainty grid from BAG HDF5 (`BAG_root/uncertainty`)
//! 2. Detect masked regions: connected clusters where uncertainty is anomalously
//!    low compared to the survey-wide distribution (below p5)
//! 3. **Erode edges**: binary erosion strips the outer boundary artifact pixels,
//!    keeping only the interior where uncertainty reflects the real object
//! 4. Use uncertainty gradient magnitude as a height proxy: high gradient = physical
//!    edge of the wreck, low gradient = flat deck or seabed
//! 5. Build a triangle mesh from the interior uncertainty heightfield
//! 6. Multiple overlapping survey lines at different beam incidence angles give
//!    us different "views" of the object — the CUBE algorithm in the BAG already
//!    fused these, so each cell's uncertainty encodes information from all angles
//! 7. Export as OBJ (simple) and glTF (web-ready, rotatable)
//!
//! # Equipment context (from H13255 Descriptive Report)
//!
//! | System           | Beams | Swath        | Beam width | Frequency   |
//! |------------------|-------|--------------|------------|-------------|
//! | R2Sonic 2022     | 256   | 10°–160°     | 0.5°×1°    | 200/400 kHz |
//! | R2Sonic 2024     | 256   | 10°–170°     | 0.5°×1°    | 200/400 kHz |
//! | Kongsberg EM2040C| 800   | 0.5°–140°    | 1°×1°      | 200/400 kHz |
//!
//! With 200–800 beams per ping and overlapping swaths, each cell in the 0.5 m
//! CUBE surface was observed from many incidence angles.  The uncertainty at each
//! pixel is the statistical combination of all those observations.

use ndarray::{s, Array2, Zip};
use std::path::Path;
use thiserror::Error;
use tracing::{info, warn};

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(thiserror::Error, Debug)]
pub enum BagMeshError {
    #[error("No valid uncertainty data in BAG file")]
    NoData,
    #[error("No masked regions found (nothing to reconstruct)")]
    NoMasks,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Survey metadata ───────────────────────────────────────────────────────────

/// Sonar system parameters extracted from BAG metadata or DR.
#[derive(Debug, Clone)]
pub struct SonarParams {
    /// Grid cell size in meters (e.g. 0.5)
    pub cell_size_m: f64,
    /// Maximum swath angle from nadir in degrees (half-angle)
    pub max_swath_half_angle_deg: f64,
    /// Number of beams per ping
    pub num_beams: u32,
    /// Operating frequency in kHz
    pub frequency_khz: f64,
}

impl Default for SonarParams {
    /// Defaults based on R2Sonic 2024 (the most capable system on H13255)
    fn default() -> Self {
        Self {
            cell_size_m: 0.5,
            max_swath_half_angle_deg: 85.0, // 170° total swath / 2
            num_beams: 256,
            frequency_khz: 400.0,
        }
    }
}

// ── Data structures ───────────────────────────────────────────────────────────

/// A detected masked region with its uncertainty heightfield.
#[derive(Debug)]
pub struct MaskedObject {
    /// Region index
    pub id: usize,
    /// Centroid row in full grid
    pub centroid_row: usize,
    /// Centroid col in full grid
    pub centroid_col: usize,
    /// Bounding box in full grid: (row_start, col_start, row_end, col_end)
    pub bbox: (usize, usize, usize, usize),
    /// Interior-only uncertainty heightfield (edge artifacts removed).
    /// Values are normalised: 0 = background, 1 = peak uncertainty.
    /// NaN = outside the object.
    pub heightfield: Array2<f32>,
    /// Boolean mask: true = object pixel (after erosion)
    pub interior_mask: Array2<bool>,
    /// Number of edge pixels that were eroded
    pub eroded_edge_pixels: usize,
    /// Peak height (normalised)
    pub peak_height: f32,
    /// Area in square meters
    pub area_m2: f64,
    /// PCA heading in degrees from north (clockwise)
    pub heading_deg: f64,
    /// PCA length in meters
    pub length_m: f64,
    /// PCA width in meters
    pub width_m: f64,
}

/// A triangle mesh ready for export.
#[derive(Debug, Clone)]
pub struct TriMesh {
    /// Vertex positions: [x, y, z] in meters (object-centred coordinates)
    pub vertices: Vec<[f32; 3]>,
    /// Vertex normals
    pub normals: Vec<[f32; 3]>,
    /// Triangle indices (3 per triangle)
    pub indices: Vec<u32>,
    /// Per-vertex colour [r, g, b, a] in [0,1]
    pub colors: Vec<[f32; 4]>,
}

// ── BAG reader ────────────────────────────────────────────────────────────────

/// Read the uncertainty grid from a BAG file (HDF5).
///
/// BAG files store data under `BAG_root/uncertainty` as float32.
/// NoData is encoded as 1_000_000.0.

/// Parse the BAG XML metadata to extract cell resolution.

// ── Mask detection ────────────────────────────────────────────────────────────

const NODATA_THRESH: f32 = 999_000.0;

/// Detect masked regions in the uncertainty grid.
///
/// Masked (redacted) regions have anomalously low uncertainty compared to the
/// survey-wide distribution.  We find connected components below the 5th
/// percentile of valid uncertainty values.
pub fn detect_masked_regions(
    uncert: &Array2<f32>,
    cell_size_m: f64,
    min_area_m2: f64,
    max_area_m2: f64,
    erosion_px: usize,
) -> Vec<MaskedObject> {
    let (rows, cols) = uncert.dim();

    // Collect valid uncertainty values (not nodata)
    let valid: Vec<f32> = uncert
        .iter()
        .copied()
        .filter(|&v| v > 0.0 && v < NODATA_THRESH && v.is_finite())
        .collect();

    if valid.len() < 100 {
        warn!("Too few valid uncertainty pixels ({})", valid.len());
        return Vec::new();
    }

    // Compute 5th percentile as mask threshold
    let mut sorted = valid.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p5 = sorted[sorted.len() * 5 / 100];
    let p50 = sorted[sorted.len() / 2];
    info!(
        "Uncertainty stats: p5={:.4}, p50={:.4}, n_valid={}",
        p5,
        p50,
        valid.len()
    );

    // Build binary mask: pixels below p5 are "masked"
    let mut mask = Array2::<bool>::default((rows, cols));
    Zip::from(&mut mask).and(uncert).for_each(|m, &u| {
        *m = u > 0.0 && u < p5 && u < NODATA_THRESH && u.is_finite();
    });

    // Binary closing (dilate then erode) to fill small gaps
    mask = binary_dilate(&mask, 3);
    mask = binary_erode(&mask, 3);

    // Connected component labelling (flood fill)
    let labels = connected_components(&mask);
    let max_label = labels.iter().copied().max().unwrap_or(0);

    info!("Found {} connected components in mask", max_label);

    let cell_area = cell_size_m * cell_size_m;
    let mut objects = Vec::new();

    for label in 1..=max_label {
        // Collect pixels for this component
        let mut pixels: Vec<(usize, usize)> = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                if labels[[r, c]] == label {
                    pixels.push((r, c));
                }
            }
        }

        let area_m2 = pixels.len() as f64 * cell_area;
        if area_m2 < min_area_m2 || area_m2 > max_area_m2 {
            continue;
        }

        // Bounding box
        let r_min = pixels.iter().map(|p| p.0).min().unwrap();
        let r_max = pixels.iter().map(|p| p.0).max().unwrap();
        let c_min = pixels.iter().map(|p| p.1).min().unwrap();
        let c_max = pixels.iter().map(|p| p.1).max().unwrap();

        // Extract local patch with padding
        let pad = erosion_px + 5;
        let lr0 = r_min.saturating_sub(pad);
        let lr1 = (r_max + 1 + pad).min(rows);
        let lc0 = c_min.saturating_sub(pad);
        let lc1 = (c_max + 1 + pad).min(cols);

        let local_uncert = uncert.slice(s![lr0..lr1, lc0..lc1]).to_owned();

        // Local mask
        let mut local_mask = Array2::<bool>::default(local_uncert.dim());
        for &(r, c) in &pixels {
            let lr = r - lr0;
            let lc = c - lc0;
            if lr < local_mask.nrows() && lc < local_mask.ncols() {
                local_mask[[lr, lc]] = true;
            }
        }

        // ── EDGE EROSION: remove boundary artifact pixels ──
        let eroded = binary_erode(&local_mask, erosion_px);
        let edge_pixels = local_mask
            .iter()
            .zip(eroded.iter())
            .filter(|(&m, &e)| m && !e)
            .count();

        // If erosion killed everything, use original mask with smaller erosion
        let interior = if eroded.iter().filter(|&&v| v).count() < 5 {
            if erosion_px > 1 {
                binary_erode(&local_mask, 1)
            } else {
                local_mask.clone()
            }
        } else {
            eroded
        };

        let interior_count = interior.iter().filter(|&&v| v).count();
        if interior_count < 5 {
            continue;
        }

        // ── BUILD HEIGHTFIELD from uncertainty gradient magnitude ──
        // The uncertainty gradient captures where the sonar's measurement
        // confidence changes rapidly — i.e. the physical edges and protrusions
        // of the masked object.
        let heightfield = build_heightfield(&local_uncert, &interior);

        let peak = heightfield
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold(0.0f32, f32::max);

        // PCA for heading, length, width
        let (heading, length_px, width_px) = pca_axis(&interior, cell_size_m);

        let centroid_row = pixels.iter().map(|p| p.0).sum::<usize>() / pixels.len();
        let centroid_col = pixels.iter().map(|p| p.1).sum::<usize>() / pixels.len();

        objects.push(MaskedObject {
            id: objects.len(),
            centroid_row,
            centroid_col,
            bbox: (r_min, c_min, r_max, c_max),
            heightfield,
            interior_mask: interior,
            eroded_edge_pixels: edge_pixels,
            peak_height: peak,
            area_m2,
            heading_deg: heading,
            length_m: length_px,
            width_m: width_px,
        });
    }

    // Sort by area descending
    objects.sort_by(|a, b| b.area_m2.partial_cmp(&a.area_m2).unwrap());
    info!("Detected {} wreck candidates after filtering", objects.len());
    objects
}

// ── Heightfield construction ──────────────────────────────────────────────────

/// Build a normalised heightfield from the uncertainty gradient magnitude.
///
/// Inside the masked region, the uncertainty values themselves encode the object's
/// shape because the multibeam sonar measured each pixel from multiple angles.
/// We use the gradient magnitude to highlight physical edges, combined with the
/// raw uncertainty deviation from the local median to capture the overall relief.
fn build_heightfield(uncert: &Array2<f32>, interior: &Array2<bool>) -> Array2<f32> {
    let (rows, cols) = uncert.dim();
    let mut height = Array2::<f32>::from_elem((rows, cols), f32::NAN);

    // Compute uncertainty stats inside the interior only
    let interior_vals: Vec<f32> = interior
        .iter()
        .zip(uncert.iter())
        .filter(|(&m, &u)| m && u > 0.0 && u < NODATA_THRESH && u.is_finite())
        .map(|(_, &u)| u)
        .collect();

    if interior_vals.is_empty() {
        return height;
    }

    let _mean_u: f32 = interior_vals.iter().sum::<f32>() / interior_vals.len() as f32;
    let max_u: f32 = interior_vals
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    let min_u: f32 = interior_vals
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min);
    let range_u = (max_u - min_u).max(1e-6);

    // Compute gradient magnitude (Sobel-like)
    let grad = gradient_magnitude(uncert);

    // Interior gradient stats
    let grad_vals: Vec<f32> = interior
        .iter()
        .zip(grad.iter())
        .filter(|(&m, &g)| m && g.is_finite())
        .map(|(_, &g)| g)
        .collect();

    let max_grad = grad_vals
        .iter()
        .copied()
        .fold(0.0f32, f32::max)
        .max(1e-6);

    // Combine: heightfield = 0.6 * normalised_uncertainty + 0.4 * normalised_gradient
    // This gives the overall shape from uncertainty magnitude plus edge detail
    // from the gradient.
    for r in 0..rows {
        for c in 0..cols {
            if interior[[r, c]] {
                let u = uncert[[r, c]];
                if u > 0.0 && u < NODATA_THRESH && u.is_finite() {
                    // Invert: lower uncertainty = the object is there (sonar had
                    // MORE confidence on the hard surface, LESS on shadow edges)
                    let u_norm = 1.0 - (u - min_u) / range_u;
                    let g_norm = grad[[r, c]] / max_grad;
                    height[[r, c]] = 0.6 * u_norm + 0.4 * g_norm;
                }
            }
        }
    }

    // Gaussian smooth to connect nearby pixels (sigma ~1.5 cells)
    gaussian_smooth_masked(&mut height, interior, 1.5);

    height
}

// ── Mesh generation ───────────────────────────────────────────────────────────

/// Convert a heightfield into a triangle mesh.
///
/// Each interior pixel becomes a vertex at (x, y, z) where x/y are in meters
/// (object-centred) and z is the heightfield value scaled by `z_scale`.
/// Adjacent pixels are connected into triangles.  A thin "skirt" at z=0 is
/// added around the object boundary to give it a visible base.
pub fn heightfield_to_mesh(
    obj: &MaskedObject,
    cell_size_m: f64,
    z_scale: f32,
) -> TriMesh {
    let (rows, cols) = obj.heightfield.dim();
    let cs = cell_size_m as f32;

    // Centre coordinates
    let x_off = cols as f32 * cs / 2.0;
    let y_off = rows as f32 * cs / 2.0;

    // Build vertex index map: pixel (r,c) → vertex index (or u32::MAX if none)
    let mut vert_idx = Array2::<u32>::from_elem((rows, cols), u32::MAX);
    let mut vertices: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();

    // Build the "skirt" mask: 1-pixel dilation of interior minus interior
    let dilated = binary_dilate(&obj.interior_mask, 2);

    for r in 0..rows {
        for c in 0..cols {
            let is_interior = obj.interior_mask[[r, c]];
            let is_skirt = dilated[[r, c]] && !is_interior;

            if is_interior || is_skirt {
                let x = c as f32 * cs - x_off;
                let y = (rows - 1 - r) as f32 * cs - y_off; // flip Y so north = +Y
                let z = if is_interior {
                    let h = obj.heightfield[[r, c]];
                    if h.is_finite() { h * z_scale } else { 0.0 }
                } else {
                    0.0 // skirt base
                };

                vert_idx[[r, c]] = vertices.len() as u32;
                vertices.push([x, y, z]);

                // Colour: plasma-like colourmap based on height
                let t = if z_scale > 0.0 { (z / z_scale).clamp(0.0, 1.0) } else { 0.0 };
                colors.push(plasma_color(t));
            }
        }
    }

    // Build triangles: for each 2×2 quad where all 4 vertices exist, emit 2 tris
    let mut indices: Vec<u32> = Vec::new();
    for r in 0..rows.saturating_sub(1) {
        for c in 0..cols.saturating_sub(1) {
            let v00 = vert_idx[[r, c]];
            let v01 = vert_idx[[r, c + 1]];
            let v10 = vert_idx[[r + 1, c]];
            let v11 = vert_idx[[r + 1, c + 1]];

            if v00 != u32::MAX && v01 != u32::MAX && v10 != u32::MAX && v11 != u32::MAX {
                // Triangle 1: v00, v10, v01
                indices.push(v00);
                indices.push(v10);
                indices.push(v01);
                // Triangle 2: v01, v10, v11
                indices.push(v01);
                indices.push(v10);
                indices.push(v11);
            }
        }
    }

    // Compute normals
    let normals = compute_normals(&vertices, &indices);

    TriMesh {
        vertices,
        normals,
        indices,
        colors,
    }
}

// ── OBJ export ────────────────────────────────────────────────────────────────

/// Write a triangle mesh to Wavefront OBJ format.
pub fn write_obj(mesh: &TriMesh, path: &Path) -> Result<(), BagMeshError> {
    use std::io::Write;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);

    writeln!(f, "# BAG Uncertainty 3D Mesh — generated by erie_remote::bag_mesh")?;
    writeln!(f, "# Vertices: {}  Triangles: {}", mesh.vertices.len(), mesh.indices.len() / 3)?;

    // Vertices with colour (OBJ extension: v x y z r g b)
    for (v, col) in mesh.vertices.iter().zip(mesh.colors.iter()) {
        writeln!(f, "v {:.4} {:.4} {:.4} {:.3} {:.3} {:.3}",
            v[0], v[1], v[2], col[0], col[1], col[2])?;
    }

    // Normals
    for n in &mesh.normals {
        writeln!(f, "vn {:.4} {:.4} {:.4}", n[0], n[1], n[2])?;
    }

    // Faces (1-indexed)
    for tri in mesh.indices.chunks(3) {
        writeln!(
            f,
            "f {}//{} {}//{} {}//{}",
            tri[0] + 1, tri[0] + 1,
            tri[1] + 1, tri[1] + 1,
            tri[2] + 1, tri[2] + 1,
        )?;
    }

    Ok(())
}

// ── glTF export ───────────────────────────────────────────────────────────────

/// Write a triangle mesh to glTF 2.0 binary (.glb) format.
///
/// This produces a self-contained .glb file that can be opened in any 3D viewer
/// or embedded in a web page with three.js / model-viewer for interactive rotation.
pub fn write_glb(mesh: &TriMesh, path: &Path) -> Result<(), BagMeshError> {
    use std::io::Write;

    // ── Build binary buffer ──
    let n_verts = mesh.vertices.len();
    let n_tris = mesh.indices.len() / 3;

    // Buffer layout: [positions] [normals] [colors] [indices]
    let pos_bytes = n_verts * 12; // 3 × f32
    let norm_bytes = n_verts * 12;
    let color_bytes = n_verts * 16; // 4 × f32
    let idx_bytes = mesh.indices.len() * 4; // u32
    let total_bytes = pos_bytes + norm_bytes + color_bytes + idx_bytes;

    let mut bin = Vec::with_capacity(total_bytes);

    // Positions
    let mut pos_min = [f32::INFINITY; 3];
    let mut pos_max = [f32::NEG_INFINITY; 3];
    for v in &mesh.vertices {
        for i in 0..3 {
            pos_min[i] = pos_min[i].min(v[i]);
            pos_max[i] = pos_max[i].max(v[i]);
            bin.extend_from_slice(&v[i].to_le_bytes());
        }
    }

    // Normals
    for n in &mesh.normals {
        for i in 0..3 {
            bin.extend_from_slice(&n[i].to_le_bytes());
        }
    }

    // Colours
    for col in &mesh.colors {
        for i in 0..4 {
            bin.extend_from_slice(&col[i].to_le_bytes());
        }
    }

    // Indices
    let idx_max = mesh.indices.iter().copied().max().unwrap_or(0);
    for &idx in &mesh.indices {
        bin.extend_from_slice(&idx.to_le_bytes());
    }

    // Pad binary buffer to 4-byte alignment
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    // ── Build JSON chunk ──
    let json = build_gltf_json(
        n_verts, n_tris, &pos_min, &pos_max,
        pos_bytes, norm_bytes, color_bytes, idx_bytes,
        mesh.indices.len(), idx_max, bin.len(),
    );

    // Pad JSON to 4-byte alignment
    let mut json_bytes = json.into_bytes();
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }

    // ── Write GLB ──
    let total_len = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);

    // GLB header
    out.write_all(b"glTF")?;                          // magic
    out.write_all(&2u32.to_le_bytes())?;               // version
    out.write_all(&(total_len as u32).to_le_bytes())?; // total length

    // JSON chunk
    out.write_all(&(json_bytes.len() as u32).to_le_bytes())?;
    out.write_all(&0x4E4F534Au32.to_le_bytes())?; // "JSON"
    out.write_all(&json_bytes)?;

    // BIN chunk
    out.write_all(&(bin.len() as u32).to_le_bytes())?;
    out.write_all(&0x004E4942u32.to_le_bytes())?; // "BIN\0"
    out.write_all(&bin)?;

    info!(
        "Wrote glb: {} verts, {} tris, {} bytes",
        n_verts, n_tris, total_len
    );
    Ok(())
}

// ── Pipeline entry point ──────────────────────────────────────────────────────

/// Full pipeline: BAG file → detected objects → OBJ + glTF meshes.
///
/// Returns the number of objects exported.

// ══════════════════════════════════════════════════════════════════════════════
// Internal helpers
// ══════════════════════════════════════════════════════════════════════════════

/// Binary erosion: shrink true regions by `iterations` pixels.
fn binary_erode(mask: &Array2<bool>, iterations: usize) -> Array2<bool> {
    let mut current = mask.clone();
    let (rows, cols) = current.dim();

    for _ in 0..iterations {
        let prev = current.clone();
        for r in 0..rows {
            for c in 0..cols {
                if prev[[r, c]] {
                    // Check 4-connected neighbours
                    let keep = (r > 0 && prev[[r - 1, c]])
                        && (r + 1 < rows && prev[[r + 1, c]])
                        && (c > 0 && prev[[r, c - 1]])
                        && (c + 1 < cols && prev[[r, c + 1]]);
                    current[[r, c]] = keep;
                }
            }
        }
    }
    current
}

/// Binary dilation: grow true regions by `iterations` pixels.
fn binary_dilate(mask: &Array2<bool>, iterations: usize) -> Array2<bool> {
    let mut current = mask.clone();
    let (rows, cols) = current.dim();

    for _ in 0..iterations {
        let prev = current.clone();
        for r in 0..rows {
            for c in 0..cols {
                if !prev[[r, c]] {
                    let grow = (r > 0 && prev[[r - 1, c]])
                        || (r + 1 < rows && prev[[r + 1, c]])
                        || (c > 0 && prev[[r, c - 1]])
                        || (c + 1 < cols && prev[[r, c + 1]]);
                    if grow {
                        current[[r, c]] = true;
                    }
                }
            }
        }
    }
    current
}

/// Connected component labelling via flood fill.
fn connected_components(mask: &Array2<bool>) -> Array2<u32> {
    let (rows, cols) = mask.dim();
    let mut labels = Array2::<u32>::zeros((rows, cols));
    let mut current_label = 0u32;

    for r in 0..rows {
        for c in 0..cols {
            if mask[[r, c]] && labels[[r, c]] == 0 {
                current_label += 1;
                // BFS flood fill
                let mut queue = std::collections::VecDeque::new();
                queue.push_back((r, c));
                labels[[r, c]] = current_label;

                while let Some((qr, qc)) = queue.pop_front() {
                    for (dr, dc) in &[(-1i32, 0), (1, 0), (0, -1i32), (0, 1),
                                       (-1, -1), (-1, 1), (1, -1), (1, 1)] {
                        let nr = qr as i32 + dr;
                        let nc = qc as i32 + dc;
                        if nr >= 0 && nr < rows as i32 && nc >= 0 && nc < cols as i32 {
                            let nr = nr as usize;
                            let nc = nc as usize;
                            if mask[[nr, nc]] && labels[[nr, nc]] == 0 {
                                labels[[nr, nc]] = current_label;
                                queue.push_back((nr, nc));
                            }
                        }
                    }
                }
            }
        }
    }
    labels
}

/// Gradient magnitude (central differences).
fn gradient_magnitude(grid: &Array2<f32>) -> Array2<f32> {
    let (rows, cols) = grid.dim();
    let mut grad = Array2::<f32>::zeros((rows, cols));

    for r in 1..rows - 1 {
        for c in 1..cols - 1 {
            let gy = grid[[r + 1, c]] - grid[[r - 1, c]];
            let gx = grid[[r, c + 1]] - grid[[r, c - 1]];
            if gy.is_finite() && gx.is_finite() {
                grad[[r, c]] = (gy * gy + gx * gx).sqrt();
            }
        }
    }
    grad
}

/// Gaussian smooth a heightfield, respecting a mask (only smooth within mask).
fn gaussian_smooth_masked(height: &mut Array2<f32>, mask: &Array2<bool>, sigma: f32) {
    let (rows, cols) = height.dim();
    let radius = (sigma * 3.0).ceil() as i32;
    let original = height.clone();

    for r in 0..rows {
        for c in 0..cols {
            if !mask[[r, c]] {
                continue;
            }
            let mut sum = 0.0f32;
            let mut weight = 0.0f32;

            for dr in -radius..=radius {
                for dc in -radius..=radius {
                    let nr = r as i32 + dr;
                    let nc = c as i32 + dc;
                    if nr >= 0 && nr < rows as i32 && nc >= 0 && nc < cols as i32 {
                        let nr = nr as usize;
                        let nc = nc as usize;
                        if mask[[nr, nc]] {
                            let v = original[[nr, nc]];
                            if v.is_finite() {
                                let d2 = (dr * dr + dc * dc) as f32;
                                let w = (-d2 / (2.0 * sigma * sigma)).exp();
                                sum += v * w;
                                weight += w;
                            }
                        }
                    }
                }
            }

            if weight > 0.0 {
                height[[r, c]] = sum / weight;
            }
        }
    }
}

/// PCA-based principal axis: heading (deg from north CW), length (m), width (m).
fn pca_axis(mask: &Array2<bool>, cell_size: f64) -> (f64, f64, f64) {
    let coords: Vec<(f64, f64)> = mask
        .indexed_iter()
        .filter(|(_, &v)| v)
        .map(|((r, c), _)| (r as f64, c as f64))
        .collect();

    if coords.len() < 3 {
        return (0.0, 0.0, 0.0);
    }

    let n = coords.len() as f64;
    let mean_r: f64 = coords.iter().map(|c| c.0).sum::<f64>() / n;
    let mean_c: f64 = coords.iter().map(|c| c.1).sum::<f64>() / n;

    // 2×2 covariance matrix
    let mut cov_rr = 0.0;
    let mut cov_rc = 0.0;
    let mut cov_cc = 0.0;
    for &(r, c) in &coords {
        let dr = r - mean_r;
        let dc = c - mean_c;
        cov_rr += dr * dr;
        cov_rc += dr * dc;
        cov_cc += dc * dc;
    }
    cov_rr /= n;
    cov_rc /= n;
    cov_cc /= n;

    // Eigenvalues of [[cov_rr, cov_rc], [cov_rc, cov_cc]]
    let trace = cov_rr + cov_cc;
    let det = cov_rr * cov_cc - cov_rc * cov_rc;
    let disc = (trace * trace / 4.0 - det).max(0.0).sqrt();
    let lambda1 = trace / 2.0 + disc; // larger
    let _lambda2 = trace / 2.0 - disc; // smaller

    // Principal eigenvector
    let (er, ec) = if cov_rc.abs() > 1e-10 {
        (lambda1 - cov_cc, cov_rc)
    } else if cov_rr >= cov_cc {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    };

    let heading = ec.atan2(er).to_degrees().rem_euclid(360.0);

    // Project onto axes for length/width
    let norm = (er * er + ec * ec).sqrt().max(1e-10);
    let (pr, pc) = (er / norm, ec / norm);
    let (mr, mc) = (-pc, pr); // minor axis

    let mut min_p = f64::INFINITY;
    let mut max_p = f64::NEG_INFINITY;
    let mut min_m = f64::INFINITY;
    let mut max_m = f64::NEG_INFINITY;

    for &(r, c) in &coords {
        let dr = r - mean_r;
        let dc = c - mean_c;
        let proj_p = dr * pr + dc * pc;
        let proj_m = dr * mr + dc * mc;
        min_p = min_p.min(proj_p);
        max_p = max_p.max(proj_p);
        min_m = min_m.min(proj_m);
        max_m = max_m.max(proj_m);
    }

    let length = (max_p - min_p) * cell_size;
    let width = (max_m - min_m) * cell_size;

    (heading, length, width)
}

/// Compute per-vertex normals from triangle mesh.
fn compute_normals(vertices: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0f32; 3]; vertices.len()];

    for tri in indices.chunks(3) {
        if tri.len() < 3 { continue; }
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let v0 = vertices[i0];
        let v1 = vertices[i1];
        let v2 = vertices[i2];

        let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];

        for &i in &[i0, i1, i2] {
            normals[i][0] += n[0];
            normals[i][1] += n[1];
            normals[i][2] += n[2];
        }
    }

    // Normalise
    for n in &mut normals {
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len > 1e-10 {
            n[0] /= len;
            n[1] /= len;
            n[2] /= len;
        } else {
            *n = [0.0, 0.0, 1.0];
        }
    }
    normals
}

/// Plasma colourmap approximation: t in [0, 1] → [r, g, b, a].
fn plasma_color(t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    // Simplified plasma: dark purple → blue → teal → yellow
    let r = (1.5 * t - 0.15).clamp(0.0, 1.0);
    let g = if t < 0.5 {
        (0.8 * t).clamp(0.0, 1.0)
    } else {
        (1.6 * t - 0.4).clamp(0.0, 1.0)
    };
    let b = if t < 0.4 {
        (0.5 + 1.2 * t).clamp(0.0, 1.0)
    } else {
        (1.1 - 1.5 * (t - 0.4)).clamp(0.0, 1.0)
    };
    [r, g, b, 1.0]
}

/// Build a glTF 2.0 JSON string for a single mesh with positions, normals, colours, and indices.
#[allow(clippy::too_many_arguments)]
fn build_gltf_json(
    n_verts: usize,
    _n_tris: usize,
    pos_min: &[f32; 3],
    pos_max: &[f32; 3],
    pos_bytes: usize,
    norm_bytes: usize,
    color_bytes: usize,
    idx_bytes: usize,
    n_indices: usize,
    idx_max: u32,
    buf_len: usize,
) -> String {
    let norm_off = pos_bytes;
    let color_off = pos_bytes + norm_bytes;
    let idx_off = pos_bytes + norm_bytes + color_bytes;

    let mut j = String::with_capacity(1024);
    j.push_str("{\"asset\":{\"version\":\"2.0\",\"generator\":\"erie_remote::bag_mesh\"},");
    j.push_str("\"scene\":0,\"scenes\":[{\"nodes\":[0]}],\"nodes\":[{\"mesh\":0}],");
    j.push_str("\"meshes\":[{\"primitives\":[{\"attributes\":{\"POSITION\":0,\"NORMAL\":1,\"COLOR_0\":2},\"indices\":3,\"mode\":4}]}],");

    // Accessors
    j.push_str("\"accessors\":[");
    // 0: POSITION
    j.push_str(&format!(
        "{{\"bufferView\":0,\"componentType\":5126,\"count\":{},\"type\":\"VEC3\",\"min\":[{},{},{}],\"max\":[{},{},{}]}},",
        n_verts, pos_min[0], pos_min[1], pos_min[2], pos_max[0], pos_max[1], pos_max[2]
    ));
    // 1: NORMAL
    j.push_str(&format!(
        "{{\"bufferView\":1,\"componentType\":5126,\"count\":{},\"type\":\"VEC3\"}},",
        n_verts
    ));
    // 2: COLOR_0
    j.push_str(&format!(
        "{{\"bufferView\":2,\"componentType\":5126,\"count\":{},\"type\":\"VEC4\"}},",
        n_verts
    ));
    // 3: indices
    j.push_str(&format!(
        "{{\"bufferView\":3,\"componentType\":5125,\"count\":{},\"type\":\"SCALAR\",\"max\":[{}]}}",
        n_indices, idx_max
    ));
    j.push_str("],");

    // BufferViews
    j.push_str("\"bufferViews\":[");
    j.push_str(&format!(
        "{{\"buffer\":0,\"byteOffset\":0,\"byteLength\":{},\"target\":34962}},",
        pos_bytes
    ));
    j.push_str(&format!(
        "{{\"buffer\":0,\"byteOffset\":{},\"byteLength\":{},\"target\":34962}},",
        norm_off, norm_bytes
    ));
    j.push_str(&format!(
        "{{\"buffer\":0,\"byteOffset\":{},\"byteLength\":{},\"target\":34962}},",
        color_off, color_bytes
    ));
    j.push_str(&format!(
        "{{\"buffer\":0,\"byteOffset\":{},\"byteLength\":{},\"target\":34963}}",
        idx_off, idx_bytes
    ));
    j.push_str("],");

    // Buffer
    j.push_str(&format!("\"buffers\":[{{\"byteLength\":{}}}]}}", buf_len));

    j
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_erode() {
        let mut mask = Array2::<bool>::from_elem((7, 7), false);
        for r in 1..6 {
            for c in 1..6 {
                mask[[r, c]] = true;
            }
        }
        let eroded = binary_erode(&mask, 1);
        // After 1 erosion, border pixels should be gone
        assert!(!eroded[[1, 1]]);
        assert!(eroded[[3, 3]]); // center still true
    }

    #[test]
    fn test_binary_dilate() {
        let mut mask = Array2::<bool>::from_elem((7, 7), false);
        mask[[3, 3]] = true;
        let dilated = binary_dilate(&mask, 1);
        assert!(dilated[[2, 3]]);
        assert!(dilated[[4, 3]]);
        assert!(dilated[[3, 2]]);
        assert!(dilated[[3, 4]]);
        assert!(!dilated[[0, 0]]);
    }

    #[test]
    fn test_connected_components() {
        let mut mask = Array2::<bool>::from_elem((10, 10), false);
        // Two separate clusters
        mask[[1, 1]] = true;
        mask[[1, 2]] = true;
        mask[[8, 8]] = true;
        mask[[8, 9]] = true;
        let labels = connected_components(&mask);
        assert!(labels[[1, 1]] > 0);
        assert_eq!(labels[[1, 1]], labels[[1, 2]]);
        assert!(labels[[8, 8]] > 0);
        assert_ne!(labels[[1, 1]], labels[[8, 8]]);
    }

    #[test]
    fn test_pca_axis_horizontal_bar() {
        // A horizontal bar: 1 row, 20 cols — should have heading ~90°
        let mut mask = Array2::<bool>::from_elem((5, 25), false);
        for c in 2..23 {
            mask[[2, c]] = true;
        }
        let (heading, length, width) = pca_axis(&mask, 0.5);
        assert!(heading > 45.0 && heading < 135.0, "heading={heading}");
        assert!(length > width, "length={length} width={width}");
    }

    #[test]
    fn test_gradient_magnitude() {
        let mut grid = Array2::<f32>::zeros((5, 5));
        // Linear gradient in x direction
        for r in 0..5 {
            for c in 0..5 {
                grid[[r, c]] = c as f32;
            }
        }
        let grad = gradient_magnitude(&grid);
        // Interior should have gradient ~2 (central diff of unit slope, step=2)
        assert!((grad[[2, 2]] - 2.0).abs() < 0.1);
    }

    #[test]
    fn test_plasma_color_range() {
        let low = plasma_color(0.0);
        let high = plasma_color(1.0);
        for i in 0..4 {
            assert!(low[i] >= 0.0 && low[i] <= 1.0);
            assert!(high[i] >= 0.0 && high[i] <= 1.0);
        }
    }

    #[test]
    fn test_mesh_generation() {
        // Tiny 5×5 object with a peaked heightfield
        let mut hf = Array2::<f32>::from_elem((5, 5), f32::NAN);
        let mut mask = Array2::<bool>::from_elem((5, 5), false);
        for r in 1..4 {
            for c in 1..4 {
                mask[[r, c]] = true;
                hf[[r, c]] = if r == 2 && c == 2 { 1.0 } else { 0.3 };
            }
        }

        let obj = MaskedObject {
            id: 0,
            centroid_row: 2,
            centroid_col: 2,
            bbox: (1, 1, 3, 3),
            heightfield: hf,
            interior_mask: mask,
            eroded_edge_pixels: 0,
            peak_height: 1.0,
            area_m2: 2.25,
            heading_deg: 0.0,
            length_m: 1.5,
            width_m: 1.5,
        };

        let mesh = heightfield_to_mesh(&obj, 0.5, 1.0);
        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.indices.is_empty());
        assert_eq!(mesh.vertices.len(), mesh.normals.len());
        assert_eq!(mesh.vertices.len(), mesh.colors.len());

        // All indices should be valid
        for &idx in &mesh.indices {
            assert!((idx as usize) < mesh.vertices.len());
        }
    }

    #[test]
    fn test_obj_write() {
        let mesh = TriMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            indices: vec![0, 1, 2],
            colors: vec![[1.0, 0.0, 0.0, 1.0]; 3],
        };
        let tmp = std::env::temp_dir().join("test_wreck.obj");
        write_obj(&mesh, &tmp).unwrap();
        let content = std::fs::read_to_string(&tmp).unwrap();
        assert!(content.contains("v "));
        assert!(content.contains("f "));
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_glb_write() {
        let mesh = TriMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            indices: vec![0, 1, 2],
            colors: vec![[1.0, 0.0, 0.0, 1.0]; 3],
        };
        let tmp = std::env::temp_dir().join("test_wreck.glb");
        write_glb(&mesh, &tmp).unwrap();
        let data = std::fs::read(&tmp).unwrap();
        assert_eq!(&data[0..4], b"glTF"); // magic bytes
        std::fs::remove_file(&tmp).ok();
    }
}
