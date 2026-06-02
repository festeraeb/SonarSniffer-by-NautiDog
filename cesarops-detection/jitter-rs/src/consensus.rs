//! Consensus: fold the primary candidate together with validator votes into a
//! final JitterSignature. Agreement raises certainty; disagreement lowers it.

use crate::heuristic::{round1_ft, round2, round3, round4};
use crate::types::{Candidate, JitterRequest, JitterSignature, ValidatorVote};

/// Maximum certainty adjustment a full validator panel can contribute.
const MAX_BOOST: f64 = 0.12;
const MAX_PENALTY: f64 = 0.25;

pub fn combine(
    req: &JitterRequest,
    primary: Candidate,
    votes: Vec<ValidatorVote>,
) -> JitterSignature {
    let mut certainty = primary.certainty;

    if !votes.is_empty() {
        // Mean agreement across validators in [0,1].
        let mean_agree: f64 =
            votes.iter().map(|v| v.agreement).sum::<f64>() / votes.len() as f64;
        let agreed_frac =
            votes.iter().filter(|v| v.agreed).count() as f64 / votes.len() as f64;

        // Symmetric adjustment centred at 0.5 mean agreement.
        if mean_agree >= 0.5 {
            certainty += MAX_BOOST * (mean_agree - 0.5) * 2.0 * agreed_frac;
        } else {
            certainty -= MAX_PENALTY * (0.5 - mean_agree) * 2.0 * (1.0 - agreed_frac).max(0.0);
        }
    }

    certainty = certainty.clamp(0.0, 0.99);

    // Material may flip to natural if confidence collapses after disagreement.
    let material = if certainty > 0.7 {
        primary.material.clone()
    } else {
        "natural".to_string()
    };
    let classification = if certainty >= 0.7 {
        "confirmed_structure"
    } else {
        "likely_natural"
    };

    let depth_ft = req.depth_estimate_m * 3.28084;

    JitterSignature {
        material,
        certainty: round3(certainty),
        depth_estimate_ft: round1_ft(depth_ft),
        jitter_frequency_hz: round4(primary.jitter_frequency_hz),
        thermal_delta_c: round2(primary.thermal_delta_c),
        classification: classification.to_string(),
        validation: votes,
        primary_backend: primary.backend,
    }
}
