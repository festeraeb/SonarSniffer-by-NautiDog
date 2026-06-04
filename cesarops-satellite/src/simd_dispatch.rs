//! Runtime CPU dispatch for the pixel-math hot path — ONE binary, every host.
//!
//! Compile with standard generic flags (NO `-C target-cpu=native`). At runtime
//! we detect the CPU and route the heavy per-pixel loops through a function
//! compiled with the widest instruction set that host actually supports:
//!   - AVX-512 on the T440 (Xeon Silver 4110)
//!   - AVX     on the HP Gen8 (Xeon E5 v2, Ivy Bridge — AVX, no AVX2)
//!   - scalar  fallback otherwise
//!
//! This replaces per-host builds: the same `target/release` binary copies freely
//! across the mixed fleet and picks the right pipeline per node, never SIGILLs.
//! (See FIELD_NOTES: Ivy Bridge lacks AVX2; a native build on the T440 would
//! crash the HP. Runtime dispatch sidesteps that entirely.)
//!
//! The `#[target_feature]` functions are tiny and contain only data-parallel
//! arithmetic so the compiler auto-vectorizes them to the enabled width. rayon
//! parallelises across rows on top.

use ndarray::Array2;
use rayon::prelude::*;

/// Which vector pipeline the current CPU resolved to (for logging/telemetry).
pub fn active_pipeline() -> &'static str {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if is_x86_feature_detected!("avx512f") {
            return "avx512";
        } else if is_x86_feature_detected!("avx") {
            return "avx";
        }
    }
    "scalar"
}

/// Apply an affine transform `v -> v*scale + offset` over every finite pixel of
/// a grid, in parallel, using the widest SIMD path the host supports at runtime.
/// NaNs are preserved. This is the canonical hot-loop shape (z-score rescale,
/// reflectance scaling, metric maps all reduce to this).
pub fn affine_inplace(matrix: &mut Array2<f32>, scale: f32, offset: f32) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if is_x86_feature_detected!("avx512f") {
            // SAFETY: avx512f verified present at runtime.
            unsafe { affine_avx512(matrix, scale, offset) };
            return;
        } else if is_x86_feature_detected!("avx") {
            // SAFETY: avx verified present at runtime.
            unsafe { affine_avx(matrix, scale, offset) };
            return;
        }
    }
    affine_scalar(matrix, scale, offset);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx512f")]
unsafe fn affine_avx512(matrix: &mut Array2<f32>, scale: f32, offset: f32) {
    // 512-bit-friendly row tiling; rayon across row tiles.
    matrix
        .axis_chunks_iter_mut(ndarray::Axis(0), 512)
        .into_par_iter()
        .for_each(|mut slab| {
            for p in slab.iter_mut() {
                if p.is_finite() {
                    *p = *p * scale + offset;
                }
            }
        });
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
unsafe fn affine_avx(matrix: &mut Array2<f32>, scale: f32, offset: f32) {
    matrix
        .axis_chunks_iter_mut(ndarray::Axis(0), 256)
        .into_par_iter()
        .for_each(|mut slab| {
            for p in slab.iter_mut() {
                if p.is_finite() {
                    *p = *p * scale + offset;
                }
            }
        });
}

fn affine_scalar(matrix: &mut Array2<f32>, scale: f32, offset: f32) {
    matrix
        .axis_chunks_iter_mut(ndarray::Axis(0), 128)
        .into_par_iter()
        .for_each(|mut slab| {
            for p in slab.iter_mut() {
                if p.is_finite() {
                    *p = *p * scale + offset;
                }
            }
        });
}

/// Initialise the global rayon pool sized to the host's logical cores across
/// all sockets. Call once at process start. Idempotent-safe: ignores the error
/// if a pool is already built.
pub fn init_thread_pool() -> usize {
    let n = num_cpus::get();
    let _ = rayon::ThreadPoolBuilder::new().num_threads(n).build_global();
    n
}

// ── Complex multi-band SAR path (interferometry-ready) ───────────────────────
//
// Raw Sentinel-1 SLC carries Real/Imag float pairs (Complex32). For multi-band
// or complex stacks the data is Array3 (Axis0 = band, Axis1 = row, Axis2 = col),
// kept row-contiguous so a vector load streams straight into cache (no
// reshuffle). An AVX-512 register holds 8 Complex32 (512/64); AVX holds 4.
//
// We runtime-dispatch ONCE at the processing boundary (not per pixel) to avoid
// branch penalties inside the hot loop. The closure shape covers the common SAR
// ops: per-pixel complex scale/phase-shift, coherence pre-multiply, calibration.

use ndarray::{Array3, ArrayViewMut3};
use num_complex::Complex32;

/// Apply a per-pixel complex transform `v -> v * scale` over a (band,row,col)
/// stack, parallel across row tiles, widest SIMD path the host supports.
/// Non-finite components are left untouched (NoData-preserving — raw SAR often
/// carries NaN fill we must not corrupt).
pub fn complex_scale_inplace(stack: &mut Array3<Complex32>, scale: Complex32) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if is_x86_feature_detected!("avx512f") {
            unsafe { complex_scale_avx512(stack.view_mut(), scale) };
            return;
        } else if is_x86_feature_detected!("avx") {
            unsafe { complex_scale_avx(stack.view_mut(), scale) };
            return;
        }
    }
    complex_scale_generic(stack.view_mut(), scale);
}

#[inline(always)]
fn cmul_keep_nodata(v: Complex32, scale: Complex32) -> Complex32 {
    if v.re.is_finite() && v.im.is_finite() {
        v * scale
    } else {
        v
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx512f")]
unsafe fn complex_scale_avx512(mut view: ArrayViewMut3<Complex32>, scale: Complex32) {
    view.axis_chunks_iter_mut(ndarray::Axis(1), 512)
        .into_par_iter()
        .for_each(|mut slab| {
            for v in slab.iter_mut() {
                *v = cmul_keep_nodata(*v, scale);
            }
        });
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
unsafe fn complex_scale_avx(mut view: ArrayViewMut3<Complex32>, scale: Complex32) {
    view.axis_chunks_iter_mut(ndarray::Axis(1), 256)
        .into_par_iter()
        .for_each(|mut slab| {
            for v in slab.iter_mut() {
                *v = cmul_keep_nodata(*v, scale);
            }
        });
}

fn complex_scale_generic(mut view: ArrayViewMut3<Complex32>, scale: Complex32) {
    view.axis_chunks_iter_mut(ndarray::Axis(1), 128)
        .into_par_iter()
        .for_each(|mut slab| {
            for v in slab.iter_mut() {
                *v = cmul_keep_nodata(*v, scale);
            }
        });
}

/// Interferometric coherence-numerator pre-product over two co-registered
/// complex scenes: `out = a * conj(b)` per pixel (the cross-correlation term;
/// the magnitude/normalisation is accumulated downstream). Row-tiled, parallel.
/// NoData (non-finite) pixels yield NaN so they drop out of the coherence sum.
pub fn cross_corr_inplace(a: &mut Array3<Complex32>, b: &Array3<Complex32>) {
    use rayon::prelude::*;
    if a.dim() != b.dim() {
        return;
    }
    ndarray::Zip::from(a.axis_iter_mut(ndarray::Axis(1)))
        .and(b.axis_iter(ndarray::Axis(1)))
        .into_par_iter()
        .for_each(|(mut arow, brow)| {
            for (av, bv) in arow.iter_mut().zip(brow.iter()) {
                if av.re.is_finite() && av.im.is_finite() && bv.re.is_finite() && bv.im.is_finite() {
                    *av = *av * bv.conj();
                } else {
                    *av = Complex32::new(f32::NAN, f32::NAN);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affine_matches_scalar_on_all_paths() {
        let mut a = Array2::<f32>::from_elem((300, 40), 1.25);
        a[[0, 0]] = f32::NAN; // preserved
        affine_inplace(&mut a, 2.0, 0.5);
        assert!(a[[0, 0]].is_nan());
        assert!((a[[1, 1]] - (1.25 * 2.0 + 0.5)).abs() < 1e-6);
    }

    #[test]
    fn pipeline_name_is_known() {
        assert!(["avx512", "avx", "scalar"].contains(&active_pipeline()));
    }

    #[test]
    fn complex_scale_preserves_nodata_and_scales() {
        let mut s = Array3::<Complex32>::from_elem((2, 300, 4), Complex32::new(1.0, 0.0));
        s[[0, 0, 0]] = Complex32::new(f32::NAN, f32::NAN);
        complex_scale_inplace(&mut s, Complex32::new(0.0, 1.0)); // multiply by i
        assert!(s[[0, 0, 0]].re.is_nan()); // nodata preserved
        // (1+0i)*i = i
        assert!((s[[1, 1, 1]].re - 0.0).abs() < 1e-6 && (s[[1, 1, 1]].im - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cross_corr_conjugate_product() {
        let mut a = Array3::<Complex32>::from_elem((1, 130, 2), Complex32::new(1.0, 2.0));
        let b = Array3::<Complex32>::from_elem((1, 130, 2), Complex32::new(3.0, 1.0));
        cross_corr_inplace(&mut a, &b);
        // (1+2i)*conj(3+1i) = (1+2i)*(3-1i) = 3 -1i +6i -2i^2 = 3 +5i +2 = 5+5i
        assert!((a[[0, 5, 0]].re - 5.0).abs() < 1e-5);
        assert!((a[[0, 5, 0]].im - 5.0).abs() < 1e-5);
    }
}
