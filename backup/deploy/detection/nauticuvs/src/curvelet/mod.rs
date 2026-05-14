//! Curvelet transform — forward and inverse passes.

pub mod coefficient_store;
pub mod phase;

pub use coefficient_store::{CoefficientStore, CurveletError, ScaleIndexError, Subband};

use crate::geo::{GeoTiffInput, GeoTransform};
use crate::precision::Scalar;
use crate::weights::{DirectionalMask, RichardsonWeighter};
use ndarray::Array2;

/// Primary entry point — backward-compatible with the existing aeromagnetic worker.
///
/// Computes the Fast Discrete Curvelet Transform (FDCT) of a 2-D grid.
///
/// # Arguments
/// * `grid`   — input 2-D array of scalar values
/// * `scales` — number of curvelet scales (must be ≥ 1)
///
/// # Returns
/// A `CoefficientStore` with `detail`, `fine`, and `coarse` fields.
pub fn curvelet_forward(
    grid: &Array2<Scalar>,
    scales: usize,
) -> Result<CoefficientStore, CurveletError> {
    if grid.nrows() == 0 || grid.ncols() == 0 {
        return Err(CurveletError::ZeroDimension);
    }
    if scales == 0 {
        return Err(CurveletError::InvalidScaleCount);
    }
    forward_impl(grid, scales, None)
}

/// Entry point that accepts a `GeoTiffInput` and propagates CRS metadata.
///
/// Identical to `curvelet_forward` but attaches the `GeoTransform` from the
/// input to the returned `CoefficientStore`, so every detected anomaly can be
/// mapped back to an exact GPS coordinate.
pub fn curvelet_forward_geo(
    input: &GeoTiffInput,
    scales: usize,
) -> Result<CoefficientStore, CurveletError> {
    if input.pixels.nrows() == 0 || input.pixels.ncols() == 0 {
        return Err(CurveletError::ZeroDimension);
    }
    if scales == 0 {
        return Err(CurveletError::InvalidScaleCount);
    }
    forward_impl(&input.pixels, scales, Some(input.geo_transform.clone()))
}

/// Inverse curvelet transform with optional directional mask and Richardson weighting.
///
/// # Arguments
/// * `store`    — coefficient store from a prior `curvelet_forward` call
/// * `mask`     — optional directional mask applied per (scale, angle) during reconstruction
/// * `weighter` — optional Richardson Number weighter applied per depth layer
///
/// When both `mask` and `weighter` are supplied, the combined weight is:
/// `clamp(mask.weight(scale, angle), 0.0, 1.0) * weighter.weight_for_depth(depth)`
pub fn curvelet_inverse(
    store: &CoefficientStore,
    mask: Option<&dyn DirectionalMask>,
    weighter: Option<&RichardsonWeighter>,
) -> Result<Array2<Scalar>, CurveletError> {
    inverse_impl(store, mask, weighter)
}

// ── Internal implementation ───────────────────────────────────────────────────

fn forward_impl(
    grid: &Array2<Scalar>,
    scales: usize,
    geo_transform: Option<GeoTransform>,
) -> Result<CoefficientStore, CurveletError> {
    use crate::fft_backend::{ActiveBackend, FftBackend as _};
    use crate::fdct_kernels::{tile, window, wrap};
    use crate::precision::C;
    use aligned_vec::AVec;
    use num_complex::Complex;

    let rows = grid.nrows();
    let cols = grid.ncols();
    let n = rows * cols;

    // Step 1: 2-D FFT of the input grid.
    let input_flat: Vec<C> = grid.iter()
        .map(|&v| Complex::new(v, 0.0 as Scalar))
        .collect();
    let mut freq_plane = vec![C::default(); n];
    ActiveBackend::fft2d(&input_flat, rows, cols, &mut freq_plane);

    // Step 2: Build the wedge table for all (scale, angle) pairs.
    let wedge_table = window::build_wedge_table(rows, cols, scales);

    // Step 3: For each (scale, angle), window → wrap → IFFT → store as Subband.
    let mut detail: Vec<Vec<Subband>> = Vec::with_capacity(scales);

    for scale_idx in 0..scales {
        let wedges = &wedge_table[scale_idx];
        let mut scale_subbands: Vec<Subband> = Vec::with_capacity(wedges.len());

        for wedge_params in wedges {
            let dst_rows = wedge_params.canonical_rows;
            let dst_cols = wedge_params.canonical_cols;
            let dst_n = dst_rows * dst_cols;

            // Window the frequency-domain wedge.
            let mut windowed = vec![C::default(); n];
            window::apply_wedge_window(
                &freq_plane, rows, cols,
                &mut windowed,
                wedge_params,
            );

            // Wrap to canonical rectangle.
            let mut wrapped = vec![C::default(); dst_n];
            wrap::wrap_to_canonical(
                &windowed, rows, cols,
                &mut wrapped, dst_rows, dst_cols,
                &wedge_params.wrap_offsets,
            );

            // 2-D IFFT of the wrapped rectangle.
            let mut coeff_data = vec![C::default(); dst_n];
            let mut scratch = vec![C::default(); dst_n];
            tile::ifft_tile(&wrapped, dst_rows, dst_cols, &mut coeff_data, &mut scratch);

            // Store as an aligned Subband.
            let mut aligned_data: AVec<C, aligned_vec::ConstAlign<64>> =
                AVec::with_capacity(64, dst_n);
            for v in &coeff_data { aligned_data.push(*v); }

            scale_subbands.push(Subband {
                data: aligned_data,
                rows: dst_rows,
                cols: dst_cols,
                scale: scale_idx,
                angle_deg: wedge_params.angle_center_deg,
            });
        }
        detail.push(scale_subbands);
    }

    // Step 4: Coarse scale — low-frequency residual (DC component region).
    let coarse_n = rows.min(8) * cols.min(8); // small DC region
    let coarse_flat: Vec<C> = freq_plane[..coarse_n.min(n)].to_vec();
    let mut coarse_aligned: AVec<C, aligned_vec::ConstAlign<64>> =
        AVec::with_capacity(64, coarse_flat.len());
    for v in &coarse_flat { coarse_aligned.push(*v); }
    let coarse = Subband {
        data: coarse_aligned,
        rows: rows.min(8),
        cols: cols.min(8),
        scale: scales,
        angle_deg: 0.0,
    };

    // Step 5: Fine scale — highest-frequency coefficients (last scale's first subband).
    let fine: Vec<C> = if let Some(last_scale) = detail.last() {
        if let Some(first_sub) = last_scale.first() {
            first_sub.data.iter().copied().collect()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    Ok(CoefficientStore {
        coarse,
        detail,
        fine,
        geo_transform,
        num_scales: scales,
    })
}

fn inverse_impl(
    store: &CoefficientStore,
    mask: Option<&dyn DirectionalMask>,
    weighter: Option<&RichardsonWeighter>,
) -> Result<Array2<Scalar>, CurveletError> {
    use crate::fft_backend::{ActiveBackend, FftBackend as _};
    use crate::precision::C;
    use num_complex::Complex;

    // Determine output dimensions from the coarse subband.
    // The coarse subband stores the DC region; we need the original grid size.
    // We reconstruct by accumulating adjoint contributions from all subbands.
    // For now, infer size from the largest subband in detail[0].
    let (rows, cols) = infer_grid_size(store);
    let n = rows * cols;

    let mut accum = vec![C::default(); n];

    for (scale_idx, scale_subbands) in store.detail.iter().enumerate() {
        for subband in scale_subbands {
            // Compute combined weight for this (scale, angle).
            let dir_weight = mask.map_or(1.0 as Scalar, |m| {
                m.weight(scale_idx, subband.angle_deg).clamp(0.0, 1.0)
            });
            let rich_weight = weighter.map_or(1.0 as Scalar, |w| {
                // Use the subband's scale as a proxy for depth (scale 0 = surface).
                let depth_m = scale_idx as f64 * 10.0;
                w.weight_for_depth(depth_m)
            });
            let combined = dir_weight * rich_weight;

            if combined == 0.0 {
                continue;
            }

            // Adjoint: FFT the subband coefficients back to frequency domain.
            let sub_n = subband.rows * subband.cols;
            let mut freq_sub = vec![C::default(); sub_n];
            let mut scratch = vec![C::default(); sub_n];
            crate::fdct_kernels::tile::fft_tile(
                subband.data.as_slice(), subband.rows, subband.cols,
                &mut freq_sub, &mut scratch,
            );

            // Unwrap from canonical rectangle back to full frequency plane.
            let mut unwrapped = vec![C::default(); n];
            crate::fdct_kernels::wrap::unwrap_from_canonical(
                &freq_sub, subband.rows, subband.cols,
                &mut unwrapped, rows, cols,
                &crate::fdct_kernels::window::get_wrap_offsets(
                    rows, cols, scale_idx, subband.angle_deg,
                ),
            );

            // Apply window and weight, accumulate.
            let wedge_params = crate::fdct_kernels::window::WedgeParams {
                scale: scale_idx,
                angle_idx: 0,
                num_angles: scale_subbands.len(),
                freq_lo: 0.0,
                freq_hi: 1.0,
                angle_lo_deg: subband.angle_deg - 15.0,
                angle_hi_deg: subband.angle_deg + 15.0,
                angle_center_deg: subband.angle_deg,
                canonical_rows: subband.rows,
                canonical_cols: subband.cols,
                wrap_offsets: Vec::new(),
            };
            let mut windowed = vec![C::default(); n];
            crate::fdct_kernels::window::apply_wedge_window(
                &unwrapped, rows, cols, &mut windowed, &wedge_params,
            );

            for (a, w) in accum.iter_mut().zip(windowed.iter()) {
                *a = *a + *w * Complex::new(combined, 0.0);
            }
        }
    }

    // IFFT the accumulated frequency-domain result.
    let mut spatial = vec![C::default(); n];
    ActiveBackend::ifft2d(&accum, rows, cols, &mut spatial);

    // Extract real parts into the output array.
    let real_data: Vec<Scalar> = spatial.iter().map(|c| c.re).collect();
    Array2::from_shape_vec((rows, cols), real_data)
        .map_err(|e| CurveletError::FftError(e.to_string()))
}

/// Infer the original grid dimensions from the coefficient store.
fn infer_grid_size(store: &CoefficientStore) -> (usize, usize) {
    // The coarse subband is a small DC region; the detail subbands are canonical
    // rectangles. We use the coarse subband's size scaled up by the number of scales
    // as a heuristic. In practice, workers should store the original size.
    let scale_factor = 1usize << store.num_scales;
    let rows = store.coarse.rows * scale_factor;
    let cols = store.coarse.cols * scale_factor;
    (rows.max(8), cols.max(8))
}


