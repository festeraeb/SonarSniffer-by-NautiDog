//! Core data structures: `Subband` and `CoefficientStore`.

use aligned_vec::AVec;
use ndarray::Array2;
use num_complex::Complex;
use serde::{Deserialize, Serialize};

use crate::geo::GeoTransform;
use crate::precision::Scalar;

// ── Compile-time alignment assertion ─────────────────────────────────────────

// Subband uses #[repr(C, align(64))] to guarantee 64-byte alignment on the stack.
// The heap buffer (AlignedVec) is separately guaranteed to be 64-byte aligned.
const _SUBBAND_ALIGN: () = assert!(std::mem::align_of::<Subband>() >= 64);

// ── Subband ───────────────────────────────────────────────────────────────────

/// A single directional frequency band produced by the FDCT.
///
/// The coefficient buffer is allocated on a 64-byte aligned boundary so that
/// it can be passed directly to SIMD intrinsics and wgpu compute shaders
/// without additional copying or alignment fixups.
#[repr(C, align(64))]
pub struct Subband {
    /// Flat buffer of complex coefficients in row-major order.
    /// Length = `rows * cols`. 64-byte aligned.
    pub(crate) data: AVec<Complex<Scalar>, aligned_vec::ConstAlign<64>>,
    /// Number of rows in the canonical rectangle.
    pub rows: usize,
    /// Number of columns in the canonical rectangle.
    pub cols: usize,
    /// Scale index (0 = finest detail, num_scales-1 = coarsest detail).
    pub scale: usize,
    /// Centre angle of this wedge in degrees.
    pub angle_deg: f64,
}

impl Subband {
    /// Iterate over all complex coefficients.
    pub fn iter(&self) -> impl Iterator<Item = &Complex<Scalar>> {
        self.data.iter()
    }

    /// Return the coefficient at (row, col).
    pub fn get(&self, row: usize, col: usize) -> Option<&Complex<Scalar>> {
        self.data.get(row * self.cols + col)
    }

    /// Return the raw slice of complex coefficients.
    pub fn as_slice(&self) -> &[Complex<Scalar>] {
        self.data.as_slice()
    }

    /// Verify the buffer pointer is 64-byte aligned.
    pub fn is_aligned(&self) -> bool {
        self.data.as_ptr() as usize % 64 == 0
    }
}

// Manual Clone because AVec doesn't derive Clone automatically in all versions.
impl Clone for Subband {
    fn clone(&self) -> Self {
        let mut new_data: AVec<Complex<Scalar>, aligned_vec::ConstAlign<64>> =
            AVec::with_capacity(64, self.data.len());
        for v in self.data.iter() { new_data.push(*v); }
        Self {
            data: new_data,
            rows: self.rows,
            cols: self.cols,
            scale: self.scale,
            angle_deg: self.angle_deg,
        }
    }
}

impl std::fmt::Debug for Subband {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subband")
            .field("rows", &self.rows)
            .field("cols", &self.cols)
            .field("scale", &self.scale)
            .field("angle_deg", &self.angle_deg)
            .field("len", &self.data.len())
            .field("aligned", &self.is_aligned())
            .finish()
    }
}

// Serde for Subband: serialise data as Vec<[Scalar; 2]> (real, imag pairs).
impl Serialize for Subband {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let pairs: Vec<[Scalar; 2]> = self.data.iter()
            .map(|c| [c.re, c.im])
            .collect();
        let mut st = s.serialize_struct("Subband", 5)?;
        st.serialize_field("data", &pairs)?;
        st.serialize_field("rows", &self.rows)?;
        st.serialize_field("cols", &self.cols)?;
        st.serialize_field("scale", &self.scale)?;
        st.serialize_field("angle_deg", &self.angle_deg)?;
        st.end()
    }
}

impl<'de> Deserialize<'de> for Subband {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct SubbandHelper {
            data: Vec<[Scalar; 2]>,
            rows: usize,
            cols: usize,
            scale: usize,
            angle_deg: f64,
        }
        let h = SubbandHelper::deserialize(d)?;
        let mut aligned: AVec<Complex<Scalar>, aligned_vec::ConstAlign<64>> =
            AVec::with_capacity(64, h.data.len());
        for pair in &h.data {
            aligned.push(Complex::new(pair[0], pair[1]));
        }
        Ok(Subband {
            data: aligned,
            rows: h.rows,
            cols: h.cols,
            scale: h.scale,
            angle_deg: h.angle_deg,
        })
    }
}

// ── CoefficientStore ──────────────────────────────────────────────────────────

/// Holds all subbands for one FDCT forward pass.
///
/// Field names (`detail`, `fine`, `coarse`) are frozen — changing them would
/// break the `cesarops-aeromagnetic-worker` without a compile error in this crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoefficientStore {
    /// Coarse scale (low-frequency residual). Single subband.
    pub coarse: Subband,

    /// Detail scales: `detail[s][a]` is the subband for scale `s`, angle `a`.
    ///
    /// Matches the aeromagnetic worker's `coeffs.detail[s][a].iter().map(|c| c.norm_sqr())`
    /// access pattern.
    pub detail: Vec<Vec<Subband>>,

    /// Fine scale (highest frequency). Flat list of complex coefficients.
    ///
    /// Matches the aeromagnetic worker's `coeffs.fine.iter().map(|c| c.norm_sqr())`
    /// access pattern.
    pub fine: Vec<Complex<Scalar>>,

    /// CRS metadata propagated from the input `GeoTiffInput`, if present.
    /// Allows any coefficient's spatial origin to be recovered as a GPS coordinate.
    pub geo_transform: Option<GeoTransform>,

    /// Number of scales used in the forward pass.
    pub num_scales: usize,
}

impl CoefficientStore {
    /// Phase angle (radians, range [−π, π]) for every coefficient in `scale`.
    ///
    /// Returns `ScaleIndexError` if `scale` is out of bounds.
    pub fn phase_map(&self, scale: usize) -> Result<Array2<Scalar>, ScaleIndexError> {
        crate::curvelet::phase::phase_map(self, scale)
    }

    /// Modulus of every coefficient in `scale`.
    ///
    /// The returned array has the same shape as `phase_map(scale)`.
    pub fn amplitude_map(&self, scale: usize) -> Result<Array2<Scalar>, ScaleIndexError> {
        crate::curvelet::phase::amplitude_map(self, scale)
    }

    /// Local phase coherence (mean resultant length) in a sliding window.
    ///
    /// Returns values in [0.0, 1.0]. A value near 1.0 indicates highly coherent
    /// phase — a reliable indicator of a human-made metal structure.
    pub fn phase_coherence(
        &self,
        scale: usize,
        window_radius: usize,
    ) -> Result<Array2<Scalar>, ScaleIndexError> {
        crate::curvelet::phase::phase_coherence(self, scale, window_radius)
    }

    /// Deterministic FNV-1a hash of all coefficient values.
    ///
    /// Workers can use this to verify data integrity after transmission without
    /// deserialising the full store.
    pub fn checksum(&self) -> u64 {
        crate::curvelet::phase::checksum(self)
    }

    /// Return the subband for (scale, angle_index), or None if out of bounds.
    pub fn subband(&self, scale: usize, angle_idx: usize) -> Option<&Subband> {
        self.detail.get(scale)?.get(angle_idx)
    }
}

// ── Error types ───────────────────────────────────────────────────────────────

/// Errors returned by curvelet transform operations.
#[derive(Debug, thiserror::Error)]
pub enum CurveletError {
    /// Input grid has a zero-length dimension.
    #[error("Input grid has zero dimension")]
    ZeroDimension,
    /// Scale count must be ≥ 1.
    #[error("Scale count must be >= 1")]
    InvalidScaleCount,
    /// FFT backend error.
    #[error("FFT backend error: {0}")]
    FftError(String),
    /// GeoTIFF loading error.
    #[error("GeoTIFF error: {0}")]
    GeoTiff(#[from] crate::geo::GeoTiffError),
}

/// Returned when a scale index is out of bounds.
#[derive(Debug, thiserror::Error)]
#[error("Scale index {index} out of bounds (store has {num_scales} scales)")]
pub struct ScaleIndexError {
    pub index: usize,
    pub num_scales: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subband_is_64_byte_aligned() {
        // Verify the compile-time assertion holds at runtime too.
        assert!(std::mem::align_of::<Subband>() >= 64);
    }

    #[test]
    fn subband_buffer_pointer_is_aligned() {
        let mut data: AVec<Complex<Scalar>, aligned_vec::ConstAlign<64>> =
            AVec::with_capacity(64, 16);
        for i in 0..16 {
            data.push(Complex::new(i as Scalar, 0.0));
        }
        let sub = Subband { data, rows: 4, cols: 4, scale: 0, angle_deg: 0.0 };
        assert!(sub.is_aligned(), "Subband buffer pointer is not 64-byte aligned");
    }

    #[test]
    fn subband_no_inter_element_padding() {
        // Elements must be densely packed — stride == size_of::<Complex<Scalar>>()
        let element_size = std::mem::size_of::<Complex<Scalar>>();
        let mut data: AVec<Complex<Scalar>, aligned_vec::ConstAlign<64>> =
            AVec::with_capacity(64, 2);
        data.push(Complex::new(1.0, 2.0));
        data.push(Complex::new(3.0, 4.0));
        let ptr0 = &data[0] as *const _ as usize;
        let ptr1 = &data[1] as *const _ as usize;
        assert_eq!(ptr1 - ptr0, element_size, "Inter-element padding detected");
    }
}

#[cfg(test)]
mod serde_tests {
    use super::*;
    use num_complex::Complex;

    fn make_minimal_store() -> CoefficientStore {
        let mut coarse_data: AVec<Complex<Scalar>, aligned_vec::ConstAlign<64>> =
            AVec::with_capacity(64, 4);
        for i in 0..4 {
            coarse_data.push(Complex::new(i as Scalar, 0.0));
        }
        let coarse = Subband { data: coarse_data, rows: 2, cols: 2, scale: 1, angle_deg: 0.0 };

        let mut sub_data: AVec<Complex<Scalar>, aligned_vec::ConstAlign<64>> =
            AVec::with_capacity(64, 4);
        for i in 0..4 {
            sub_data.push(Complex::new(i as Scalar * 0.5, i as Scalar * 0.25));
        }
        let sub = Subband { data: sub_data, rows: 2, cols: 2, scale: 0, angle_deg: 45.0 };

        CoefficientStore {
            coarse,
            detail: vec![vec![sub]],
            fine: vec![Complex::new(1.0, 0.5)],
            geo_transform: None,
            num_scales: 1,
        }
    }

    /// Requirement 10.4: CoefficientStore serde round-trip.
    #[test]
    fn coefficient_store_serde_roundtrip() {
        let store = make_minimal_store();
        let json = serde_json::to_string(&store).expect("serialise failed");
        let restored: CoefficientStore =
            serde_json::from_str(&json).expect("deserialise failed");

        // Verify detail coefficients are equal within precision tolerance.
        let orig_sub = &store.detail[0][0];
        let rest_sub = &restored.detail[0][0];
        assert_eq!(orig_sub.rows, rest_sub.rows);
        assert_eq!(orig_sub.cols, rest_sub.cols);
        for (o, r) in orig_sub.data.iter().zip(rest_sub.data.iter()) {
            assert!((o.re - r.re).abs() < 1e-5, "re mismatch: {} vs {}", o.re, r.re);
            assert!((o.im - r.im).abs() < 1e-5, "im mismatch: {} vs {}", o.im, r.im);
        }

        // Verify fine coefficients.
        assert_eq!(store.fine.len(), restored.fine.len());
        for (o, r) in store.fine.iter().zip(restored.fine.iter()) {
            assert!((o.re - r.re).abs() < 1e-5);
        }

        // Verify checksum is preserved.
        assert_eq!(store.checksum(), restored.checksum());
    }

    /// Requirement 10.5: GeoTransform serde round-trip.
    #[test]
    fn geo_transform_serde_roundtrip() {
        use crate::geo::GeoTransform;
        let gt = GeoTransform::new(
            [-83.5, 0.01, 0.0, 42.5, 0.0, -0.01],
            Some(4326),
            Some("WGS84".to_string()),
        );
        let json = serde_json::to_string(&gt).unwrap();
        let restored: GeoTransform = serde_json::from_str(&json).unwrap();
        for (a, b) in gt.coeffs.iter().zip(restored.coeffs.iter()) {
            assert_eq!(a.to_bits(), b.to_bits(), "GeoTransform coefficient not bit-identical");
        }
        assert_eq!(gt.epsg, restored.epsg);
    }
}

