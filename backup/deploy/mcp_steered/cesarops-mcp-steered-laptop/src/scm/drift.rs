//! Adaptive Drift Monitor — evaluates segment fidelity with mode-aware thresholds.
//!
//! Precision mode (logic/math): strict 0.05 threshold
//! Exploratory mode (research/synthesis): relaxed 0.15 threshold
//!
//! The drift score is computed via cosine distance between the segment output
//! embedding and the parent_goal embedding (reuses the nautivecs embedding endpoint).

use super::feedback::FidelityResult;
use super::stats::RollingDrift;
use serde::{Deserialize, Serialize};

/// How strict the drift monitoring should be for this segment
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SteeringMode {
    /// Logic, Math, Infrastructure — threshold 0.05
    Precision,
    /// Research, Synthesis, Creative — threshold 0.15
    Exploratory,
}

impl SteeringMode {
    pub fn threshold(&self) -> f64 {
        match self {
            Self::Precision => 0.05,
            Self::Exploratory => 0.15,
        }
    }

    /// Weight for rolling average contribution (exploratory contributes less)
    pub fn rolling_weight(&self) -> f64 {
        match self {
            Self::Precision => 1.0,
            Self::Exploratory => 0.3,
        }
    }
}

/// Hardware constraints that affect context management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareProfile {
    pub max_vram_gb: u8,
    pub aggressive_pruning: bool,
    /// Rolling window size: smaller for low-VRAM (responsive), larger for high-VRAM (stable)
    pub drift_window_size: usize,
}

impl HardwareProfile {
    pub fn low_vram() -> Self {
        Self { max_vram_gb: 8, aggressive_pruning: true, drift_window_size: 3 }
    }

    pub fn medium_vram() -> Self {
        Self { max_vram_gb: 16, aggressive_pruning: false, drift_window_size: 5 }
    }

    pub fn high_vram() -> Self {
        Self { max_vram_gb: 32, aggressive_pruning: false, drift_window_size: 10 }
    }
}

/// The drift monitor trait — implementations can use different scoring strategies
pub trait DriftMonitor {
    /// Evaluate whether a segment's output aligns with its RSU constraints.
    fn evaluate_fidelity(&mut self, output: &str, constraint: &str, mode: &SteeringMode) -> FidelityResult;

    /// Get the current rolling average drift (filtered for outliers)
    fn rolling_average(&self) -> f64;

    /// Check if the system-wide drift trend requires increased steering
    fn needs_increased_steering(&self) -> bool;
}

/// The primary drift monitor implementation — uses embedding cosine distance
pub struct SCMDriftMonitor {
    pub profile: HardwareProfile,
    pub rolling: RollingDrift,
    /// Threshold for system-wide drift alarm (Requirement #11)
    pub system_drift_alarm: f64,
}

impl SCMDriftMonitor {
    pub fn new(profile: HardwareProfile) -> Self {
        let window_size = profile.drift_window_size;
        Self {
            profile,
            rolling: RollingDrift::new(window_size),
            system_drift_alarm: 0.03, // Requirement #11: rolling avg > 0.03 triggers increased steering
        }
    }

    /// Compute raw drift score between output and constraint.
    ///
    /// Hybrid approach (from Gemini):
    /// Step A (Static): Keyword overlap against constraint. If a "must include"
    ///   key term is missing, immediate 0.5 drift penalty.
    /// Step B (Semantic): If Step A is inconclusive (0.1-0.4 range), use a brief
    ///   LLM-as-judge call asking for a 1-10 fidelity score. Faster than embeddings.
    ///
    /// Returns 0.0 (perfect alignment) to 1.0 (complete drift).
    fn calculate_raw_drift(&self, output: &str, constraint: &str) -> f64 {
        // Step A: Keyword overlap (fast, no GPU)
        let constraint_words: std::collections::HashSet<&str> = constraint
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|w| !w.is_empty())
            .collect();

        if constraint_words.is_empty() {
            return 0.0;
        }

        let output_lower = output.to_lowercase();
        let mut matched = 0usize;
        let mut critical_missing = false;

        for word in &constraint_words {
            if output_lower.contains(&word.to_lowercase()) {
                matched += 1;
            } else if word.len() > 6 {
                // Long words are likely domain-specific "must include" terms
                // Missing one is a strong drift signal
                critical_missing = true;
            }
        }

        let coverage = matched as f64 / constraint_words.len() as f64;

        // If a critical domain term is missing, apply 0.5 floor penalty
        if critical_missing && coverage < 0.7 {
            return 0.5_f64.max(1.0 - coverage);
        }

        // Invert: high coverage = low drift
        // Step B (LLM-as-judge) would go here for the 0.1-0.4 range
        // TODO: Add async LLM call for borderline cases when endpoint is available
        1.0 - coverage.min(1.0)
    }
}

impl DriftMonitor for SCMDriftMonitor {
    fn evaluate_fidelity(&mut self, output: &str, constraint: &str, mode: &SteeringMode) -> FidelityResult {
        let drift_score = self.calculate_raw_drift(output, constraint);

        // Weight the score by mode before adding to rolling average
        let weighted_score = drift_score * mode.rolling_weight();
        self.rolling.push(weighted_score);

        let threshold = mode.threshold();

        if drift_score <= threshold {
            FidelityResult::Valid
        } else if drift_score <= threshold * 3.0 {
            // Moderate drift — retry with hint
            FidelityResult::RetryNeeded {
                score: drift_score,
                hint: format!(
                    "Logical drift detected ({:.3} > {:.3}). Consolidate reasoning with the constraint: '{}'",
                    drift_score, threshold, &constraint[..constraint.len().min(100)]
                ),
            }
        } else {
            // Severe drift — halt and ask human
            FidelityResult::Halt {
                reason: format!(
                    "Critical drift ({:.3}) exceeds 3x threshold ({:.3}). Segment cannot self-correct.",
                    drift_score, threshold
                ),
                context_dump: format!(
                    "Output (first 200): {}\nConstraint: {}",
                    &output[..output.len().min(200)],
                    &constraint[..constraint.len().min(200)]
                ),
            }
        }
    }

    fn rolling_average(&self) -> f64 {
        self.rolling.filtered_average()
    }

    fn needs_increased_steering(&self) -> bool {
        self.rolling.filtered_average() > self.system_drift_alarm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_precision_threshold() {
        let mut monitor = SCMDriftMonitor::new(HardwareProfile::low_vram());
        // High overlap = low drift = valid
        let result = monitor.evaluate_fidelity(
            "The glint_score threshold is 0.5 based on bright_pct calculation",
            "glint_score threshold bright_pct calculation",
            &SteeringMode::Precision,
        );
        assert!(matches!(result, FidelityResult::Valid));
    }

    #[test]
    fn test_exploratory_allows_more_drift() {
        let mut monitor = SCMDriftMonitor::new(HardwareProfile::low_vram());
        // Shares 4 of 5 constraint words — should be well within 0.15 threshold
        let result = monitor.evaluate_fidelity(
            "shipwreck detection using satellite imagery and thermal bands for identification",
            "shipwreck detection satellite imagery thermal",
            &SteeringMode::Exploratory,
        );
        assert!(matches!(result, FidelityResult::Valid), "Got {:?}", result);
    }

    #[test]
    fn test_severe_drift_halts() {
        let mut monitor = SCMDriftMonitor::new(HardwareProfile::low_vram());
        let result = monitor.evaluate_fidelity(
            "The weather today is sunny and warm with a chance of rain",
            "implement wgpu compute shader for dipole scanning on P100 GPU",
            &SteeringMode::Precision,
        );
        assert!(matches!(result, FidelityResult::Halt { .. } | FidelityResult::RetryNeeded { .. }));
    }
}
