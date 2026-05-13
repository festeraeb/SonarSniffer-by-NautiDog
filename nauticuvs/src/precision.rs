//! Compile-time floating-point precision selection.
//!
//! DEFAULT: f64 for maximum accuracy in aeromagnetic/drift-correction passes.
//! The sub-pixel alignment algorithm requires >7 decimal digits of precision
//! to avoid phase accumulation errors across 20+ day temporal stacks.
//!
//! Use the `f32-fast` feature flag ONLY for GPU-bound preprocessing where
//! throughput matters more than precision (e.g., dipole scoring before the
//! curvelet inverse pass).
//!
//! # Feature flags
//! - (default)      — use 64-bit floats throughout the FDCT pipeline (recommended)
//! - `f32-fast`     — use 32-bit floats for maximum GPU throughput (lossy)
//!
//! # Why f64 is the default
//! - 2D FFT on 4096×4096 grid: 16M complex multiplications accumulate ~4 pixels
//!   of phase drift in f32, but only ~0.0001 pixels in f64
//! - Meyer window application: subtracting nearly-equal values near window edges
//!   causes catastrophic cancellation in f32 (loses 3-4 bits of precision)
//! - Energy ratio computation: sum of norm_sqr() across subbands can exceed f32
//!   dynamic range, causing small anomalies to vanish into the noise floor
//! - The Xeon 4110 does native FP64 at full speed via AVX-512 (8 doubles/cycle)
//!   so there is NO performance penalty for using f64 on the CPU path
//!
//! # Usage
//! ```rust
//! use nauticuvs::Scalar;
//! let grid: ndarray::Array2<Scalar> = ndarray::Array2::zeros((64, 64));
//! // Scalar is f64 by default — full precision for curvelet transforms
//! ```

// Mutual-exclusion guard — both flags active at once is a configuration error.
#[cfg(all(feature = "f32-fast", feature = "f64"))]
compile_error!(
    "Features `f32-fast` and `f64` are mutually exclusive. \
     Enable at most one precision feature in your Cargo.toml."
);

// ── Scalar type alias ─────────────────────────────────────────────────────────

/// The active floating-point scalar type for all FDCT calculations.
///
/// Resolves to `f32` ONLY when the `f32-fast` Cargo feature is active.
/// Default is `f64` for maximum accuracy in sub-pixel alignment and
/// temporal stacking across 20+ day satellite passes.
///
/// Workers should use this alias instead of hard-coding a float type so that
/// recompiling with a different precision flag requires no source changes.
#[cfg(feature = "f32-fast")]
pub type Scalar = f32;

#[cfg(not(feature = "f32-fast"))]
pub type Scalar = f64;

// ── Complex re-export ─────────────────────────────────────────────────────────

pub use num_complex::Complex;

/// Complex number parameterised on the active `Scalar` type.
pub type C = Complex<Scalar>;
