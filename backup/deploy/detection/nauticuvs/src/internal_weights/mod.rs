//! Private wreck-signature detection parameters.
//!
//! This module is declared WITHOUT `pub` in `lib.rs` — it is crate-private only.
//! No symbol from this module appears in the public API or in `cargo doc` output.
//!
//! Workers supply parameters at runtime via the opaque `DetectionConfig` type
//! in `detection.rs`. The parameter schema is never visible externally.

use crate::precision::Scalar;

/// Errors from deserialising a parameter blob.
#[derive(Debug)]
pub(crate) enum ParamError {
    /// The blob is too short to contain a valid header.
    TooShort,
    /// The blob version is not supported.
    UnsupportedVersion(u8),
    /// The blob is malformed (checksum mismatch, truncated fields, etc.).
    Malformed(String),
}

impl std::fmt::Display for ParamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParamError::TooShort => write!(f, "Parameter blob is too short"),
            ParamError::UnsupportedVersion(v) => write!(f, "Unsupported parameter version: {}", v),
            ParamError::Malformed(s) => write!(f, "Malformed parameter blob: {}", s),
        }
    }
}

/// Calibrated wreck-signature detection parameters.
///
/// These values represent years of calibration against known wreck sites and
/// aeromagnetic survey data. They are kept private to prevent reverse-engineering
/// of the exact detection thresholds.
pub(crate) struct WreckSignatureParams {
    /// Minimum curvelet energy ratio to flag a dipole candidate.
    pub(crate) dipole_energy_threshold: Scalar,
    /// Minimum phase coherence score for a human-made structure signature.
    pub(crate) phase_coherence_min: Scalar,
    /// Per-scale weights applied during anomaly scoring (8 scales max).
    pub(crate) scale_weights: [Scalar; 8],
    /// Minimum dipole separation in metres to distinguish a wreck from noise.
    pub(crate) min_dipole_separation_m: f64,
    /// Maximum dipole separation in metres (larger = geological, not a wreck).
    pub(crate) max_dipole_separation_m: f64,
}

/// Blob format:
/// - Byte 0:    version (must be 1)
/// - Bytes 1-4: dipole_energy_threshold (f32 little-endian)
/// - Bytes 5-8: phase_coherence_min (f32 little-endian)
/// - Bytes 9-40: scale_weights[0..8] (8 × f32 little-endian)
/// - Bytes 41-48: min_dipole_separation_m (f64 little-endian)
/// - Bytes 49-56: max_dipole_separation_m (f64 little-endian)
/// Total: 57 bytes minimum
const BLOB_MIN_LEN: usize = 57;
const BLOB_VERSION: u8 = 1;

pub(crate) fn load_params(blob: &[u8]) -> Result<WreckSignatureParams, ParamError> {
    if blob.len() < BLOB_MIN_LEN {
        return Err(ParamError::TooShort);
    }
    if blob[0] != BLOB_VERSION {
        return Err(ParamError::UnsupportedVersion(blob[0]));
    }

    let read_f32 = |offset: usize| -> f32 {
        let bytes: [u8; 4] = blob[offset..offset + 4].try_into().unwrap();
        f32::from_le_bytes(bytes)
    };
    let read_f64 = |offset: usize| -> f64 {
        let bytes: [u8; 8] = blob[offset..offset + 8].try_into().unwrap();
        f64::from_le_bytes(bytes)
    };

    let dipole_energy_threshold = read_f32(1) as Scalar;
    let phase_coherence_min = read_f32(5) as Scalar;

    let mut scale_weights = [0.0 as Scalar; 8];
    for i in 0..8 {
        scale_weights[i] = read_f32(9 + i * 4) as Scalar;
    }

    let min_dipole_separation_m = read_f64(41);
    let max_dipole_separation_m = read_f64(49);

    if min_dipole_separation_m >= max_dipole_separation_m {
        return Err(ParamError::Malformed(
            "min_dipole_separation_m must be less than max_dipole_separation_m".into(),
        ));
    }

    Ok(WreckSignatureParams {
        dipole_energy_threshold,
        phase_coherence_min,
        scale_weights,
        min_dipole_separation_m,
        max_dipole_separation_m,
    })
}

/// Build a minimal valid parameter blob for testing.
/// The values are chosen to produce a score > 0.5 on a synthetic dipole grid.
#[cfg(test)]
pub(crate) fn test_param_blob() -> Vec<u8> {
    let mut blob = vec![0u8; BLOB_MIN_LEN];
    blob[0] = BLOB_VERSION;
    // dipole_energy_threshold = 0.1 (low threshold → easy to trigger)
    blob[1..5].copy_from_slice(&0.1_f32.to_le_bytes());
    // phase_coherence_min = 0.1
    blob[5..9].copy_from_slice(&0.1_f32.to_le_bytes());
    // scale_weights = [1.0; 8]
    for i in 0..8 {
        blob[9 + i * 4..9 + i * 4 + 4].copy_from_slice(&1.0_f32.to_le_bytes());
    }
    // min_dipole_separation_m = 50.0
    blob[41..49].copy_from_slice(&50.0_f64.to_le_bytes());
    // max_dipole_separation_m = 5000.0
    blob[49..57].copy_from_slice(&5000.0_f64.to_le_bytes());
    blob
}
