//! Fidelity Feedback Loop — translates drift scores into actions.
//!
//! Three outcomes:
//! - Valid: commit the segment, proceed to next
//! - RetryNeeded: inject [STEERING ALERT] and re-run (token-efficient)
//! - Halt: stop everything, ask human (safety valve)

use super::rsu::RsuTask;
use serde::{Deserialize, Serialize};

/// The result of a fidelity evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FidelityResult {
    /// Segment is within threshold. Proceed to commit.
    Valid,
    /// Minor drift. Re-run the current RSU with a correction hint.
    RetryNeeded {
        score: f64,
        hint: String,
    },
    /// Critical drift. Halt execution and request human intervention.
    Halt {
        reason: String,
        context_dump: String,
    },
}

/// Actions the pipeline can take after fidelity evaluation
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Segment passed — commit output and move to next RSU
    CommitSegment,
    /// Segment drifted — retry with steering alert injected
    RetrySegment,
    /// Critical failure — halt pipeline, signal human via n8n
    SignalInterrupt(String),
}

/// Dispatches fidelity results into pipeline actions.
/// Sits between the DriftMonitor and the LLM Client.
pub struct FeedbackDispatcher;

impl FeedbackDispatcher {
    /// Translate a FidelityResult into a concrete Action.
    /// Mutates the RSU in-place for retries (preserves work, refines current slice).
    pub fn dispatch(result: FidelityResult, current_rsu: &mut RsuTask) -> Action {
        match result {
            FidelityResult::Valid => Action::CommitSegment,

            FidelityResult::RetryNeeded { score, hint } => {
                // Update retry counter
                current_rsu.metadata.retries += 1;

                // Safety: max 3 retries before escalating to halt
                if current_rsu.metadata.retries > 3 {
                    return Action::SignalInterrupt(format!(
                        "Segment {} exceeded max retries (3). Last drift: {:.3}. Hint: {}",
                        current_rsu.id, score, hint
                    ));
                }

                // Inject the steering alert into the observation phase
                // This is token-efficient: one line instead of full error dump
                current_rsu.phases.observation.push_str(&format!(
                    "\n[STEERING ALERT]: Previous attempt had drift score {:.3}. Correction: {}",
                    score, hint
                ));

                Action::RetrySegment
            }

            FidelityResult::Halt { reason, .. } => {
                Action::SignalInterrupt(reason)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::rsu::{RsuTask, RsuMetadata, RsuPhases};

    fn make_test_rsu() -> RsuTask {
        RsuTask {
            id: "segment_001".to_string(),
            metadata: RsuMetadata {
                parent_goal: "test goal".to_string(),
                thinking_budget: super::super::rsu::ThinkingBudget::Medium,
                steering_mode: super::super::drift::SteeringMode::Precision,
                retries: 0,
            },
            phases: RsuPhases {
                observation: "observe something".to_string(),
                reasoning: "reason about it".to_string(),
                accuracy_check: "verify correctness".to_string(),
            },
            output: None,
        }
    }

    #[test]
    fn test_valid_commits() {
        let mut rsu = make_test_rsu();
        let action = FeedbackDispatcher::dispatch(FidelityResult::Valid, &mut rsu);
        assert_eq!(action, Action::CommitSegment);
    }

    #[test]
    fn test_retry_injects_alert() {
        let mut rsu = make_test_rsu();
        let action = FeedbackDispatcher::dispatch(
            FidelityResult::RetryNeeded {
                score: 0.08,
                hint: "Use the actual threshold from tpu_client.py".to_string(),
            },
            &mut rsu,
        );
        assert_eq!(action, Action::RetrySegment);
        assert!(rsu.phases.observation.contains("[STEERING ALERT]"));
        assert_eq!(rsu.metadata.retries, 1);
    }

    #[test]
    fn test_max_retries_escalates_to_halt() {
        let mut rsu = make_test_rsu();
        rsu.metadata.retries = 3; // Already at max

        let action = FeedbackDispatcher::dispatch(
            FidelityResult::RetryNeeded {
                score: 0.12,
                hint: "still drifting".to_string(),
            },
            &mut rsu,
        );
        assert!(matches!(action, Action::SignalInterrupt(_)));
    }

    #[test]
    fn test_halt_signals_interrupt() {
        let mut rsu = make_test_rsu();
        let action = FeedbackDispatcher::dispatch(
            FidelityResult::Halt {
                reason: "critical drift".to_string(),
                context_dump: "...".to_string(),
            },
            &mut rsu,
        );
        assert!(matches!(action, Action::SignalInterrupt(_)));
    }
}
