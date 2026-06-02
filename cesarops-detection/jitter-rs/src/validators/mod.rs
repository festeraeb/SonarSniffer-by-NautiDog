//! Heterogeneous accelerator validators.
//!
//! The primary inference (CPU `tract`, or GPU) yields a [`Candidate`]. Each
//! available validator independently scores the same tile and votes on whether
//! it agrees with the primary's material classification. Votes are folded into
//! a consensus certainty in `consensus.rs`.
//!
//! Validators are feature-gated so the default build runs on any node:
//!   - `edgetpu`  -> Coral Edge TPU (FFI to libedgetpu.so) — typically ML350e
//!   - `movidius` -> Intel NCS2 / Myriad X (OpenVINO 2022.3 MYRIAD) — T440
//!
//! When a feature is off, or the hardware is absent, the validator reports
//! itself unavailable and is skipped — never fatal.

use crate::types::{Candidate, JitterRequest, ValidatorVote};

mod edgetpu;
mod movidius;
mod remote;

/// A device that can independently corroborate the primary candidate.
#[async_trait::async_trait]
pub trait Validator: Send + Sync {
    /// Stable device label, e.g. "coral_edgetpu" / "movidius_ncs2".
    fn device(&self) -> &str;
    /// True when the backing hardware/runtime is usable on this node.
    fn available(&self) -> bool;
    /// Score the tile and vote relative to the primary candidate.
    async fn vote(&self, req: &JitterRequest, primary: &Candidate) -> Option<ValidatorVote>;
}

/// Build the validator set for this node. Only available devices are returned.
pub async fn discover() -> Vec<Box<dyn Validator>> {
    let mut out: Vec<Box<dyn Validator>> = Vec::new();

    if let Some(v) = edgetpu::EdgeTpuValidator::try_new() {
        if v.available() {
            out.push(Box::new(v));
        }
    }
    if let Some(v) = movidius::MovidiusValidator::try_new() {
        if v.available() {
            out.push(Box::new(v));
        }
    }

    // Remote HTTP validators (e.g. Coral worker on the ML350e). Only reachable
    // remotes (probed at startup) are added.
    for r in remote::from_env().await {
        if r.available() {
            out.push(Box::new(r));
        }
    }
    out
}

/// Helper shared by validators: agreement score in [0,1].
///
/// Same material => high agreement scaled by how close the independent
/// certainty is to the primary's. Different material => low agreement.
pub(crate) fn agreement_score(primary: &Candidate, indep_material: &str, indep_certainty: f64) -> f64 {
    if primary.material == indep_material {
        let delta = (primary.certainty - indep_certainty).abs();
        (1.0 - delta).clamp(0.0, 1.0)
    } else {
        // Disagreement, but weight by how confident the validator was.
        (1.0 - indep_certainty).clamp(0.0, 0.5) * 0.5
    }
}
