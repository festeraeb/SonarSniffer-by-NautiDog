//! # nauticuvs — Precision Curvelet Engine
//!
//! Mathematical core for CESAROPS shipwreck detection, SAR analysis, and
//! aeromagnetic survey processing.
//!
//! ## Quick start
//!
//! ```rust
//! use nauticuvs::{curvelet_forward, Scalar};
//! use ndarray::Array2;
//!
//! let grid: Array2<Scalar> = Array2::zeros((64, 64));
//! let coeffs = curvelet_forward(&grid, 4).unwrap();
//!
//! // Access detail and fine scales (backward-compatible with aeromagnetic worker)
//! let energy: f64 = coeffs.detail[0][0].iter().map(|c| c.norm_sqr() as f64).sum();
//! let fine_energy: f64 = coeffs.fine.iter().map(|c| c.norm_sqr() as f64).sum();
//! ```
//!
//! ## Feature flags
//! - `f64`          — use 64-bit floats (default: f32)
//! - `fftw`         — use FFTW3 backend instead of rustfft
//! - `gdal-support` — use GDAL for GeoTIFF parsing
//! - `xla`          — compile FDCT kernels for TPU via XLA

// ── Module declarations ───────────────────────────────────────────────────────

pub mod curvelet;
pub mod fdct_kernels;
pub mod geo;
pub mod protocol;
pub mod synthetic_grid;
pub mod weights;

mod detection;
mod fft_backend;
mod internal_weights;  // pub(crate) only — never re-exported
mod precision;

// ── Public re-exports ─────────────────────────────────────────────────────────

/// The active floating-point scalar type.
/// Resolves to `f64` with the `f64` feature, otherwise `f32`.
pub use precision::Scalar;

// Primary entry point — backward-compatible with the existing aeromagnetic worker.
pub use curvelet::curvelet_forward;

// Additional transform entry points.
pub use curvelet::{curvelet_forward_geo, curvelet_inverse};

// Core data structures.
pub use curvelet::coefficient_store::{
    CoefficientStore, CurveletError, ScaleIndexError, Subband,
};

// Geographic types.
pub use geo::transform::GeoTransform;
pub use geo::GeoTiffError;
pub use geo::geotiff::GeoTiffInput;

// Weighting traits and implementations.
pub use weights::directional::{
    ComplementMask, DirectionalMask, IdentityMask, StripeSuppressor,
};
pub use weights::richardson::{
    RichardsonError, RichardsonProfile, RichardsonWeighter,
};

// Detection interface (opaque — internal parameters not exposed).
pub use detection::{detect_anomaly, DetectionConfig, DetectionResult};
