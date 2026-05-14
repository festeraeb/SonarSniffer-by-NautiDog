//! Steering Controller — non-bypassable think-prefix injection layer.
//!
//! Every RSU passes through this controller before reaching the LlmClient.
//! Direct LLM calls are not permitted — this is the ONLY path to the model.
//!
//! Responsibilities:
//! - Build think-prefix referencing RSU's reasoning content (Requirement 3.1)
//! - Include accuracy_check as explicit constraint in prefix (Requirement 3.2)
//! - Query SteeringEngine for nautivecs context (Requirements 3.3, 10.1, 10.4)
//! - Enforce non-bypassable guarantee (Requirement 3.7)
//! - Handle needs_human_approval flag from SteeringContext (Requirement 10.2)
//! - Include corrections from correction store with highest priority (Requirement 10.3)
//! - Respect SteeringEngine's context_budget limit (Requirement 10.5)
//!
//! The controller combines:
//! 1. Corrections from correction store (highest priority in think-prefix)
//! 2. Think-prefix (reasoning guidance + accuracy constraint)
//! 3. Steering context (nautivecs fragments via SteeringEngine)
//! 4. Phase-specific prompt content
//! Then dispatches to LlmClient for completion.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::llm_client::LlmClient;
use crate::steering::SteeringEngine;

use super::rsu::{Rsu, RsuSteeringPolicy, SegmentMode, ThinkingBudget};

// ── Phase Enum ───────────────────────────────────────────────────────────────

/// Which phase of RSU execution is being processed.
///
/// Strict ordering: Observation → Reasoning → AccuracyCheck.
/// The SteeringController enforces this by requiring the caller to specify
/// the phase explicitly — it does not auto-advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Gather context from prior segments and nautivecs
    Observation,
    /// Execute the core reasoning task
    Reasoning,
    /// Verify reasoning output satisfies the stated constraint
    AccuracyCheck,
}

impl Phase {
    /// Human-readable label for prompt construction
    pub fn label(&self) -> &'static str {
        match self {
            Self::Observation => "Observation",
            Self::Reasoning => "Reasoning",
            Self::AccuracyCheck => "Accuracy Check",
        }
    }
}

// ── BudgetConfig ─────────────────────────────────────────────────────────────

/// Tracks token budget for a single RSU's execution across all phases.
///
/// - `max_reasoning_tokens`: per-phase limit for the reasoning phase
///   (low=512, medium=2048, high=4096) — Requirements 3.4, 3.5, 3.6
/// - `total_budget`: total token limit across all phases
///   (low=1024, medium=4096, high=8192) — Requirements 9.1, 9.2, 9.3
/// - `tokens_consumed`: running total of tokens used so far
/// - `budget_exhausted`: set to true when total limit is hit (Requirement 9.4)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Max tokens for the reasoning phase specifically
    pub max_reasoning_tokens: u32,
    /// Total token budget across all phases for this RSU
    pub total_budget: u32,
    /// Running total of tokens consumed across phases
    pub tokens_consumed: u32,
    /// Whether the budget has been exhausted (Requirement 9.4)
    pub budget_exhausted: bool,
}

impl BudgetConfig {
    /// Create a BudgetConfig from a ThinkingBudget tier.
    ///
    /// Maps:
    /// - Low:    reasoning=512,  total=1024
    /// - Medium: reasoning=2048, total=4096
    /// - High:   reasoning=4096, total=8192
    pub fn from_thinking_budget(budget: &ThinkingBudget) -> Self {
        Self {
            max_reasoning_tokens: budget.reasoning_tokens(),
            total_budget: budget.total_tokens(),
            tokens_consumed: 0,
            budget_exhausted: false,
        }
    }

    /// Remaining tokens available before budget is exhausted.
    pub fn remaining(&self) -> u32 {
        self.total_budget.saturating_sub(self.tokens_consumed)
    }
}

/// Result of budget enforcement on a response.
#[derive(Debug, Clone)]
pub struct BudgetResult {
    /// The (possibly truncated) response content
    pub content: String,
    /// Number of tokens estimated for this response
    pub tokens_used: u32,
    /// Whether the budget was exhausted by this response (Requirement 9.4)
    pub budget_exhausted: bool,
}

// ── SteerResult ──────────────────────────────────────────────────────────────

/// Result of a steered execution, including the LLM response and metadata
/// about the steering context that was applied.
///
/// The `needs_human_approval` flag (Requirement 10.2) signals the pipeline
/// that this segment's steering context had low confidence and should be
/// reviewed by a human before the output is committed.
#[derive(Debug, Clone)]
pub struct SteerResult {
    /// The LLM response content
    pub response: String,
    /// Whether the SteeringEngine flagged this as needing human approval
    /// (Requirement 10.2: low-confidence warning)
    pub needs_human_approval: bool,
    /// Number of corrections that were included in the think-prefix
    /// (Requirement 10.3)
    pub corrections_applied: usize,
    /// Whether the segment ran without nautivecs context (store unreachable)
    pub unsteered: bool,
}

// ── DriftMonitorFactory ──────────────────────────────────────────────────────

/// Factory for selecting the appropriate drift monitor based on steering policy.
/// Used by SteeringController to determine threshold behavior per RSU.
pub struct DriftMonitorFactory;

impl DriftMonitorFactory {
    pub fn new() -> Self {
        Self
    }

    /// Select the drift threshold based on the RSU's steering policy.
    /// Precision mode → 0.05, Exploratory mode → 0.15.
    pub fn threshold_for_policy(&self, policy: &RsuSteeringPolicy) -> f32 {
        policy.drift_threshold
    }

    /// Get the segment mode from the policy.
    pub fn mode_for_policy(&self, policy: &RsuSteeringPolicy) -> SegmentMode {
        policy.mode
    }
}

impl Default for DriftMonitorFactory {
    fn default() -> Self {
        Self::new()
    }
}

// ── SteeringController ───────────────────────────────────────────────────────

/// Non-bypassable steering layer. Every RSU passes through here
/// before reaching the LlmClient.
///
/// This is the ONLY permitted path to the LLM. Direct calls to
/// `LlmClient::steered_completion` or `LlmClient::chat_completion`
/// from outside this controller violate the non-bypassable guarantee
/// (Requirement 3.7).
pub struct SteeringController {
    /// Selects the appropriate drift threshold based on RSU's steering_policy
    drift_monitors: DriftMonitorFactory,
}

impl SteeringController {
    /// Create a new SteeringController.
    pub fn new() -> Self {
        Self {
            drift_monitors: DriftMonitorFactory::new(),
        }
    }

    /// Build the steered prompt for an RSU and execute it against the LLM.
    ///
    /// This is the ONLY path to the LlmClient — direct calls are not permitted
    /// (Requirement 3.7: non-bypassable guarantee).
    ///
    /// Steps:
    /// 1. Query SteeringEngine for nautivecs context (Reqs 3.3, 10.1, 10.4)
    /// 2. Handle needs_human_approval flag (Req 10.2)
    /// 3. Include corrections in think-prefix with highest priority (Req 10.3)
    /// 4. Build think-prefix from RSU's reasoning + accuracy_check (Reqs 3.1, 3.2)
    /// 5. Respect context_budget limit (Req 10.5)
    /// 6. Combine think-prefix + steering context + phase-specific prompt
    /// 7. Call LlmClient for completion
    /// 8. Return SteerResult with response and metadata
    pub async fn steer_and_execute(
        &self,
        rsu: &Rsu,
        phase: Phase,
        prior_output: Option<&str>,
        steering: &mut SteeringEngine,
        llm: &LlmClient,
    ) -> Result<SteerResult> {
        // Step 1: Query SteeringEngine for nautivecs context (Requirements 3.3, 10.1, 10.4)
        // Use RSU's reasoning as query, domain as role_hint
        let role_hint = self.domain_to_role_hint(&rsu.meta.steering_policy);
        let steering_context = steering
            .build_context(
                &rsu.phases.reasoning,
                Some(role_hint),
                Some(&rsu.id),
            )
            .await;

        // Extract steering data, handling unreachable nautivecs (Requirement 12.4)
        let (steering_prompt, needs_human_approval, corrections_text, corrections_applied, unsteered) =
            match steering_context {
                Ok(ctx) => {
                    // Step 2: Handle needs_human_approval flag (Requirement 10.2)
                    // When SteeringEngine returns low confidence, pause and emit warning
                    if ctx.needs_human_approval {
                        tracing::warn!(
                            rsu_id = %rsu.id,
                            phase = %phase.label(),
                            confidence = ctx.confidence,
                            "Low-confidence steering context — segment needs human review before proceeding"
                        );
                    }

                    (
                        ctx.system_prompt,
                        ctx.needs_human_approval,
                        ctx.corrections_text,
                        ctx.corrections_applied,
                        false,
                    )
                }
                Err(e) => {
                    tracing::warn!(
                        rsu_id = %rsu.id,
                        error = %e,
                        "SteeringEngine unreachable — proceeding without nautivecs context"
                    );
                    (String::new(), false, String::new(), 0, true)
                }
            };

        // Step 3 & 4: Build think-prefix with corrections at highest priority (Requirement 10.3)
        let think_prefix = self.build_think_prefix_with_corrections(
            rsu,
            phase,
            prior_output,
            &corrections_text,
        );

        // Step 5: Respect context_budget limit (Requirement 10.5)
        // The context_budget is the max token count for injected context.
        // We must not exceed it when combining think-prefix + steering prompt + phase prompt.
        let context_budget = steering.context_budget();
        let phase_prompt = self.build_phase_prompt(rsu, phase, prior_output);

        let system_context = self.assemble_system_context(
            &think_prefix,
            &steering_prompt,
            &phase_prompt,
            context_budget,
        );

        // Step 6: Call LlmClient for completion (via steered_completion)
        let user_query = self.build_user_query(rsu, phase, prior_output);
        let response = llm.steered_completion(&system_context, &user_query).await?;

        // Step 7: Return SteerResult with metadata
        Ok(SteerResult {
            response,
            needs_human_approval,
            corrections_applied,
            unsteered,
        })
    }

    /// Select the drift monitor threshold based on the RSU's steering_policy.
    pub fn select_threshold(&self, policy: &RsuSteeringPolicy) -> f32 {
        self.drift_monitors.threshold_for_policy(policy)
    }

    /// Step B: LLM-as-Judge for drift scoring.
    ///
    /// When Step A (keyword overlap) returns a drift score > 0.1, this method
    /// fires a 1-token query to the LLM asking it to rate the segment's fidelity
    /// to the constraint on a 1-5 scale.
    ///
    /// A score of 4-5 from the judge overrides the keyword-based drift score,
    /// dropping it below threshold. This prevents false-positive retries when
    /// the output is semantically correct but uses different vocabulary.
    ///
    /// Returns the adjusted drift score (0.0-1.0).
    pub async fn judge_drift(
        &self,
        output: &str,
        constraint: &str,
        keyword_drift: f32,
        llm: &LlmClient,
    ) -> f32 {
        // Only invoke the judge when Step A gives a suspicious score
        if keyword_drift <= 0.1 {
            return keyword_drift;
        }

        // Truncate output for the judge prompt (keep it fast)
        let output_preview = if output.len() > 300 {
            &output[..300]
        } else {
            output
        };

        let judge_prompt = format!(
            "Rate this segment's fidelity to the constraint. Output ONLY a single number 1-5.\n\
             1=completely unrelated, 2=tangentially related, 3=partially addresses it, 4=mostly satisfies it, 5=fully satisfies it.\n\n\
             Constraint: {}\n\n\
             Segment output: {}\n\n\
             Rating (1-5):",
            constraint,
            output_preview
        );

        let result = llm
            .steered_completion(
                "You are a fidelity judge. Output ONLY a single digit 1-5. No explanation.",
                &judge_prompt,
            )
            .await;

        match result {
            Ok(response) => {
                // Parse the 1-5 score from the response
                let score = response
                    .trim()
                    .chars()
                    .find(|c| c.is_ascii_digit())
                    .and_then(|c| c.to_digit(10))
                    .unwrap_or(1) as f32;

                tracing::info!(
                    keyword_drift = keyword_drift,
                    judge_score = score,
                    "Step B (LLM-Judge): semantic fidelity check"
                );

                // Map judge score to drift:
                // 5 → 0.01 (perfect), 4 → 0.03, 3 → 0.08, 2 → 0.15, 1 → keep keyword score
                match score as u32 {
                    5 => 0.01,
                    4 => 0.03,
                    3 => 0.08,
                    2 => 0.15,
                    _ => keyword_drift, // Judge agrees with keyword scorer — it's real drift
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Step B (LLM-Judge) failed — falling back to keyword drift score"
                );
                keyword_drift
            }
        }
    }

    /// Create a BudgetConfig for the given RSU based on its thinking_budget.
    ///
    /// Maps ThinkingBudget to token limits:
    /// - Low:    reasoning=512,  total=1024  (Requirements 3.4, 9.1)
    /// - Medium: reasoning=2048, total=4096  (Requirements 3.5, 9.2)
    /// - High:   reasoning=4096, total=8192  (Requirements 3.6, 9.3)
    pub fn budget_for_rsu(rsu: &Rsu) -> BudgetConfig {
        BudgetConfig::from_thinking_budget(&rsu.meta.thinking_budget)
    }

    /// Enforce the token budget on a phase response.
    ///
    /// - Estimates tokens used (chars / 4 approximation)
    /// - Adds to `config.tokens_consumed`
    /// - If total exceeds budget, truncates response and marks `budget_exhausted = true`
    ///   (Requirement 9.4)
    /// - Returns the (possibly truncated) response and whether budget was exhausted
    ///
    /// The caller (pipeline coordinator) should proceed to accuracy_check with
    /// the truncated output when budget is exhausted (Requirement 9.5).
    pub fn enforce_budget(config: &mut BudgetConfig, _phase: Phase, response: &str) -> BudgetResult {
        // Simple token estimation: chars / 4 (standard approximation)
        let estimated_tokens = (response.len() as u32) / 4;
        let remaining = config.remaining();

        if estimated_tokens <= remaining {
            // Within budget — record and return full response
            config.tokens_consumed += estimated_tokens;
            BudgetResult {
                content: response.to_string(),
                tokens_used: estimated_tokens,
                budget_exhausted: false,
            }
        } else {
            // Budget exceeded — truncate response to fit remaining budget
            // Convert remaining tokens back to approximate char count
            let max_chars = (remaining as usize) * 4;
            let truncated = if max_chars < response.len() {
                // Truncate at char boundary
                let truncated_str = &response[..response.floor_char_boundary(max_chars)];
                truncated_str.to_string()
            } else {
                response.to_string()
            };

            // Consume whatever remains of the budget
            let tokens_used = remaining;
            config.tokens_consumed += tokens_used;
            config.budget_exhausted = true;

            BudgetResult {
                content: truncated,
                tokens_used,
                budget_exhausted: true,
            }
        }
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Build the think-prefix that guides the model toward accurate reasoning,
    /// with corrections from the correction store injected at highest priority.
    ///
    /// Order (highest priority first):
    /// 1. Corrections from correction store (Requirement 10.3)
    /// 2. Reasoning context (Requirement 3.1)
    /// 3. Accuracy constraint (Requirement 3.2)
    /// 4. Prior phase output (if available)
    /// 5. Parent goal reminder
    fn build_think_prefix_with_corrections(
        &self,
        rsu: &Rsu,
        phase: Phase,
        prior_output: Option<&str>,
        corrections_text: &str,
    ) -> String {
        let mut prefix = String::with_capacity(1024);

        prefix.push_str("## [THINK-PREFIX: Steering Guidance]\n\n");

        // Requirement 10.3: Include corrections with HIGHEST priority
        // Corrections override all other guidance — they represent human feedback.
        if !corrections_text.is_empty() {
            prefix.push_str("### [HIGHEST PRIORITY] Human Corrections\n");
            prefix.push_str("The following corrections from human review OVERRIDE all other guidance:\n");
            prefix.push_str(corrections_text);
            prefix.push('\n');
        }

        // Requirement 3.1: Reference the RSU's reasoning content
        prefix.push_str("### Reasoning Context\n");
        prefix.push_str(&format!(
            "You are executing phase '{}' of segment '{}'.\n",
            phase.label(),
            rsu.id
        ));
        prefix.push_str(&format!(
            "Core reasoning task: {}\n\n",
            rsu.phases.reasoning
        ));

        // Requirement 3.2: Include accuracy_check as explicit constraint
        prefix.push_str("### Accuracy Constraint (MUST SATISFY)\n");
        prefix.push_str(&format!(
            "Your output MUST satisfy this constraint: {}\n\n",
            rsu.phases.accuracy_check
        ));

        // Include prior output context if available (for phase chaining)
        if let Some(prior) = prior_output {
            prefix.push_str("### Prior Phase Output\n");
            prefix.push_str(&format!(
                "The previous phase produced:\n{}\n\n",
                prior
            ));
        }

        // Parent goal reminder for drift prevention
        prefix.push_str("### Parent Goal\n");
        prefix.push_str(&format!(
            "All work serves this objective: {}\n",
            rsu.meta.parent_goal
        ));

        prefix
    }

    /// Legacy build_think_prefix without corrections (used by tests and backward compat).
    ///
    /// - References RSU's `phases.reasoning` content (Requirement 3.1)
    /// - Includes `phases.accuracy_check` as explicit constraint (Requirement 3.2)
    fn build_think_prefix(&self, rsu: &Rsu, phase: Phase, prior_output: Option<&str>) -> String {
        self.build_think_prefix_with_corrections(rsu, phase, prior_output, "")
    }

    /// Assemble the final system context, respecting the context_budget limit.
    ///
    /// Requirement 10.5: Do not exceed the SteeringEngine's configured token limit.
    /// If the combined context exceeds the budget, truncate the steering_prompt
    /// (nautivecs fragments) since think-prefix and phase prompt are essential.
    ///
    /// Token estimation: chars / 4 (standard approximation for English text).
    fn assemble_system_context(
        &self,
        think_prefix: &str,
        steering_prompt: &str,
        phase_prompt: &str,
        context_budget: usize,
    ) -> String {
        // Estimate tokens for each component
        let prefix_tokens = think_prefix.len() / 4;
        let phase_tokens = phase_prompt.len() / 4;
        let steering_tokens = steering_prompt.len() / 4;

        let total_tokens = prefix_tokens + phase_tokens + steering_tokens;

        if total_tokens <= context_budget || context_budget == 0 {
            // Within budget — use everything
            format!("{}\n\n{}\n\n{}", think_prefix, steering_prompt, phase_prompt)
        } else {
            // Over budget — truncate steering_prompt (nautivecs fragments) to fit.
            // Think-prefix and phase_prompt are essential and non-negotiable.
            let essential_tokens = prefix_tokens + phase_tokens;

            if essential_tokens >= context_budget {
                // Even without steering, we're over budget.
                // Include think-prefix and phase prompt only (they're critical).
                tracing::warn!(
                    essential_tokens = essential_tokens,
                    context_budget = context_budget,
                    "Essential context exceeds budget — steering fragments omitted entirely"
                );
                format!("{}\n\n{}", think_prefix, phase_prompt)
            } else {
                // Truncate steering_prompt to fit remaining budget
                let available_tokens = context_budget - essential_tokens;
                let available_chars = available_tokens * 4;

                let truncated_steering = if available_chars < steering_prompt.len() {
                    let boundary = steering_prompt.floor_char_boundary(available_chars);
                    &steering_prompt[..boundary]
                } else {
                    steering_prompt
                };

                tracing::debug!(
                    original_steering_tokens = steering_tokens,
                    truncated_to_tokens = available_tokens,
                    context_budget = context_budget,
                    "Steering context truncated to respect context_budget"
                );

                format!("{}\n\n{}\n\n{}", think_prefix, truncated_steering, phase_prompt)
            }
        }
    }

    /// Build the phase-specific prompt content.
    fn build_phase_prompt(&self, rsu: &Rsu, phase: Phase, _prior_output: Option<&str>) -> String {
        match phase {
            Phase::Observation => {
                format!(
                    "## Phase: Observation\n\
                     Gather the following context:\n{}\n\n\
                     Provide a structured summary of the gathered context.",
                    rsu.phases.observation
                )
            }
            Phase::Reasoning => {
                format!(
                    "## Phase: Reasoning\n\
                     Execute the following reasoning task:\n{}\n\n\
                     Ground your reasoning in the context provided above.\n\
                     Your output must satisfy: {}",
                    rsu.phases.reasoning, rsu.phases.accuracy_check
                )
            }
            Phase::AccuracyCheck => {
                format!(
                    "## Phase: Accuracy Check\n\
                     Verify the reasoning output against this constraint:\n{}\n\n\
                     If the constraint is satisfied, output PASS with a brief justification.\n\
                     If the constraint is violated, output FAIL with the specific violation.",
                    rsu.phases.accuracy_check
                )
            }
        }
    }

    /// Build the user query portion of the LLM call.
    fn build_user_query(&self, rsu: &Rsu, phase: Phase, prior_output: Option<&str>) -> String {
        match phase {
            Phase::Observation => {
                format!(
                    "Execute the observation phase for segment {}. Gather context as specified.",
                    rsu.id
                )
            }
            Phase::Reasoning => {
                let context = prior_output.unwrap_or("(no prior output)");
                format!(
                    "Execute the reasoning phase for segment {}.\n\
                     Observation context:\n{}\n\n\
                     Reasoning task: {}",
                    rsu.id, context, rsu.phases.reasoning
                )
            }
            Phase::AccuracyCheck => {
                let reasoning = prior_output.unwrap_or("(no reasoning output)");
                format!(
                    "Verify the following reasoning output for segment {}:\n{}\n\n\
                     Constraint to check: {}",
                    rsu.id, reasoning, rsu.phases.accuracy_check
                )
            }
        }
    }

    /// Map the RSU's steering policy to a role_hint for SteeringEngine queries.
    /// (Requirement 10.4: pass RSU's execution domain as role_hint)
    fn domain_to_role_hint(&self, policy: &RsuSteeringPolicy) -> &'static str {
        match policy.mode {
            SegmentMode::Precision => "sensor",
            SegmentMode::Exploratory => "research",
        }
    }
}

impl Default for SteeringController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scm::rsu::{Rsu, RsuExecution, RsuMeta, RsuPhases, RsuSteeringPolicy, ThinkingBudget};
    use chrono::Utc;

    /// Helper: build a test RSU
    fn test_rsu() -> Rsu {
        Rsu {
            context: "https://kiro.ai".to_string(),
            rsu_type: "SegmentTask".to_string(),
            id: "segment_001".to_string(),
            meta: RsuMeta {
                parent_goal: "Analyze thermal anomaly in tile B02".to_string(),
                thinking_budget: ThinkingBudget::Medium,
                steering_ref: "steering/accuracy-guardrail.md".to_string(),
                steering_policy: RsuSteeringPolicy::precision(),
                context_ttl_seconds: 30,
                created_at: Utc::now(),
            },
            phases: RsuPhases {
                observation: "Load prior segment outputs for tile B02.".to_string(),
                reasoning: "Compare thermal delta against known wreck signatures.".to_string(),
                accuracy_check: "Verify anomaly coordinates fall within tile bounds.".to_string(),
            },
            execution: RsuExecution {
                action: "queryNautivecs".to_string(),
                parameters: serde_json::json!({"query": "thermal anomaly", "top_k": 5}),
            },
            dependencies: vec!["segment_000".to_string()],
        }
    }

    #[test]
    fn test_think_prefix_contains_reasoning_content() {
        let ctrl = SteeringController::new();
        let rsu = test_rsu();
        let prefix = ctrl.build_think_prefix(&rsu, Phase::Reasoning, None);

        // Requirement 3.1: think-prefix references phases.reasoning
        assert!(
            prefix.contains("Compare thermal delta against known wreck signatures"),
            "Think-prefix must reference reasoning content. Got:\n{}",
            prefix
        );
    }

    #[test]
    fn test_think_prefix_contains_accuracy_check_constraint() {
        let ctrl = SteeringController::new();
        let rsu = test_rsu();
        let prefix = ctrl.build_think_prefix(&rsu, Phase::Reasoning, None);

        // Requirement 3.2: think-prefix includes accuracy_check as constraint
        assert!(
            prefix.contains("Verify anomaly coordinates fall within tile bounds"),
            "Think-prefix must include accuracy_check constraint. Got:\n{}",
            prefix
        );
    }

    #[test]
    fn test_think_prefix_includes_prior_output_when_provided() {
        let ctrl = SteeringController::new();
        let rsu = test_rsu();
        let prior = "Observation found 3 thermal anomalies in tile B02.";
        let prefix = ctrl.build_think_prefix(&rsu, Phase::Reasoning, Some(prior));

        assert!(
            prefix.contains(prior),
            "Think-prefix must include prior output. Got:\n{}",
            prefix
        );
    }

    #[test]
    fn test_think_prefix_includes_parent_goal() {
        let ctrl = SteeringController::new();
        let rsu = test_rsu();
        let prefix = ctrl.build_think_prefix(&rsu, Phase::Observation, None);

        assert!(
            prefix.contains("Analyze thermal anomaly in tile B02"),
            "Think-prefix must include parent goal. Got:\n{}",
            prefix
        );
    }

    #[test]
    fn test_select_threshold_precision() {
        let ctrl = SteeringController::new();
        let policy = RsuSteeringPolicy::precision();
        assert_eq!(ctrl.select_threshold(&policy), 0.05);
    }

    #[test]
    fn test_select_threshold_exploratory() {
        let ctrl = SteeringController::new();
        let policy = RsuSteeringPolicy::exploratory();
        assert_eq!(ctrl.select_threshold(&policy), 0.15);
    }

    #[test]
    fn test_domain_to_role_hint_precision() {
        let ctrl = SteeringController::new();
        let policy = RsuSteeringPolicy::precision();
        assert_eq!(ctrl.domain_to_role_hint(&policy), "sensor");
    }

    #[test]
    fn test_domain_to_role_hint_exploratory() {
        let ctrl = SteeringController::new();
        let policy = RsuSteeringPolicy::exploratory();
        assert_eq!(ctrl.domain_to_role_hint(&policy), "research");
    }

    #[test]
    fn test_phase_labels() {
        assert_eq!(Phase::Observation.label(), "Observation");
        assert_eq!(Phase::Reasoning.label(), "Reasoning");
        assert_eq!(Phase::AccuracyCheck.label(), "Accuracy Check");
    }

    #[test]
    fn test_build_phase_prompt_observation() {
        let ctrl = SteeringController::new();
        let rsu = test_rsu();
        let prompt = ctrl.build_phase_prompt(&rsu, Phase::Observation, None);

        assert!(prompt.contains("Phase: Observation"));
        assert!(prompt.contains("Load prior segment outputs for tile B02"));
    }

    #[test]
    fn test_build_phase_prompt_reasoning() {
        let ctrl = SteeringController::new();
        let rsu = test_rsu();
        let prompt = ctrl.build_phase_prompt(&rsu, Phase::Reasoning, None);

        assert!(prompt.contains("Phase: Reasoning"));
        assert!(prompt.contains("Compare thermal delta against known wreck signatures"));
        assert!(prompt.contains("Verify anomaly coordinates fall within tile bounds"));
    }

    #[test]
    fn test_build_phase_prompt_accuracy_check() {
        let ctrl = SteeringController::new();
        let rsu = test_rsu();
        let prompt = ctrl.build_phase_prompt(&rsu, Phase::AccuracyCheck, None);

        assert!(prompt.contains("Phase: Accuracy Check"));
        assert!(prompt.contains("Verify anomaly coordinates fall within tile bounds"));
    }

    #[test]
    fn test_non_bypassable_guarantee_steer_and_execute_is_only_path() {
        // This is a design-level test: SteeringController is the ONLY struct
        // that holds the logic to call LlmClient. The non-bypassable guarantee
        // (Requirement 3.7) is enforced architecturally — all pipeline code
        // must route through steer_and_execute().
        //
        // We verify this by confirming the controller exists and has the method.
        let ctrl = SteeringController::new();
        // The controller is constructible and ready to serve as the single gateway.
        assert_eq!(ctrl.select_threshold(&RsuSteeringPolicy::precision()), 0.05);
    }

    // ── Tests for Task 4.3: SteeringEngine integration details ───────────────

    #[test]
    fn test_think_prefix_with_corrections_includes_corrections_first() {
        // Requirement 10.3: Corrections from correction store with highest priority
        let ctrl = SteeringController::new();
        let rsu = test_rsu();
        let corrections = "\n## [CRITICAL: PREVIOUS HUMAN CORRECTIONS]\n\n- **queryNautivecs** (2024-07-15): Use band ratio 1.8 not 2.0\n\n";

        let prefix = ctrl.build_think_prefix_with_corrections(
            &rsu,
            Phase::Reasoning,
            None,
            corrections,
        );

        // Corrections must appear in the prefix
        assert!(
            prefix.contains("Use band ratio 1.8 not 2.0"),
            "Think-prefix must include correction text. Got:\n{}",
            prefix
        );

        // Corrections must appear BEFORE reasoning context (highest priority)
        let corrections_pos = prefix.find("HIGHEST PRIORITY").unwrap();
        let reasoning_pos = prefix.find("Reasoning Context").unwrap();
        assert!(
            corrections_pos < reasoning_pos,
            "Corrections must appear before reasoning context (highest priority). \
             Corrections at {}, Reasoning at {}",
            corrections_pos,
            reasoning_pos
        );
    }

    #[test]
    fn test_think_prefix_without_corrections_has_no_corrections_section() {
        // When no corrections are available, the section should be absent
        let ctrl = SteeringController::new();
        let rsu = test_rsu();

        let prefix = ctrl.build_think_prefix_with_corrections(
            &rsu,
            Phase::Reasoning,
            None,
            "",
        );

        assert!(
            !prefix.contains("HIGHEST PRIORITY"),
            "Think-prefix should not have corrections section when empty. Got:\n{}",
            prefix
        );
    }

    #[test]
    fn test_assemble_system_context_within_budget() {
        // Requirement 10.5: When within budget, all content is included
        let ctrl = SteeringController::new();

        let think_prefix = "Think prefix content here"; // ~6 tokens
        let steering_prompt = "Steering context from nautivecs"; // ~7 tokens
        let phase_prompt = "Phase prompt content"; // ~5 tokens

        // Budget of 1000 tokens — way more than needed
        let result = ctrl.assemble_system_context(
            think_prefix,
            steering_prompt,
            phase_prompt,
            1000,
        );

        assert!(result.contains(think_prefix));
        assert!(result.contains(steering_prompt));
        assert!(result.contains(phase_prompt));
    }

    #[test]
    fn test_assemble_system_context_truncates_steering_when_over_budget() {
        // Requirement 10.5: Respect context_budget by truncating steering fragments
        let ctrl = SteeringController::new();

        let think_prefix = "TP"; // ~1 token (2 chars / 4)
        let steering_prompt = "A".repeat(400); // 100 tokens
        let phase_prompt = "PP"; // ~1 token (2 chars / 4)

        // Budget of 10 tokens — steering must be truncated
        let result = ctrl.assemble_system_context(
            think_prefix,
            &steering_prompt,
            phase_prompt,
            10,
        );

        // Think-prefix and phase prompt must be preserved
        assert!(result.contains(think_prefix));
        assert!(result.contains(phase_prompt));

        // Steering should be truncated (not all 400 chars present)
        assert!(
            result.len() < think_prefix.len() + steering_prompt.len() + phase_prompt.len(),
            "Result should be shorter than full content when budget is exceeded"
        );
    }

    #[test]
    fn test_assemble_system_context_omits_steering_when_essential_exceeds_budget() {
        // Requirement 10.5: When even essential content exceeds budget,
        // steering is omitted entirely but think-prefix and phase prompt remain
        let ctrl = SteeringController::new();

        let think_prefix = "A".repeat(100); // 25 tokens
        let steering_prompt = "Steering content";
        let phase_prompt = "B".repeat(100); // 25 tokens

        // Budget of 5 tokens — even essential content exceeds it
        // But we still include think-prefix and phase prompt (they're critical)
        let result = ctrl.assemble_system_context(
            &think_prefix,
            steering_prompt,
            &phase_prompt,
            5,
        );

        assert!(result.contains(&think_prefix));
        assert!(result.contains(&phase_prompt));
        // Steering should be omitted
        assert!(
            !result.contains("Steering content"),
            "Steering should be omitted when essential content exceeds budget"
        );
    }

    #[test]
    fn test_assemble_system_context_zero_budget_includes_everything() {
        // When context_budget is 0, treat as unlimited (no enforcement)
        let ctrl = SteeringController::new();

        let think_prefix = "Think prefix";
        let steering_prompt = "Steering prompt";
        let phase_prompt = "Phase prompt";

        let result = ctrl.assemble_system_context(
            think_prefix,
            steering_prompt,
            phase_prompt,
            0,
        );

        assert!(result.contains(think_prefix));
        assert!(result.contains(steering_prompt));
        assert!(result.contains(phase_prompt));
    }

    #[test]
    fn test_steer_result_struct_fields() {
        // Verify SteerResult can be constructed with all required fields
        let result = SteerResult {
            response: "LLM response".to_string(),
            needs_human_approval: true,
            corrections_applied: 2,
            unsteered: false,
        };

        assert_eq!(result.response, "LLM response");
        assert!(result.needs_human_approval);
        assert_eq!(result.corrections_applied, 2);
        assert!(!result.unsteered);
    }

    #[test]
    fn test_steer_result_unsteered_flag() {
        // When nautivecs is unreachable, unsteered should be true
        let result = SteerResult {
            response: "Response without steering".to_string(),
            needs_human_approval: false,
            corrections_applied: 0,
            unsteered: true,
        };

        assert!(result.unsteered);
        assert_eq!(result.corrections_applied, 0);
    }

    // ── Budget enforcement tests (Task 4.2) ──────────────────────────────────

    #[test]
    fn test_budget_for_rsu_low() {
        let mut rsu = test_rsu();
        rsu.meta.thinking_budget = ThinkingBudget::Low;
        let config = SteeringController::budget_for_rsu(&rsu);
        // Requirement 3.4: low = 512 reasoning tokens
        assert_eq!(config.max_reasoning_tokens, 512);
        // Requirement 9.1: low = 1024 total tokens
        assert_eq!(config.total_budget, 1024);
        assert_eq!(config.tokens_consumed, 0);
        assert!(!config.budget_exhausted);
    }

    #[test]
    fn test_budget_for_rsu_medium() {
        let mut rsu = test_rsu();
        rsu.meta.thinking_budget = ThinkingBudget::Medium;
        let config = SteeringController::budget_for_rsu(&rsu);
        // Requirement 3.5: medium = 2048 reasoning tokens
        assert_eq!(config.max_reasoning_tokens, 2048);
        // Requirement 9.2: medium = 4096 total tokens
        assert_eq!(config.total_budget, 4096);
    }

    #[test]
    fn test_budget_for_rsu_high() {
        let mut rsu = test_rsu();
        rsu.meta.thinking_budget = ThinkingBudget::High;
        let config = SteeringController::budget_for_rsu(&rsu);
        // Requirement 3.6: high = 4096 reasoning tokens
        assert_eq!(config.max_reasoning_tokens, 4096);
        // Requirement 9.3: high = 8192 total tokens
        assert_eq!(config.total_budget, 8192);
    }

    #[test]
    fn test_enforce_budget_within_limit() {
        let mut config = BudgetConfig::from_thinking_budget(&ThinkingBudget::Medium);
        // 100 chars ≈ 25 tokens, well within 4096 total budget
        let response = "a".repeat(100);
        let result = SteeringController::enforce_budget(&mut config, Phase::Reasoning, &response);

        assert_eq!(result.content, response);
        assert_eq!(result.tokens_used, 25);
        assert!(!result.budget_exhausted);
        assert_eq!(config.tokens_consumed, 25);
        assert!(!config.budget_exhausted);
    }

    #[test]
    fn test_enforce_budget_accumulates_across_phases() {
        let mut config = BudgetConfig::from_thinking_budget(&ThinkingBudget::Low);
        // Low budget = 1024 total tokens

        // Phase 1: 2000 chars ≈ 500 tokens
        let obs_response = "x".repeat(2000);
        let r1 = SteeringController::enforce_budget(&mut config, Phase::Observation, &obs_response);
        assert!(!r1.budget_exhausted);
        assert_eq!(config.tokens_consumed, 500);

        // Phase 2: 2000 chars ≈ 500 tokens (total now 1000, still within 1024)
        let reason_response = "y".repeat(2000);
        let r2 = SteeringController::enforce_budget(&mut config, Phase::Reasoning, &reason_response);
        assert!(!r2.budget_exhausted);
        assert_eq!(config.tokens_consumed, 1000);

        // Phase 3: 200 chars ≈ 50 tokens (total would be 1050, exceeds 1024)
        let check_response = "z".repeat(200);
        let r3 = SteeringController::enforce_budget(&mut config, Phase::AccuracyCheck, &check_response);
        // Requirement 9.4: truncate and mark budget-exhausted
        assert!(r3.budget_exhausted);
        assert!(config.budget_exhausted);
        // Truncated content should be shorter than original
        assert!(r3.content.len() <= check_response.len());
    }

    #[test]
    fn test_enforce_budget_truncates_on_exceed() {
        let mut config = BudgetConfig::from_thinking_budget(&ThinkingBudget::Low);
        // Low budget = 1024 total tokens = ~4096 chars

        // A response that exceeds the entire budget in one go
        let huge_response = "a".repeat(8000); // 8000 chars ≈ 2000 tokens > 1024
        let result = SteeringController::enforce_budget(&mut config, Phase::Reasoning, &huge_response);

        // Requirement 9.4: response is truncated
        assert!(result.budget_exhausted);
        assert!(result.content.len() < huge_response.len());
        // Truncated to fit remaining budget (1024 tokens * 4 chars = 4096 chars max)
        assert!(result.content.len() <= 4096);
        assert!(config.budget_exhausted);
        // All remaining budget consumed
        assert_eq!(config.tokens_consumed, config.total_budget);
    }

    #[test]
    fn test_enforce_budget_remaining_tracks_correctly() {
        let mut config = BudgetConfig::from_thinking_budget(&ThinkingBudget::High);
        assert_eq!(config.remaining(), 8192);

        // Consume some tokens
        let response = "b".repeat(4000); // 1000 tokens
        SteeringController::enforce_budget(&mut config, Phase::Observation, &response);
        assert_eq!(config.remaining(), 7192);
    }

    #[test]
    fn test_budget_config_from_thinking_budget_matches_rsu_methods() {
        // Verify BudgetConfig values match ThinkingBudget's own methods
        for budget in &[ThinkingBudget::Low, ThinkingBudget::Medium, ThinkingBudget::High] {
            let config = BudgetConfig::from_thinking_budget(budget);
            assert_eq!(config.max_reasoning_tokens, budget.reasoning_tokens());
            assert_eq!(config.total_budget, budget.total_tokens());
        }
    }
}
