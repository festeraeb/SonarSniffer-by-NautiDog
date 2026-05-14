//! **Steps 3–6: Georectification, TVG, Nadir/Slant-Range, and Output Engine**
//!
//! This is the core mosaic rendering engine.  It consumes a `DiscoveryResult`
//! (from `channel_discovery`) and a `ParseResult` (from the parser) to produce
//! a fully georectified, multi-frequency, TVG-normalised side-scan mosaic.
//!
//! ## Architecture
//! - Uses the existing `MosaicGrid` (atomic accumulator) for pixel output.
//! - Replaces the old `project_pings_to_grid` with a discovery-aware pipeline.
//! - Adds trapezoidal interpolation, TVG, slant-range correction, nadir handling.
//! - Produces KML Super Overlays with `gx:LatLonQuad` for Google Earth.

use crate::channel_discovery::{DiscoveryResult, SignalArchetype, SpatialRole};
use crate::garmin_rsd_parser::{ParseResult, Ping};
use crate::mosaic::grid::MosaicGrid;
use crate::mosaic::projection::{latlon_to_meters, meters_to_latlon};
use image::{ImageBuffer, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ═══════════════════════════════════════════════════════════════════════════════
// §1  PUBLIC TYPES & CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════════

/// How to handle the nadir (water-column gap between port and starboard).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NadirMode {
    /// Pull port and starboard seafloor returns together to close the gap.
    Stitch,
    /// Map normalised DownVü/ClearVü ribbon into the center gap.
    Fill,
    /// Leave the gap transparent — show the true data limits.
    Raw,
}

impl Default for NadirMode {
    fn default() -> Self {
        Self::Stitch
    }
}

/// Configuration for the mosaic engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MosaicConfig {
    /// Grid resolution in metres per pixel.
    pub resolution_m: f64,
    /// Colormap name (maps to `apply_colormap` in outputs.rs).
    pub colormap: String,
    /// Nadir handling mode.
    pub nadir_mode: NadirMode,
    /// Whether to apply TVG (Time Varied Gain) correction.
    pub tvg_enabled: bool,
    /// TVG spreading loss coefficient (dB/decade). Default 15.
    pub tvg_alpha: f32,
    /// TVG absorption coefficient (dB/m). Default 0.08.
    pub tvg_beta: f32,
    /// Whether to apply histogram normalisation across transducers.
    pub histogram_normalize: bool,
    /// Whether to remove the water column (slant-range correct).
    pub remove_water_column: bool,
    /// Gamma correction for final image. Default 0.65.
    pub gamma: f32,
    /// MBTiles zoom levels to render.
    pub tile_zoom_levels: Vec<u32>,
    /// Base output directory.
    pub output_dir: PathBuf,
}

impl Default for MosaicConfig {
    fn default() -> Self {
        Self {
            resolution_m: 0.25,
            colormap: "amber".to_string(),
            nadir_mode: NadirMode::Stitch,
            tvg_enabled: true,
            tvg_alpha: 15.0,
            tvg_beta: 0.08,
            histogram_normalize: true,
            remove_water_column: true,
            gamma: 0.65,
            tile_zoom_levels: vec![14, 15, 16, 17, 18],
            output_dir: PathBuf::from("."),
        }
    }
}

/// Result of the mosaic rendering pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct MosaicOutput {
    pub grid_width: usize,
    pub grid_height: usize,
    pub bounds_latlon: (f64, f64, f64, f64),
    pub tiles_rendered: usize,
    pub artifacts: Vec<(String, String)>, // (kind, path)
    pub log: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// §2  CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Default sample interval (metres per sample) for Garmin sonar.
const DEFAULT_M_PER_SAMPLE: f64 = 0.01;

/// Maximum grid dimension (pixels) to prevent OOM on huge survey areas.
const MAX_GRID_DIM: usize = 32768;

// ═══════════════════════════════════════════════════════════════════════════════
// §3  STEP 3: GEORECTIFICATION & SWATH PROJECTION
// ═══════════════════════════════════════════════════════════════════════════════

/// Build the mosaic grid using discovery-driven channel mapping.
///
/// This replaces `project_pings_to_grid` with a pipeline that:
/// 1. Uses `DiscoveryResult` for channel roles (no static tables)
/// 2. Calculates true swath width from sample count × sample interval
/// 3. Applies slant-range correction
/// 4. Uses trapezoidal interpolation between consecutive pings
/// 5. Applies TVG, alpha feathering, and histogram normalization
pub fn build_mosaic(
    parsed: &ParseResult,
    discovery: &DiscoveryResult,
    config: &MosaicConfig,
) -> (Arc<MosaicGrid>, Vec<String>) {
    let mut log: Vec<String> = Vec::new();

    // ── Compute grid bounds from GPS data ───────────────────────────────────
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;

    // Estimate maximum swath extent for buffer
    let max_samples = parsed
        .pings
        .iter()
        .filter(|p| {
            let role = discovery.profile(p.channel).map(|pr| pr.spatial_role);
            role == Some(SpatialRole::Port) || role == Some(SpatialRole::Starboard)
        })
        .map(|p| p.samples.len())
        .max()
        .unwrap_or(0);
    let max_swath_m = max_samples as f64 * DEFAULT_M_PER_SAMPLE;
    let buffer_m = max_swath_m * 1.5 + 50.0;

    for p in &parsed.pings {
        if p.latitude == 0.0 || p.longitude == 0.0 || !p.latitude.is_finite() {
            continue;
        }
        let (x, y) = latlon_to_meters(p.latitude, p.longitude);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }

    if min_x == f64::MAX {
        log.push("No GPS data found — using dummy grid bounds".to_string());
        min_x = 0.0;
        max_x = 100.0;
        min_y = 0.0;
        max_y = 100.0;
    }

    min_x -= buffer_m;
    max_x += buffer_m;
    min_y -= buffer_m;
    max_y += buffer_m;

    // Clamp resolution to prevent absurd grid sizes
    let res = config.resolution_m.max(0.05);
    let tentative_w = ((max_x - min_x) / res).ceil() as usize + 1;
    let tentative_h = ((max_y - min_y) / res).ceil() as usize + 1;
    let effective_res = if tentative_w > MAX_GRID_DIM || tentative_h > MAX_GRID_DIM {
        let scale = (tentative_w.max(tentative_h) as f64) / (MAX_GRID_DIM as f64);
        let new_res = res * scale;
        log.push(format!(
            "Grid clamped: {tentative_w}×{tentative_h} too large at {res}m, using {new_res:.3}m",
        ));
        new_res
    } else {
        res
    };

    log.push(format!(
        "Grid bounds: ({:.1},{:.1})→({:.1},{:.1}) at {:.3}m/px, buffer={:.1}m",
        min_x, min_y, max_x, max_y, effective_res, buffer_m,
    ));

    let grid = Arc::new(MosaicGrid::new(min_x, min_y, max_x, max_y, effective_res));
    log.push(format!("Grid size: {}×{} pixels", grid.width, grid.height));

    // ── Pre-compute TVG LUT ─────────────────────────────────────────────────
    let tvg_lut = if config.tvg_enabled {
        Some(precompute_tvg_lut(max_samples, config.tvg_alpha, config.tvg_beta))
    } else {
        None
    };

    // ── Pre-compute per-channel histogram normalization ─────────────────────
    let channel_norms: BTreeMap<u32, (f32, f32)> = if config.histogram_normalize {
        compute_channel_norms(parsed, discovery)
    } else {
        BTreeMap::new()
    };

    // ── Project pings by scanline for trapezoidal interpolation ─────────────
    // Group sidescan pings by channel, sorted by timestamp.
    let (port_ch, star_ch) = discovery.primary_sidescan_pair();
    let center_ch = discovery.best_center_channel();

    // For each channel, collect pings in timestamp order
    let project_channel = |ch_id: u32, role: SpatialRole| {
        let pings: Vec<&Ping> = parsed
            .pings
            .iter()
            .filter(|p| {
                p.channel == ch_id
                    && p.latitude != 0.0
                    && p.longitude != 0.0
                    && p.latitude.is_finite()
                    && !p.samples.is_empty()
            })
            .collect();

        if pings.len() < 2 {
            return;
        }

        let profile = discovery.profile(ch_id);
        let nadir_gap = profile.map(|p| p.nadir_gap_width).unwrap_or(0);

        // Get norm factors for this channel
        let (norm_p2, norm_p98) = channel_norms
            .get(&ch_id)
            .copied()
            .unwrap_or((0.0, 65535.0));
        let norm_span = (norm_p98 - norm_p2).max(1.0);

        // Process consecutive ping pairs for trapezoidal interpolation
        for pair in pings.windows(2) {
            let ping_a = pair[0];
            let ping_b = pair[1];

            // Skip if pings are too far apart in time (>2 seconds = different track)
            if ping_b.timestamp_ms.saturating_sub(ping_a.timestamp_ms) > 2000 {
                continue;
            }

            let (ax, ay) = latlon_to_meters(ping_a.latitude, ping_a.longitude);
            let (bx, by) = latlon_to_meters(ping_b.latitude, ping_b.longitude);

            let heading_a = ping_a.heading_deg.unwrap_or(0.0) as f64;
            let heading_b = ping_b.heading_deg.unwrap_or(0.0) as f64;

            let angle_offset = match role {
                SpatialRole::Port => -90.0,
                SpatialRole::Starboard => 90.0,
                SpatialRole::Center => 0.0,
                _ => return,
            };

            let depth_a = ping_a.depth_m as f64;
            let depth_b = ping_b.depth_m as f64;

            let n_a = ping_a.samples.len();
            let n_b = ping_b.samples.len();
            let n_max = n_a.max(n_b);
            if n_max == 0 {
                continue;
            }

            // True swath width for each ping
            let swath_a = compute_swath_m(n_a, depth_a, nadir_gap, &config);
            let swath_b = compute_swath_m(n_b, depth_b, nadir_gap, &config);

            // Sweet-spot sigma for alpha feathering (Gaussian beam weight)
            let sigma_a = (swath_a * 0.30).max(0.25);
            let sigma_b = (swath_b * 0.30).max(0.25);

            // Number of interpolation steps along the track (between ping A and B)
            let track_dist = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
            let track_steps = ((track_dist / grid.resolution).ceil() as usize).max(1).min(500);

            // Center scale for downscan: narrow projection
            let center_scale = if role == SpatialRole::Center {
                0.1
            } else {
                1.0
            };

            for ts in 0..=track_steps {
                let t = ts as f64 / track_steps as f64;

                // Interpolate position along track
                let cx = ax + (bx - ax) * t;
                let cy = ay + (by - ay) * t;

                // Interpolate heading
                let heading = lerp_heading(heading_a, heading_b, t);
                let depth = depth_a + (depth_b - depth_a) * t;
                let swath = swath_a + (swath_b - swath_a) * t;
                let sigma = sigma_a + (sigma_b - sigma_a) * t;

                // Perpendicular vector
                let true_angle = heading + angle_offset;
                let math_rad = (90.0 - true_angle).to_radians();
                let cos_a = math_rad.cos();
                let sin_a = math_rad.sin();

                // Sample count for cross-track stepping
                let _n_interp = n_a + ((n_b as f64 - n_a as f64) * t) as usize;
                let swath_proj = swath * center_scale;

                // Number of cross-track steps
                let cross_steps =
                    ((swath_proj / grid.resolution).ceil() as usize).max(1).min(2000);

                for cs in 0..=cross_steps {
                    let frac = cs as f64 / cross_steps as f64;
                    let ground_m = frac * swath;

                    // Skip nadir zone if in Stitch mode
                    if config.nadir_mode == NadirMode::Stitch && role != SpatialRole::Center {
                        // Skip the inner nadir_gap region
                        let nadir_m = nadir_gap as f64 * DEFAULT_M_PER_SAMPLE;
                        if ground_m < nadir_m && config.remove_water_column {
                            continue;
                        }
                    }

                    // Slant-range correction: map ground distance back to slant for sampling
                    let slant_m = (ground_m * ground_m + depth * depth).sqrt();
                    let sample_pos = slant_m / DEFAULT_M_PER_SAMPLE;

                    // Interpolate intensity from ping A and B
                    let intensity_a = sample_intensity(ping_a, sample_pos, &tvg_lut);
                    let intensity_b = sample_intensity(ping_b, sample_pos, &tvg_lut);
                    let raw_intensity = intensity_a * (1.0 - t as f32) + intensity_b * t as f32;

                    // Histogram normalization
                    let normalized = if config.histogram_normalize {
                        ((raw_intensity - norm_p2) / norm_span).clamp(0.0, 1.0)
                            * 65535.0
                    } else {
                        raw_intensity
                    };

                    // Alpha feathering: Gaussian weight (sweet spot ~60% of swath)
                    let sweet_spot = swath * 0.6;
                    let dist_from_sweet = (ground_m - sweet_spot).abs();
                    let gauss_weight =
                        (-(dist_from_sweet * dist_from_sweet) / (2.0 * sigma * sigma))
                            .exp() as f32;

                    // Edge fade: reduce weight at very near/far range
                    let edge_frac = frac as f32;
                    let edge_weight = if edge_frac < 0.05 {
                        edge_frac / 0.05 // fade in at near range
                    } else if edge_frac > 0.95 {
                        (1.0 - edge_frac) / 0.05 // fade out at far range
                    } else {
                        1.0
                    };

                    let final_weight = gauss_weight * edge_weight;

                    let proj_m = ground_m * center_scale;
                    let px = cx + proj_m * cos_a;
                    let py = cy + proj_m * sin_a;

                    grid.add_weighted_sample(px, py, normalized, final_weight);
                }
            }
        }
    };

    // ── Project all discovered channels ─────────────────────────────────────
    if let Some(port) = port_ch {
        log.push(format!("Projecting port ch{port}..."));
        project_channel(port, SpatialRole::Port);
    }
    if let Some(star) = star_ch {
        log.push(format!("Projecting starboard ch{star}..."));
        project_channel(star, SpatialRole::Starboard);
    }

    // Fill mode: project DownVü into center
    if config.nadir_mode == NadirMode::Fill {
        if let Some(center) = center_ch {
            log.push(format!("Projecting center fill ch{center}..."));
            project_channel(center, SpatialRole::Center);
        }
    }

    log.push(format!(
        "Projected {} pings across {} channels",
        parsed.pings.len(),
        discovery.profiles.len()
    ));

    (grid, log)
}

// ═══════════════════════════════════════════════════════════════════════════════
// §4  STEP 4: TVG (TIME VARIED GAIN)
// ═══════════════════════════════════════════════════════════════════════════════

/// Pre-compute TVG correction LUT.
///
/// TVG(r) = α × log₁₀(r) + β × r
/// Applied as a multiplicative gain: gain = 10^(TVG/20)
///
/// This compensates for signal attenuation with range, producing the uniform
/// "orange glow" across the entire swath.
fn precompute_tvg_lut(max_samples: usize, alpha: f32, beta: f32) -> Vec<f32> {
    let mut lut = Vec::with_capacity(max_samples);
    for i in 0..max_samples {
        let range_m = (i as f32 + 1.0) * DEFAULT_M_PER_SAMPLE as f32;
        let tvg_db = alpha * range_m.log10().max(0.0) + beta * range_m;
        let gain = (10.0_f32).powf(tvg_db / 20.0);
        // Clamp gain to prevent extreme amplification of near-field noise
        lut.push(gain.clamp(0.5, 50.0));
    }
    lut
}

/// Sample intensity from a ping with linear interpolation and optional TVG.
#[inline]
fn sample_intensity(ping: &Ping, sample_pos: f64, tvg_lut: &Option<Vec<f32>>) -> f32 {
    let n = ping.samples.len();
    if n == 0 {
        return 0.0;
    }

    let base = sample_pos.floor() as usize;
    let raw = if base + 1 < n {
        let t = (sample_pos - base as f64) as f32;
        let a = ping.samples[base] as f32;
        let b = ping.samples[base + 1] as f32;
        a + (b - a) * t
    } else if base < n {
        ping.samples[base] as f32
    } else {
        0.0
    };

    // Apply TVG if enabled
    if let Some(lut) = tvg_lut {
        let idx = base.min(lut.len().saturating_sub(1));
        raw * lut.get(idx).copied().unwrap_or(1.0)
    } else {
        raw
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// §5  STEP 4: HISTOGRAM NORMALIZATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute per-channel P2/P98 normalization values.
///
/// This ensures consistent brightness across different transducers
/// (GT54 vs GT56) and firmware versions by stretching each channel's
/// histogram to a common range.
fn compute_channel_norms(
    parsed: &ParseResult,
    discovery: &DiscoveryResult,
) -> BTreeMap<u32, (f32, f32)> {
    let mut norms = BTreeMap::new();

    for profile in &discovery.profiles {
        if profile.archetype == SignalArchetype::DepthTemp
            || profile.archetype == SignalArchetype::Noise
        {
            continue;
        }

        // Collect a sample of intensity values from this channel
        let mut values: Vec<f32> = Vec::with_capacity(50_000);
        let mut count = 0usize;
        for ping in &parsed.pings {
            if ping.channel != profile.channel_id {
                continue;
            }
            // Sample every Nth ping to keep it fast
            count += 1;
            if count % 5 != 0 {
                continue;
            }
            for &s in &ping.samples {
                if s > 0 {
                    values.push(s as f32);
                }
            }
            if values.len() > 100_000 {
                break;
            }
        }

        if values.len() < 100 {
            continue;
        }

        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p2 = values[(values.len() as f32 * 0.02) as usize];
        let p98 = values[(values.len() as f32 * 0.98) as usize];

        norms.insert(profile.channel_id, (p2, p98));
    }

    norms
}

// ═══════════════════════════════════════════════════════════════════════════════
// §6  STEP 5: NADIR & SLANT RANGE HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Calculate the true horizontal swath width for a ping.
///
/// Uses slant-range correction: ground_range = √(slant² − depth²)
fn compute_swath_m(sample_count: usize, depth_m: f64, nadir_gap: usize, config: &MosaicConfig) -> f64 {
    if sample_count == 0 {
        return 0.0;
    }

    let max_slant_m = sample_count as f64 * DEFAULT_M_PER_SAMPLE;

    // Skip nadir gap samples if stitching
    let effective_slant = if config.remove_water_column || config.nadir_mode == NadirMode::Stitch {
        let nadir_m = nadir_gap as f64 * DEFAULT_M_PER_SAMPLE;
        (max_slant_m - nadir_m).max(0.0)
    } else {
        max_slant_m
    };

    // Slant-range to ground-range correction
    if effective_slant > depth_m {
        (effective_slant * effective_slant - depth_m * depth_m).sqrt()
    } else {
        effective_slant * 0.5 // Very shallow — fallback to half-projection
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// §7  HEADING INTERPOLATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Linearly interpolate between two headings, handling the 0°/360° wrap.
fn lerp_heading(a: f64, b: f64, t: f64) -> f64 {
    let mut diff = b - a;
    // Normalize to [-180, 180]
    while diff > 180.0 {
        diff -= 360.0;
    }
    while diff < -180.0 {
        diff += 360.0;
    }
    let result = a + diff * t;
    // Normalize to [0, 360)
    ((result % 360.0) + 360.0) % 360.0
}

// ═══════════════════════════════════════════════════════════════════════════════
// §8  STEP 6: OUTPUT GENERATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Run the full mosaic pipeline: build grid → render image → generate tiles + KML.
pub fn render_mosaic(
    parsed: &ParseResult,
    discovery: &DiscoveryResult,
    config: &MosaicConfig,
) -> MosaicOutput {
    let mut all_log: Vec<String> = Vec::new();
    let mut artifacts: Vec<(String, String)> = Vec::new();

    // ── Build the georectified grid ─────────────────────────────────────────
    let (grid, build_log) = build_mosaic(parsed, discovery, config);
    all_log.extend(build_log);

    let bounds = grid.bounds_latlon();
    all_log.push(format!(
        "Bounds: lat [{:.6}, {:.6}] lon [{:.6}, {:.6}]",
        bounds.0, bounds.2, bounds.1, bounds.3
    ));

    // ── Render the master image ─────────────────────────────────────────────
    let master_img = build_image_with_gamma(&grid, &config.colormap, config.gamma);
    let master_path = config.output_dir.join("mosaic_master.png");
    if let Err(e) = master_img.save(&master_path) {
        all_log.push(format!("Failed to save master image: {e}"));
    } else {
        artifacts.push(("mosaic_png".into(), master_path.display().to_string()));
        all_log.push(format!(
            "Master image: {}×{} px → {}",
            master_img.width(),
            master_img.height(),
            master_path.display()
        ));
    }

    // ── Generate 256×256 tile pyramid ───────────────────────────────────────
    let tiles_dir = config.output_dir.join("tiles");
    let tile_count = match render_tile_pyramid(&grid, &config.colormap, config.gamma, &config.tile_zoom_levels, &tiles_dir) {
        Ok(n) => {
            artifacts.push(("tile_pyramid".into(), tiles_dir.display().to_string()));
            all_log.push(format!("Tile pyramid: {n} tiles at zooms {:?}", config.tile_zoom_levels));
            n
        }
        Err(e) => {
            all_log.push(format!("Tile pyramid failed: {e}"));
            0
        }
    };

    // ── Generate KML Super Overlay ──────────────────────────────────────────
    let kml_path = config.output_dir.join("mosaic_overlay.kml");
    match generate_kml_super_overlay(&grid, &config.tile_zoom_levels, &tiles_dir, &kml_path) {
        Ok(_) => {
            artifacts.push(("kml_super_overlay".into(), kml_path.display().to_string()));
            all_log.push(format!("KML Super Overlay → {}", kml_path.display()));
        }
        Err(e) => {
            all_log.push(format!("KML generation failed: {e}"));
        }
    }

    // ── Generate KMZ (zipped KML + tiles) ───────────────────────────────────
    let kmz_path = config.output_dir.join("mosaic_overlay.kmz");
    match generate_kmz(&grid, &config.colormap, config.gamma, &kmz_path) {
        Ok(ok) => {
            if ok {
                artifacts.push(("kmz".into(), kmz_path.display().to_string()));
                all_log.push(format!("KMZ → {}", kmz_path.display()));
            }
        }
        Err(e) => {
            all_log.push(format!("KMZ generation failed: {e}"));
        }
    }

    // ── Generate MBTiles ────────────────────────────────────────────────────
    let mbt_path = config.output_dir.join("mosaic.mbtiles");
    match crate::mosaic::blending::export_mbtiles(grid.clone(), &config.tile_zoom_levels, &mbt_path) {
        Ok(_) => {
            artifacts.push(("mbtiles".into(), mbt_path.display().to_string()));
            all_log.push(format!("MBTiles → {}", mbt_path.display()));
        }
        Err(e) => {
            all_log.push(format!("MBTiles failed: {e}"));
        }
    }

    for line in &all_log {
        eprintln!("[mosaic-engine] {}", line);
    }

    MosaicOutput {
        grid_width: grid.width,
        grid_height: grid.height,
        bounds_latlon: bounds,
        tiles_rendered: tile_count,
        artifacts,
        log: all_log,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// §9  IMAGE RENDERING WITH GAMMA
// ═══════════════════════════════════════════════════════════════════════════════

/// Build an RGBA image from the grid with P2/P98 stretch and configurable gamma.
pub fn build_image_with_gamma(grid: &MosaicGrid, colormap: &str, gamma: f32) -> RgbaImage {
    let mut img = ImageBuffer::new(grid.width as u32, grid.height as u32);

    // Gather non-zero intensities for percentile stretch
    let mut intensities: Vec<f32> = Vec::with_capacity(10000);
    for py in 0..grid.height {
        for px in 0..grid.width {
            let v = grid.get_normalized_pixel(px, py);
            if v > 0.0 {
                intensities.push(v);
            }
        }
    }

    let (p_min, p_max) = if !intensities.is_empty() {
        // Sub-sample if huge
        if intensities.len() > 100_000 {
            let step = intensities.len() / 20000;
            intensities = intensities.into_iter().step_by(step).collect();
        }
        intensities.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
        let lo = intensities[(intensities.len() as f32 * 0.02) as usize];
        let hi = intensities[(intensities.len() as f32 * 0.98) as usize];
        if hi > lo {
            (lo, hi)
        } else {
            (lo, lo + 1.0)
        }
    } else {
        (0.0, 255.0)
    };

    for y in 0..grid.height {
        let img_y = (grid.height - 1 - y) as u32;
        for x in 0..grid.width {
            let v = grid.get_normalized_pixel(x, y);
            if v > 0.0 {
                let mut norm = ((v - p_min) / (p_max - p_min)).clamp(0.0, 1.0);
                norm = norm.powf(gamma);
                let rgb = crate::outputs::apply_colormap(norm, colormap);
                img.put_pixel(x as u32, img_y, Rgba([rgb[0], rgb[1], rgb[2], 255]));
            }
        }
    }
    img
}

// ═══════════════════════════════════════════════════════════════════════════════
// §10  TILE PYRAMID RENDERER
// ═══════════════════════════════════════════════════════════════════════════════

const EARTH_CIRCUMFERENCE: f64 = 40075016.68;
const EARTH_HALF: f64 = 20037508.34;

/// Render a zoom-level pyramid of 256×256 PNG tiles.
fn render_tile_pyramid(
    grid: &MosaicGrid,
    colormap: &str,
    gamma: f32,
    zoom_levels: &[u32],
    tiles_dir: &Path,
) -> Result<usize, anyhow::Error> {
    std::fs::create_dir_all(tiles_dir)?;

    // Pre-compute global normalization from grid data
    let mut intensities: Vec<f32> = Vec::with_capacity(50_000);
    let step_x = (grid.width / 500).max(1);
    let step_y = (grid.height / 500).max(1);
    for py in (0..grid.height).step_by(step_y) {
        for px in (0..grid.width).step_by(step_x) {
            let v = grid.get_normalized_pixel(px, py);
            if v > 0.0 {
                intensities.push(v);
            }
        }
    }
    let (p_min, p_max) = if !intensities.is_empty() {
        intensities.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
        let lo = intensities[(intensities.len() as f32 * 0.02) as usize];
        let hi = intensities[(intensities.len() as f32 * 0.98) as usize];
        if hi > lo { (lo, hi) } else { (lo, lo + 1.0) }
    } else {
        (0.0, 255.0)
    };

    let mut total_tiles = 0usize;

    for &zoom in zoom_levels {
        let zoom_dir = tiles_dir.join(zoom.to_string());
        std::fs::create_dir_all(&zoom_dir)?;

        let scale = 1_u64 << zoom;
        let tile_size_m = EARTH_CIRCUMFERENCE / scale as f64;

        // Determine tile range that intersects our grid
        let min_col = ((grid.min_x + EARTH_HALF) / tile_size_m).floor() as u32;
        let max_col = ((grid.max_x + EARTH_HALF) / tile_size_m).floor() as u32;
        let min_row = ((grid.min_y + EARTH_HALF) / tile_size_m).floor() as u32;
        let max_row = ((grid.max_y + EARTH_HALF) / tile_size_m).floor() as u32;

        for col in min_col..=max_col {
            let col_dir = zoom_dir.join(col.to_string());
            std::fs::create_dir_all(&col_dir)?;

            for row in min_row..=max_row {
                let t_min_x = col as f64 * tile_size_m - EARTH_HALF;
                let t_min_y = row as f64 * tile_size_m - EARTH_HALF;
                let t_max_x = t_min_x + tile_size_m;
                let t_max_y = t_min_y + tile_size_m;

                // Check overlap with grid
                if t_max_x < grid.min_x
                    || t_min_x > grid.max_x
                    || t_max_y < grid.min_y
                    || t_min_y > grid.max_y
                {
                    continue;
                }

                let tile_dx = (t_max_x - t_min_x) / 256.0;
                let tile_dy = (t_max_y - t_min_y) / 256.0;

                let mut tile = ImageBuffer::new(256, 256);
                let mut has_data = false;

                for py in 0..256u32 {
                    let y_m = t_max_y - (py as f64 * tile_dy);
                    for px in 0..256u32 {
                        let x_m = t_min_x + (px as f64 * tile_dx);

                        if x_m >= grid.min_x
                            && x_m <= grid.max_x
                            && y_m >= grid.min_y
                            && y_m <= grid.max_y
                        {
                            let gx = ((x_m - grid.min_x) / grid.resolution) as usize;
                            let gy = ((y_m - grid.min_y) / grid.resolution) as usize;
                            if gx < grid.width && gy < grid.height {
                                let v = grid.get_normalized_pixel(gx, gy);
                                if v > 0.0 {
                                    has_data = true;
                                    let mut norm =
                                        ((v - p_min) / (p_max - p_min)).clamp(0.0, 1.0);
                                    norm = norm.powf(gamma);
                                    let rgb =
                                        crate::outputs::apply_colormap(norm, colormap);
                                    tile.put_pixel(
                                        px,
                                        py,
                                        Rgba([rgb[0], rgb[1], rgb[2], 255]),
                                    );
                                }
                            }
                        }
                    }
                }

                if has_data {
                    let tile_path = col_dir.join(format!("{row}.png"));
                    tile.save(&tile_path)?;
                    total_tiles += 1;
                }
            }
        }
    }

    Ok(total_tiles)
}

// ═══════════════════════════════════════════════════════════════════════════════
// §11  KML SUPER OVERLAY WITH gx:LatLonQuad
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate a KML Super Overlay file referencing the tile pyramid.
///
/// Uses `gx:LatLonQuad` for precise corner georeferencing (no fractaling).
/// Creates a hierarchical structure with `<Region>` LOD control.
fn generate_kml_super_overlay(
    grid: &MosaicGrid,
    zoom_levels: &[u32],
    tiles_dir: &Path,
    kml_path: &Path,
) -> Result<(), anyhow::Error> {
    let (_min_lat, _min_lon, _max_lat, _max_lon) = grid.bounds_latlon();

    let mut kml = String::new();
    kml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2"
     xmlns:gx="http://www.google.com/kml/ext/2.2">
<Document>
  <name>Sonar Mosaic Super Overlay</name>
  <description>Generated by SonarSniffer Mosaic Engine</description>
"#);

    for &zoom in zoom_levels {
        let scale = 1_u64 << zoom;
        let tile_size_m = EARTH_CIRCUMFERENCE / scale as f64;

        let min_col = ((grid.min_x + EARTH_HALF) / tile_size_m).floor() as u32;
        let max_col = ((grid.max_x + EARTH_HALF) / tile_size_m).floor() as u32;
        let min_row = ((grid.min_y + EARTH_HALF) / tile_size_m).floor() as u32;
        let max_row = ((grid.max_y + EARTH_HALF) / tile_size_m).floor() as u32;

        // LOD pixel range: tiles appear at appropriate zoom levels
        let min_lod = match zoom {
            0..=10 => 0,
            11..=14 => 128,
            15..=17 => 256,
            _ => 512,
        };
        let max_lod = if zoom == *zoom_levels.last().unwrap_or(&18) {
            -1 // infinite
        } else {
            1024
        };

        for col in min_col..=max_col {
            for row in min_row..=max_row {
                let tile_path = tiles_dir
                    .join(zoom.to_string())
                    .join(col.to_string())
                    .join(format!("{row}.png"));

                if !tile_path.exists() {
                    continue;
                }

                // Compute tile corners in lat/lon
                let t_min_x = col as f64 * tile_size_m - EARTH_HALF;
                let t_min_y = row as f64 * tile_size_m - EARTH_HALF;
                let t_max_x = t_min_x + tile_size_m;
                let t_max_y = t_min_y + tile_size_m;

                let (sw_lat, sw_lon) = meters_to_latlon(t_min_x, t_min_y);
                let (nw_lat, nw_lon) = meters_to_latlon(t_min_x, t_max_y);
                let (ne_lat, ne_lon) = meters_to_latlon(t_max_x, t_max_y);
                let (se_lat, se_lon) = meters_to_latlon(t_max_x, t_min_y);

                // Use relative path from KML to tiles
                let rel_path = format!("tiles/{zoom}/{col}/{row}.png");

                kml.push_str(&format!(
                    r#"  <GroundOverlay>
    <name>z{zoom}/{col}/{row}</name>
    <Region>
      <LatLonAltBox>
        <north>{ne_lat:.8}</north>
        <south>{sw_lat:.8}</south>
        <east>{ne_lon:.8}</east>
        <west>{sw_lon:.8}</west>
      </LatLonAltBox>
      <Lod>
        <minLodPixels>{min_lod}</minLodPixels>
        <maxLodPixels>{max_lod}</maxLodPixels>
      </Lod>
    </Region>
    <Icon>
      <href>{rel_path}</href>
    </Icon>
    <gx:LatLonQuad>
      <coordinates>
        {sw_lon:.8},{sw_lat:.8},0
        {se_lon:.8},{se_lat:.8},0
        {ne_lon:.8},{ne_lat:.8},0
        {nw_lon:.8},{nw_lat:.8},0
      </coordinates>
    </gx:LatLonQuad>
  </GroundOverlay>
"#
                ));
            }
        }
    }

    kml.push_str("</Document>\n</kml>\n");

    std::fs::write(kml_path, kml)?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// §12  KMZ GENERATION (SINGLE-FILE BUNDLE)
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate a KMZ (zipped KML + overlay PNG) with `gx:LatLonQuad`.
fn generate_kmz(
    grid: &MosaicGrid,
    colormap: &str,
    gamma: f32,
    kmz_path: &Path,
) -> Result<bool, anyhow::Error> {
    let (min_lat, min_lon, max_lat, max_lon) = grid.bounds_latlon();

    // Scale down if too large
    let max_dim = grid.width.max(grid.height);
    let scale = if max_dim > 4096 {
        4096.0 / max_dim as f64
    } else {
        1.0
    };

    let out_w = (grid.width as f64 * scale) as u32;
    let out_h = (grid.height as f64 * scale) as u32;
    if out_w == 0 || out_h == 0 {
        return Ok(false);
    }

    let img = build_image_with_gamma(grid, colormap, gamma);

    // Resize if needed
    let final_img = if scale < 1.0 {
        image::imageops::resize(
            &img,
            out_w,
            out_h,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };

    let mut png_bytes = Vec::new();
    final_img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )?;

    if png_bytes.is_empty() {
        return Ok(false);
    }

    // Compute corner coordinates for gx:LatLonQuad
    let (sw_lat, sw_lon) = (min_lat, min_lon);
    let (nw_lat, nw_lon) = (max_lat, min_lon);
    let (ne_lat, ne_lon) = (max_lat, max_lon);
    let (se_lat, se_lon) = (min_lat, max_lon);

    let kml_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2"
     xmlns:gx="http://www.google.com/kml/ext/2.2">
  <Document>
    <name>Sonar Mosaic</name>
    <description>Generated by SonarSniffer Mosaic Engine</description>
    <GroundOverlay>
      <name>High-Res Sonar Overlay</name>
      <Icon>
        <href>overlay.png</href>
      </Icon>
      <gx:LatLonQuad>
        <coordinates>
          {sw_lon:.8},{sw_lat:.8},0
          {se_lon:.8},{se_lat:.8},0
          {ne_lon:.8},{ne_lat:.8},0
          {nw_lon:.8},{nw_lat:.8},0
        </coordinates>
      </gx:LatLonQuad>
    </GroundOverlay>
  </Document>
</kml>"#
    );

    let file = std::fs::File::create(kmz_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("doc.kml", options)?;
    zip.write_all(kml_content.as_bytes())?;

    zip.start_file("overlay.png", options)?;
    zip.write_all(&png_bytes)?;

    zip.finish()?;
    Ok(true)
}

// ═══════════════════════════════════════════════════════════════════════════════
// §13  TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tvg_lut_monotonic() {
        let lut = precompute_tvg_lut(1000, 15.0, 0.08);
        assert_eq!(lut.len(), 1000);
        // TVG gain should generally increase with range
        assert!(lut[500] > lut[50], "TVG should increase with range");
        // Near-field should be clamped (not too close to 0)
        assert!(lut[0] >= 0.5, "Near-field gain should be >= 0.5");
    }

    #[test]
    fn test_heading_interpolation() {
        // Simple case
        assert!((lerp_heading(10.0, 20.0, 0.5) - 15.0).abs() < 0.01);
        // Wrap around 360
        assert!((lerp_heading(350.0, 10.0, 0.5) - 0.0).abs() < 0.01);
        // Wrap around the other way
        assert!((lerp_heading(10.0, 350.0, 0.5) - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_swath_calculation() {
        let config = MosaicConfig::default();
        // 1000 samples at 0.01m = 10m slant range, depth 3m
        // ground = sqrt(10² - 3²) = sqrt(91) ≈ 9.54m
        let swath = compute_swath_m(1000, 3.0, 0, &config);
        assert!(swath > 9.0 && swath < 10.0, "Swath should be ~9.5m, got {swath:.2}");
    }

    #[test]
    fn test_default_config() {
        let config = MosaicConfig::default();
        assert_eq!(config.resolution_m, 0.25);
        assert_eq!(config.nadir_mode, NadirMode::Stitch);
        assert!(config.tvg_enabled);
    }
}
