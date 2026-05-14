//! Main signal processing pipeline.
//!
//! Applies the full enhancement stack:
//! Raw → TVG → Log Compression → Filtering → Histogram Eq → Colormap

use crate::egn;
use crate::garmin_rsd_parser::Ping;
use crate::video_enhanced::{
    filters, statistics::DatasetStatistics, tvg, ColorLUT, SonarProcessingParams,
};
use std::collections::HashMap;

/// Processed frame data ready for video encoding.
#[derive(Debug, Clone)]
pub struct ProcessedFrame {
    /// RGB pixels (width × height × 3)
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Intermediate processing result (one frame).
#[derive(Debug)]
struct IntermediateFrame {
    /// Grayscale intensities (0.0-1.0 normalized)
    intensities: Vec<f32>,
    width: usize,
    height: usize,
}

/// Apply full processing pipeline to ping dataset.
///
/// Returns a vector of processed frames ready for encoding.
pub fn apply_processing_pipeline(
    pings: &[Ping],
    params: &SonarProcessingParams,
    stats: &DatasetStatistics,
) -> anyhow::Result<Vec<ProcessedFrame>> {
    // Group pings by channel for stitching
    let mut by_channel: HashMap<u32, Vec<&Ping>> = HashMap::new();
    for ping in pings {
        by_channel.entry(ping.channel).or_default().push(ping);
    }

    // Detect best port/starboard pair based on known IDs and total sample mass.
    let (port_pings, star_pings) = select_sidescan_pair(&by_channel);

    let (mode_width, nadir_skip, port_pings, star_pings, single_channel) =
        if let (Some(port), Some(star)) = (port_pings, star_pings) {
            // Compute water-column nadir skip if requested (same logic as static mosaic).
            let skip = if params.remove_water_column {
                let ps = detect_nadir_video(&port);
                let ss = detect_nadir_video(&star);
                ps.min(ss)
            } else {
                0
            };
            let max_samples_side = port
                .iter()
                .chain(star.iter())
                .map(|p| p.samples.len().saturating_sub(skip))
                .max()
                .unwrap_or(stats.max_samples)
                .max(1);
            (max_samples_side * 2, skip, Some(port), Some(star), None)
        } else {
            // Fallback to primary channel only (legacy behavior)
            let primary_pings: Vec<&Ping> = pings
                .iter()
                .filter(|p| p.channel == stats.primary_channel)
                .collect();

            if primary_pings.is_empty() {
                anyhow::bail!(
                    "No pings found for primary channel {}",
                    stats.primary_channel
                );
            }

            let skip = if params.remove_water_column {
                detect_nadir_video(&primary_pings)
            } else {
                0
            };
            (
                stats.max_samples.saturating_sub(skip),
                skip,
                None,
                None,
                Some(primary_pings),
            )
        };

    let height = params.video_height as usize;
    let total_rows = if let (Some(port), Some(star)) = (&port_pings, &star_pings) {
        port.len().max(star.len())
    } else {
        single_channel.as_ref().map(|v| v.len()).unwrap_or(0)
    };
    let total_frames = (total_rows + height - 1).max(1) / height.max(1);

    // ── Blanking-zone detection (GT56 AGC ring-down) ─────────────────────────
    // Detect from the first ≤500 pings of whichever side is available.
    // Result drives:
    //   1. TVG start sample (don't amplify hardware silence)
    //   2. Per-row soft-fill in both frame builders
    let blanking = {
        let rep_pings: Vec<&[u16]> = if let Some(ref p) = port_pings {
            p.iter().take(500).map(|p| p.samples.as_slice()).collect()
        } else if let Some(ref s) = single_channel {
            s.iter().take(500).map(|p| p.samples.as_slice()).collect()
        } else {
            vec![]
        };
        egn::detect_blanking_zone(&rep_pings)
    };
    if blanking.is_active() {
        eprintln!(
            "[processing] Blanking zone detected: end_sample={} peak_blank_rate={:.1}%",
            blanking.end_sample,
            blanking.peak_blank_rate * 100.0,
        );
    }

    // Precompute TVG LUT (per side when stitching).
    // If a blanking zone was found, delay TVG start past the dead zone.
    let side_width = if port_pings.is_some() {
        mode_width / 2
    } else {
        mode_width
    };
    let tvg_lut_side = {
        let tvg_start = tvg::blanking_aware_start_sample(&blanking, params.tvg_start_sample);
        if tvg_start != params.tvg_start_sample {
            eprintln!(
                "[processing] TVG start delayed: {}→{} (blanking-aware)",
                params.tvg_start_sample, tvg_start,
            );
        }

        // Infer transducer profile from signal characteristics to tune TVG.
        let entropy_like_detail = (stats.percentile_ceiling - stats.percentile_floor) > 1024.0;
        let max_sample_value = stats.raw_max.clamp(0.0, u16::MAX as f32) as u16;
        let inferred_profile =
            tvg::TransducerProfile::from_signal(entropy_like_detail, max_sample_value);

        let mut p = params.clone();
        p.tvg_start_sample = tvg_start;
        p.tvg_absorption_db_per_m = inferred_profile.absorption_db_per_m();
        p.tvg_spreading_factor = inferred_profile.spreading_factor();
        tvg::precompute_tvg_lut(side_width, &p)
    };

    // Generate colormap LUT
    let colormap_lut = params.colormap.generate_lut();

    // Determine dynamic range using either combined channels or provided stats
    let (floor_db, ceiling_db) = if params.use_adaptive_range {
        let percentile = compute_percentiles_combined(
            port_pings.as_deref(),
            star_pings.as_deref(),
            single_channel.as_deref(),
            params,
        );
        let floor = if percentile.0 > 0.0 {
            20.0 * percentile.0.log10()
        } else {
            params.noise_floor_db
        };
        let ceiling = if percentile.1 > 0.0 {
            20.0 * percentile.1.log10()
        } else {
            params.signal_ceiling_db
        };
        (floor, ceiling)
    } else {
        (params.noise_floor_db, params.signal_ceiling_db)
    };

    let mut frames = Vec::with_capacity(total_frames);

    for frame_idx in 0..total_frames {
        let row_start = frame_idx * height;
        let row_end = (row_start + height).min(total_rows);

        let intermediate = if let (Some(port), Some(star)) = (&port_pings, &star_pings) {
            build_intermediate_frame_stitched(
                port,
                star,
                row_start,
                row_end,
                side_width,
                height,
                nadir_skip,
                &tvg_lut_side,
                params,
                floor_db,
                ceiling_db,
                &blanking,
            )?
        } else {
            let channel = single_channel.as_ref().unwrap();
            let frame_pings = &channel[row_start.min(channel.len())..row_end.min(channel.len())];
            build_intermediate_frame_single(
                frame_pings,
                mode_width,
                height,
                nadir_skip,
                &tvg_lut_side,
                params,
                floor_db,
                ceiling_db,
                &blanking,
            )?
        };

        let filtered = filters::apply_filters(
            &intermediate.intensities,
            intermediate.width,
            intermediate.height,
            params,
        );

        let enhanced = if params.histogram_equalization {
            histogram_equalize(&filtered, intermediate.width, intermediate.height)
        } else {
            filtered
        };

        let enhanced = if params.clahe_enabled {
            apply_clahe(
                &enhanced,
                intermediate.width,
                intermediate.height,
                params.clahe_tile_size,
                params.clahe_clip_limit,
            )
        } else {
            enhanced
        };

        let rgb_pixels = apply_colormap(&enhanced, &colormap_lut);

        frames.push(ProcessedFrame {
            pixels: rgb_pixels,
            width: intermediate.width as u32,
            height: intermediate.height as u32,
        });
    }

    Ok(frames)
}

/// Build intermediate frame for a single channel (legacy path).
fn build_intermediate_frame_single(
    pings: &[&Ping],
    width: usize,
    height: usize,
    nadir_skip: usize,
    tvg_lut: &[f32],
    params: &SonarProcessingParams,
    floor_db: f32,
    ceiling_db: f32,
    blanking: &egn::BlankingZone,
) -> anyhow::Result<IntermediateFrame> {
    let mut intensities = vec![0.0f32; width * height];

    for (row, ping) in pings.iter().enumerate() {
        // ── Blanking fill (cross-ping soft-fill for hardware dead zone) ──────
        let filled = if blanking.is_active() {
            let ctx_start = row.saturating_sub(egn::FILL_RADIUS);
            let ctx_end = (row + egn::FILL_RADIUS + 1).min(pings.len());
            let ctx: Vec<&[u16]> = pings[ctx_start..ctx_end]
                .iter()
                .map(|p| p.samples.as_slice())
                .collect();
            egn::fill_blanking_ping(row - ctx_start, &ctx, blanking)
        } else {
            ping.samples.to_vec()
        };
        let start = nadir_skip.min(filled.len());
        let samples = if params.interpolate_gaps {
            interpolate_gaps_u16(&filled[start..], params.gap_threshold_samples)
        } else {
            filled[start..].to_vec()
        };
        let corrected = if tvg_lut.len() >= samples.len() {
            tvg::apply_tvg_lut(&samples, tvg_lut)
        } else {
            tvg::apply_tvg_correction(&samples, params)
        };
        let compressed = compress_row(&corrected, params, floor_db, ceiling_db);

        for col in 0..width {
            let value = if col < compressed.len() {
                compressed[col]
            } else {
                0.0
            };
            intensities[row * width + col] = value;
        }
    }

    Ok(IntermediateFrame {
        intensities,
        width,
        height: pings.len(),
    })
}

/// Build intermediate frame that stitches port + starboard into a butterfly layout.
fn build_intermediate_frame_stitched(
    port: &[&Ping],
    star: &[&Ping],
    row_start: usize,
    row_end: usize,
    side_width: usize,
    height: usize,
    nadir_skip: usize,
    tvg_lut: &[f32],
    params: &SonarProcessingParams,
    floor_db: f32,
    ceiling_db: f32,
    blanking: &egn::BlankingZone,
) -> anyhow::Result<IntermediateFrame> {
    let width = side_width * 2;
    let mut intensities = vec![0.0f32; width * height];

    for (row_idx, src_row) in (row_start..row_end).enumerate() {
        // ── Helper: fill + nadir-trim + TVG for one side's row ───────────────
        let prepare = |pings: &[&Ping], row: usize| -> Vec<f32> {
            if row >= pings.len() {
                return vec![];
            }
            let filled = if blanking.is_active() {
                let ctx_start = row.saturating_sub(egn::FILL_RADIUS);
                let ctx_end = (row + egn::FILL_RADIUS + 1).min(pings.len());
                let ctx: Vec<&[u16]> = pings[ctx_start..ctx_end]
                    .iter()
                    .map(|p| p.samples.as_slice())
                    .collect();
                egn::fill_blanking_ping(row - ctx_start, &ctx, blanking)
            } else {
                pings[row].samples.to_vec()
            };
            let start = nadir_skip.min(filled.len());
            let samples = if params.interpolate_gaps {
                interpolate_gaps_u16(&filled[start..], params.gap_threshold_samples)
            } else {
                filled[start..].to_vec()
            };
            if tvg_lut.len() >= samples.len() {
                tvg::apply_tvg_lut(&samples, tvg_lut)
            } else {
                tvg::apply_tvg_correction(&samples, params)
            }
        };

        // Port (left half, reversed)
        {
            let corrected = prepare(port, src_row);
            let compressed = compress_row(&corrected, params, floor_db, ceiling_db);
            for (col, &v) in compressed.iter().take(side_width).enumerate() {
                let dst_x = side_width - 1 - col;
                intensities[row_idx * width + dst_x] = v;
            }
        }

        // Starboard (right half)
        {
            let corrected = prepare(star, src_row);
            let compressed = compress_row(&corrected, params, floor_db, ceiling_db);
            for (col, &v) in compressed.iter().take(side_width).enumerate() {
                let dst_x = side_width + col;
                intensities[row_idx * width + dst_x] = v;
            }
        }
    }

    Ok(IntermediateFrame {
        intensities,
        width,
        height: row_end - row_start,
    })
}

/// Apply logarithmic compression: 20*log10(value) scaled to [0, 1].
fn log_compress(values: &[f32], floor_db: f32, ceiling_db: f32) -> Vec<f32> {
    const EPSILON: f32 = 1e-10;

    values
        .iter()
        .map(|&v| {
            let db = 20.0 * (v + EPSILON).log10();
            ((db - floor_db) / (ceiling_db - floor_db)).clamp(0.0, 1.0)
        })
        .collect()
}

/// Apply global histogram equalization.
fn histogram_equalize(data: &[f32], width: usize, height: usize) -> Vec<f32> {
    // Build histogram (256 bins)
    let mut hist = [0u32; 256];
    for &val in data {
        let bin = (val * 255.0).clamp(0.0, 255.0) as usize;
        hist[bin] += 1;
    }

    // Compute CDF
    let total = (width * height) as f32;
    let mut cdf = [0.0f32; 256];
    let mut cumsum = 0u32;
    for i in 0..256 {
        cumsum += hist[i];
        cdf[i] = cumsum as f32 / total;
    }

    // Apply equalization
    data.iter()
        .map(|&val| {
            let bin = (val * 255.0).clamp(0.0, 255.0) as usize;
            cdf[bin]
        })
        .collect()
}

/// Compress a TVG-corrected row either with log compression or linear normalization.
fn compress_row(
    values: &[f32],
    params: &SonarProcessingParams,
    floor_db: f32,
    ceiling_db: f32,
) -> Vec<f32> {
    if params.log_compression {
        log_compress(values, floor_db, ceiling_db)
    } else {
        let max_val = values.iter().copied().fold(0.0f32, f32::max).max(1.0);
        values.iter().map(|&v| v / max_val).collect()
    }
}

/// Compute adaptive percentile floor/ceiling across the channels in use.
fn compute_percentiles_combined(
    port: Option<&[&Ping]>,
    star: Option<&[&Ping]>,
    single: Option<&[&Ping]>,
    params: &SonarProcessingParams,
) -> (f32, f32) {
    let mut samples: Vec<f32> = Vec::new();
    let mut push_ping = |ping: &Ping| {
        for &s in &ping.samples {
            samples.push(s as f32);
        }
    };

    if let Some(port) = port {
        for p in port {
            push_ping(p);
        }
    }
    if let Some(star) = star {
        for p in star {
            push_ping(p);
        }
    }
    if let Some(single) = single {
        for p in single {
            push_ping(p);
        }
    }

    if samples.is_empty() {
        return (params.noise_floor_db, params.signal_ceiling_db);
    }

    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let floor_idx = ((params.floor_percentile / 100.0) * samples.len() as f32) as usize;
    let ceiling_idx = ((params.ceiling_percentile / 100.0) * samples.len() as f32) as usize;

    let floor = samples[floor_idx.min(samples.len() - 1)];
    let ceiling = samples[ceiling_idx.min(samples.len() - 1)];
    (floor, ceiling)
}

fn select_sidescan_pair<'a>(
    by_channel: &'a HashMap<u32, Vec<&Ping>>,
) -> (Option<Vec<&'a Ping>>, Option<Vec<&'a Ping>>) {
    // Priority heuristics based on observed captures:
    // 1) If 3+4 present, treat 3=port, 4=starboard.
    // 2) Else if 4+5 present, treat 4=port, 5=starboard.
    // 3) Else if 1+5 present, treat 1=port, 5=starboard.
    // 4) Else pick the top two channels by total sample mass and assign lower id to port.

    let has = |id: u32| by_channel.contains_key(&id);
    if has(3) && has(4) {
        return (Some(by_channel[&3].clone()), Some(by_channel[&4].clone()));
    }
    if has(4) && has(5) {
        return (Some(by_channel[&4].clone()), Some(by_channel[&5].clone()));
    }
    if has(1) && has(5) {
        return (Some(by_channel[&1].clone()), Some(by_channel[&5].clone()));
    }

    // Fallback: choose top two by total sample count
    let mut totals: Vec<(u32, usize)> = by_channel
        .iter()
        .map(|(ch, v)| {
            let total: usize = v.iter().map(|p| p.samples.len()).sum();
            (*ch, total)
        })
        .collect();
    totals.sort_by(|a, b| b.1.cmp(&a.1));
    if totals.len() >= 2 {
        let (a_id, _) = totals[0];
        let (b_id, _) = totals[1];
        let (port_id, star_id) = if a_id <= b_id {
            (a_id, b_id)
        } else {
            (b_id, a_id)
        };
        return (
            Some(by_channel[&port_id].clone()),
            Some(by_channel[&star_id].clone()),
        );
    }
    (None, None)
}

/// Detect the water-column nadir offset: the number of near-field blank samples
/// before the first real bottom return, measured as the median across all pings.
/// Uses the same 512-count noise threshold as the static mosaic renderer.
fn detect_nadir_video(pings: &[&Ping]) -> usize {
    const NOISE_THRESHOLD: u16 = 512;
    if pings.is_empty() {
        return 0;
    }
    let mut skips: Vec<usize> = pings
        .iter()
        .map(|p| {
            p.samples
                .iter()
                .position(|&s| s > NOISE_THRESHOLD)
                .unwrap_or(0)
        })
        .collect();
    skips.sort_unstable();
    skips[skips.len() / 2]
}

/// Optional gap interpolation: linearly fill runs of low/zero samples.
fn interpolate_gaps_u16(samples: &[u16], min_gap: usize) -> Vec<u16> {
    let mut out = samples.to_vec();
    let n = out.len();
    let mut i = 0;
    while i < n {
        // find start of gap
        if out[i] > 2 {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && out[i] <= 2 {
            i += 1;
        }
        let end = i; // exclusive
        let gap_len = end - start;
        if gap_len < min_gap {
            continue;
        }
        // find boundary values
        let prev_val = if start > 0 {
            out[start - 1] as f32
        } else {
            out[end.min(n - 1)] as f32
        };
        let next_val = if end < n { out[end] as f32 } else { prev_val };
        for (k, v) in out[start..end].iter_mut().enumerate() {
            let t = (k as f32 + 1.0) / (gap_len as f32 + 1.0);
            *v = (prev_val + (next_val - prev_val) * t).round().max(0.0) as u16;
        }
    }
    out
}

/// Apply CLAHE (Contrast-Limited Adaptive Histogram Equalization).
///
/// Simplified implementation: divide image into tiles, equalize each tile, interpolate.
fn apply_clahe(
    data: &[f32],
    width: usize,
    height: usize,
    tile_size: usize,
    clip_limit: f32,
) -> Vec<f32> {
    if tile_size == 0 {
        return data.to_vec();
    }

    let tiles_x = (width + tile_size - 1) / tile_size;
    let tiles_y = (height + tile_size - 1) / tile_size;

    // Precompute CDF per tile with clip limit
    let mut cdfs: Vec<[f32; 256]> = Vec::with_capacity(tiles_x * tiles_y);
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let mut hist = [0u32; 256];
            let x0 = tx * tile_size;
            let y0 = ty * tile_size;
            let x1 = (x0 + tile_size).min(width);
            let y1 = (y0 + tile_size).min(height);

            for y in y0..y1 {
                let row = &data[y * width..(y + 1) * width];
                for &v in &row[x0..x1] {
                    let bin = (v * 255.0).clamp(0.0, 255.0) as usize;
                    hist[bin] += 1;
                }
            }

            // Clip histogram
            let clip_max = (clip_limit * ((x1 - x0) * (y1 - y0)) as f32 / 256.0).max(1.0) as u32;
            let mut excess = 0u32;
            for h in hist.iter_mut() {
                if *h > clip_max {
                    excess += *h - clip_max;
                    *h = clip_max;
                }
            }
            // Redistribute excess uniformly
            let increment = excess / 256;
            let remainder = excess % 256;
            for (i, h) in hist.iter_mut().enumerate() {
                *h += increment + if i < remainder as usize { 1 } else { 0 };
            }

            // Compute CDF
            let mut cdf = [0f32; 256];
            let mut cumsum = 0u32;
            let total = ((x1 - x0) * (y1 - y0)) as f32;
            for i in 0..256 {
                cumsum += hist[i];
                cdf[i] = cumsum as f32 / total;
            }
            cdfs.push(cdf);
        }
    }

    // Interpolate CDFs bilinearly per pixel
    let mut out = vec![0f32; data.len()];
    for y in 0..height {
        for x in 0..width {
            let gx = x as f32 / tile_size as f32;
            let gy = y as f32 / tile_size as f32;
            let tx0 = gx.floor() as usize;
            let ty0 = gy.floor() as usize;
            let tx1 = (tx0 + 1).min(tiles_x - 1);
            let ty1 = (ty0 + 1).min(tiles_y - 1);
            let dx = gx - tx0 as f32;
            let dy = gy - ty0 as f32;

            let idx = |tx: usize, ty: usize| -> &[f32; 256] { &cdfs[ty * tiles_x + tx] };

            let val = data[y * width + x];
            let bin = (val * 255.0).clamp(0.0, 255.0) as usize;

            let c00 = idx(tx0, ty0)[bin];
            let c10 = idx(tx1, ty0)[bin];
            let c01 = idx(tx0, ty1)[bin];
            let c11 = idx(tx1, ty1)[bin];

            let c0 = c00 * (1.0 - dx) + c10 * dx;
            let c1 = c01 * (1.0 - dx) + c11 * dx;
            let c = c0 * (1.0 - dy) + c1 * dy;
            out[y * width + x] = c;
        }
    }

    out
}

/// Apply colormap LUT to normalized intensities.
fn apply_colormap(intensities: &[f32], lut: &ColorLUT) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(intensities.len() * 3);

    for &intensity in intensities {
        let idx = (intensity * 255.0).clamp(0.0, 255.0) as usize;
        let (r, g, b) = lut[idx.min(255)];
        rgb.push(r);
        rgb.push(g);
        rgb.push(b);
    }

    rgb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_compress() {
        let values = vec![1.0, 10.0, 100.0, 1000.0];
        let compressed = log_compress(&values, -60.0, 60.0);

        // Should be monotonically increasing
        assert!(compressed[0] < compressed[1]);
        assert!(compressed[1] < compressed[2]);
        assert!(compressed[2] < compressed[3]);

        // Should be in [0, 1]
        for &v in &compressed {
            assert!(v >= 0.0 && v <= 1.0);
        }
    }

    #[test]
    fn test_histogram_equalize() {
        // Uniform distribution should remain relatively uniform
        let data: Vec<f32> = (0..256).map(|i| i as f32 / 255.0).collect();
        let eq = histogram_equalize(&data, 16, 16);

        // Check range
        let min = eq.iter().copied().fold(f32::MAX, f32::min);
        let max = eq.iter().copied().fold(f32::MIN, f32::max);
        assert!(min >= 0.0 && max <= 1.0);
    }
}
