use crate::channel_discovery::SpatialRole;
use serde::Serialize;

// ─────────────────────────────────────────────────────────────────────────────
//  Tuning constants
// ─────────────────────────────────────────────────────────────────────────────

/// Minimum gain correction factor (never darken a column more than 10×).
const MIN_GAIN: f32 = 0.10;
/// Maximum gain correction factor (never amplify a column more than 20×).
const MAX_GAIN: f32 = 20.0;

/// Percentile (0.0–1.0) used to build the beam profile.  10th percentile per
/// column gives the "average seabed response" while ignoring fish/structure echoes.
const PROFILE_PERCENTILE: f32 = 0.10;

/// Minimum number of pings required to compute a reliable profile.
const MIN_PINGS_FOR_PROFILE: usize = 20;

/// Smoothing window (in samples) for the profile — prevents single-bin spikes
/// from introducing correction artefacts.
const SMOOTH_WINDOW: usize = 15;

/// For GT51 single-wing channels: the gain profile is expected to be a monotone
/// slope.  We enforce this by capping the near-edge gain so we never brighten
/// index 0 (already the strongest signal in the beam) past this factor.
const GT51_NEAR_EDGE_CAP: f32 = 1.5;

// ─────────────────────────────────────────────────────────────────────────────
//  Public types
// ─────────────

#[derive(Debug, Clone, Serialize)]
pub struct EgnProfile {
    /// The correction factors (1.0 = no change).
    /// Each element corresponds to a sample index (range bin).
    pub factors: Vec<f32>,
    /// The number of pings used to calculate this profile.
    pub ping_count: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Implementation
// ─────────────

impl EgnProfile {
    /// Computes the beam profile from a collection of pings.
    ///
    /// The profile is calculated by taking the Nth percentile of values at each
    /// sample index across all pings. This ensures that transient echoes (fish,
    /// structure) do not bias the beam shape.
    pub fn compute_beam_profile(
        pings: &[Vec<u16>],
        role: SpatialRole,
    ) -> Option<Self> {
        if pings.len() < MIN_PINGS_FOR_PROFILE {
            return None;
        }

        let num_samples = pings[0].len();
        let mut factors = Vec::with_capacity(num_samples);

        for i in 0..num_samples {
            // Collect all values at this sample index across all pings
            let mut values: Vec<u16> = pings.iter().map(|p| p[i]).collect();
            values.sort_unstable();

            // Find the value at the specified percentile
            let idx = ((values.len() as f32) * PROFILE_PERCENTILE).floor() as usize;
            let val = values[idx.min(values.len() - 1)] as f32;

            // We want to find the ratio of the "average" signal to the peak signal.
            // However, we don't know the peak yet. We'll store the raw percentile
            // values and normalize them in the next step.
            factors.push(val);
        }

        // 1. Normalize: divide by the maximum value found in the profile to get a relative shape.
        //    Since we want to amplify weak signals, we calculate: factor = max_val / current_val.
        let max_val = factors.iter().cloned().fold(0.0, f32::max);
        
        if max_val <= 0.0 {
            return None;
        }

        for val in factors.iter_mut() {
            if *val <= 0.0 {
                *val = 1.0; // Avoid division by zero
            }
            *val = max_val / *val;
        }

        // 2. Apply smoothing window to the factors to prevent jitter
        let mut smoothed = factors.clone();
        for i in 0..num_samples {
            let start = i.saturating_sub(SMOOTH_WINDOW / 2);
            let end = (i + SMOOTH_WINDOW / 2 + 1).min(num_samples);
            let window = &factors[start..end];
            let sum: f32 = window.iter().sum();
            smoothed[i] = sum / (window.len() as f32);
        }

        // 3. Apply GT51 single-wing constraint:
        //    In a single-wing beam, the strongest signal is at index 0.
        //    The profile should be a monotone decrease.
        if let SpatialRole::SingleWing = role {
            let cap = smoothed[0] * GT51_NEAR_EDGE_CAP;
            for i in 1..num_samples {
                if smoothed[i] > cap {
                    smoothed[i] = cap;
                }
            }
        }

        // 4. Clamp to global safety limits
        for val in smoothed.iter_mut() {
            *val = val.clamp(MIN_GAIN, MAX_GAIN);
        }

        Some(Self {
            factors: smoothed,
            ping_count: pings.len(),
        })
    }

    /// Applies the EGN profile to a single ping.
    pub fn apply_to_ping(&self, ping: &mut [u16]) {
        for (i, val) in ping.iter_mut().enumerate() {
            if i < self.factors.len() {
                let factor = self.factors[i];
                // Use f32 for intermediate calculation to prevent overflow/underflow
                let corrected = (*val as f32) * factor;
                // Clamp to u16 range and floor at 1 to avoid zeroing out columns
                *val = (corrected.clamp(1.0, 65535.0)) as u16;
            }
        }
    }
}
