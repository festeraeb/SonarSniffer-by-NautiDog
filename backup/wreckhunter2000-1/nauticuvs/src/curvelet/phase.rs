//! Phase and amplitude extraction from curvelet coefficients.

use ndarray::Array2;
use crate::curvelet::coefficient_store::{CoefficientStore, ScaleIndexError};
use crate::precision::Scalar;

/// Phase angle (radians, range [−π, π]) for every coefficient in `scale`.
pub fn phase_map(store: &CoefficientStore, scale: usize) -> Result<Array2<Scalar>, ScaleIndexError> {
    let subbands = store.detail.get(scale).ok_or(ScaleIndexError {
        index: scale,
        num_scales: store.num_scales,
    })?;

    // Aggregate all subbands at this scale into a single 2-D phase map.
    // Use the first subband's dimensions as the canonical shape.
    if subbands.is_empty() {
        return Ok(Array2::zeros((0, 0)));
    }
    let sub = &subbands[0];
    let data: Vec<Scalar> = sub.data.iter()
        .map(|c| c.im.atan2(c.re))
        .collect();
    Array2::from_shape_vec((sub.rows, sub.cols), data)
        .map_err(|_| ScaleIndexError { index: scale, num_scales: store.num_scales })
}

/// Modulus of every coefficient in `scale`.
///
/// The returned array has the same shape as `phase_map(scale)`.
pub fn amplitude_map(store: &CoefficientStore, scale: usize) -> Result<Array2<Scalar>, ScaleIndexError> {
    let subbands = store.detail.get(scale).ok_or(ScaleIndexError {
        index: scale,
        num_scales: store.num_scales,
    })?;

    if subbands.is_empty() {
        return Ok(Array2::zeros((0, 0)));
    }
    let sub = &subbands[0];
    let data: Vec<Scalar> = sub.data.iter()
        .map(|c| c.norm())
        .collect();
    Array2::from_shape_vec((sub.rows, sub.cols), data)
        .map_err(|_| ScaleIndexError { index: scale, num_scales: store.num_scales })
}

/// Local phase coherence (mean resultant length) in a sliding window.
///
/// For each pixel, computes the mean resultant length of the phase vectors
/// within a square window of side `2 * window_radius + 1`. Values are in [0.0, 1.0].
pub fn phase_coherence(
    store: &CoefficientStore,
    scale: usize,
    window_radius: usize,
) -> Result<Array2<Scalar>, ScaleIndexError> {
    let phase = phase_map(store, scale)?;
    let (rows, cols) = (phase.nrows(), phase.ncols());
    let mut result = Array2::<Scalar>::zeros((rows, cols));
    let r = window_radius as isize;

    for i in 0..rows {
        for j in 0..cols {
            let mut sum_cos = 0.0_f64;
            let mut sum_sin = 0.0_f64;
            let mut count = 0usize;

            for di in -r..=r {
                for dj in -r..=r {
                    let ni = i as isize + di;
                    let nj = j as isize + dj;
                    if ni >= 0 && ni < rows as isize && nj >= 0 && nj < cols as isize {
                        let phi = phase[[ni as usize, nj as usize]] as f64;
                        sum_cos += phi.cos();
                        sum_sin += phi.sin();
                        count += 1;
                    }
                }
            }

            if count > 0 {
                let mrl = ((sum_cos / count as f64).powi(2)
                    + (sum_sin / count as f64).powi(2))
                    .sqrt();
                result[[i, j]] = mrl.clamp(0.0, 1.0) as Scalar;
            }
        }
    }

    Ok(result)
}

/// Deterministic FNV-1a hash of all coefficient values.
///
/// Traverses `coarse`, then `detail` (scale-major, angle-minor), then `fine`
/// in a fixed order. The hash is over the raw bytes of the complex coefficients.
pub fn checksum(store: &CoefficientStore) -> u64 {
    const FNV_OFFSET: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;

    let mut hash = FNV_OFFSET;

    let hash_bytes = |hash: &mut u64, bytes: &[u8]| {
        for &b in bytes {
            *hash ^= b as u64;
            *hash = hash.wrapping_mul(FNV_PRIME);
        }
    };

    // Hash coarse subband.
    let coarse_bytes = bytemuck_cast_slice(store.coarse.data.as_slice());
    hash_bytes(&mut hash, coarse_bytes);

    // Hash detail subbands in scale-major, angle-minor order.
    for scale_subbands in &store.detail {
        for subband in scale_subbands {
            let bytes = bytemuck_cast_slice(subband.data.as_slice());
            hash_bytes(&mut hash, bytes);
        }
    }

    // Hash fine coefficients.
    let fine_bytes = bytemuck_cast_slice(&store.fine);
    hash_bytes(&mut hash, fine_bytes);

    hash
}

/// Cast a slice of complex numbers to a byte slice for hashing.
fn bytemuck_cast_slice<T: Copy>(data: &[T]) -> &[u8] {
    let ptr = data.as_ptr() as *const u8;
    let len = data.len() * std::mem::size_of::<T>();
    // SAFETY: T is Copy (plain data), we're reading its bytes.
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex;

    fn make_store_with_one_subband(rows: usize, cols: usize) -> CoefficientStore {
        use aligned_vec::AVec;
        use crate::curvelet::coefficient_store::Subband;
        use crate::geo::GeoTransform;

        let n = rows * cols;
        let mut data: AVec<Complex<Scalar>, aligned_vec::ConstAlign<64>> =
            AVec::with_capacity(64, n);
        for i in 0..n {
            data.push(Complex::new(i as Scalar * 0.1, i as Scalar * 0.05));
        }
        let sub = Subband { data: data.clone(), rows, cols, scale: 0, angle_deg: 45.0 };
        let coarse_data: AVec<Complex<Scalar>, aligned_vec::ConstAlign<64>> =
            AVec::with_capacity(64, 1);
        let coarse = Subband { data: coarse_data, rows: 1, cols: 1, scale: 1, angle_deg: 0.0 };

        CoefficientStore {
            coarse,
            detail: vec![vec![sub]],
            fine: vec![Complex::new(1.0, 0.0)],
            geo_transform: None,
            num_scales: 1,
        }
    }

    #[test]
    fn phase_map_range() {
        let store = make_store_with_one_subband(4, 4);
        let phases = store.phase_map(0).unwrap();
        for &p in phases.iter() {
            assert!(p >= -std::f32::consts::PI as Scalar - 1e-5);
            assert!(p <= std::f32::consts::PI as Scalar + 1e-5);
        }
    }

    #[test]
    fn phase_equals_atan2() {
        let store = make_store_with_one_subband(4, 4);
        let phases = store.phase_map(0).unwrap();
        let sub = &store.detail[0][0];
        for (i, (&p, c)) in phases.iter().zip(sub.data.iter()).enumerate() {
            let expected = c.im.atan2(c.re);
            let diff = (p - expected).abs();
            assert!(diff < 1e-4, "phase mismatch at {}: got {}, expected {}", i, p, expected);
        }
    }

    #[test]
    fn amplitude_and_phase_same_shape() {
        let store = make_store_with_one_subband(4, 4);
        let phases = store.phase_map(0).unwrap();
        let amps = store.amplitude_map(0).unwrap();
        assert_eq!(phases.shape(), amps.shape());
    }

    #[test]
    fn phase_coherence_in_range() {
        let store = make_store_with_one_subband(8, 8);
        let coh = store.phase_coherence(0, 2).unwrap();
        for &v in coh.iter() {
            assert!(v >= 0.0 && v <= 1.0, "coherence out of range: {}", v);
        }
    }

    #[test]
    fn checksum_is_deterministic() {
        let store = make_store_with_one_subband(4, 4);
        assert_eq!(store.checksum(), store.checksum());
    }

    #[test]
    fn out_of_bounds_scale_returns_error() {
        let store = make_store_with_one_subband(4, 4);
        assert!(store.phase_map(99).is_err());
        assert!(store.amplitude_map(99).is_err());
        assert!(store.phase_coherence(99, 1).is_err());
    }
}
