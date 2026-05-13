//! SCM Executor — the async loop that ties everything together.
//!
//! Flow:
//! 1. Pre-process: ContextPruner strips context for hardware profile
//! 2. Dispatch: Send pruned RSU to LLM (Cake/KoboldCPP/any endpoint)
//! 3. Evaluate: DriftMonitor Step A (keyword) + Step B (1-token judge)
//! 4. React: FeedbackDispatcher → commit / retry / halt
//!
//! This is the "nerve center" that connects the SCM quality filter
//! to the actual LLM execution.

use anyhow::Result;

use crate::llm_client::LlmClient;
use crate::steering::SteeringEngine;

use super::drift::{DriftMonitor, HardwareProfile, SCMDriftMonitor, SteeringMode};
use super::feedback::{Action, FeedbackDispatcher, FidelityResult};
use super::pruner::ContextPruner;
use super::rsu::RsuTask;

/// The 1-token judge prompt — forces the model into "auditor" mode
pub const JUDGE_PROMPT_TEMPLATE: &str = r#"Evaluate the following output against these constraints:
Constraints: {constraints}
Output: {output}

On a scale of 1-5, where 5 is perfect fidelity and 1 is total drift, rate this segment.
Output ONLY the numeric digit.
Rating:"#;

/// Configuration for the SCM executor
pub struct ExecutorConfig {
    pub hardware: HardwareProfile,
    /// Whether to run Step B (LLM-as-judge) for borderline cases
    pub enable_step_b: bool,
    /// Drift score range that triggers Step B (borderline zone)
    pub step_b_range: (f64, f64), // (min, max) — e.g., (0.1, 0.4)
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            hardware: HardwareProfile::low_vram(),
            enable_step_b: true,
            step_b_range: (0.1, 0.4),
        }
    }
}

/// The SCM Executor — owns the async execution loop
pub struct SCMExecutor {
    config: ExecutorConfig,
    drift_monitor: SCMDriftMonitor,
}

impl SCMExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        let drift_monitor = SCMDriftMonitor::new(config.hardware.clone());
        Self { config, drift_monitor }
    }

    /// Execute a single RSU through the full SCM pipeline.
    ///
    /// Returns the committed output on success, or an error report on halt.
    pub async fn execute_rsu(
        &mut self,
        rsu: &mut RsuTask,
        llm: &LlmClient,
        steering: &mut SteeringEngine,
    ) -> Result<String> {
        loop {
            // Step 1: Build steered context from nautivecs
            let steering_context = steering
                .build_context(
                    &rsu.phases.reasoning,
                    Some(self.mode_to_role_hint(&rsu.metadata.steering_mode)),
                    Some(&rsu.id),
                )
                .await?;

            // Step 2: Prune context for hardware
            let mut full_prompt = format!(
                "{}\n\n## Task (Segment {})\n\
                Observation: {}\n\
                Reasoning task: {}\n\
                Accuracy constraint: {}\n\n\
                Provide your answer grounded in the code context above.",
                steering_context.system_prompt,
                rsu.id,
                rsu.phases.observation,
                rsu.phases.reasoning,
                rsu.phases.accuracy_check,
            );

            ContextPruner::prune_for_hardware(&mut full_prompt, &self.config.hardware);

            // Step 3: Dispatch to LLM
            let max_tokens = rsu.metadata.thinking_budget.max_tokens();
            let response = llm.steered_completion(&full_prompt, "").await?;

            // Step 4: Evaluate — Step A (keyword drift)
            let fidelity = self.drift_monitor.evaluate_fidelity(
                &response,
                &rsu.phases.accuracy_check,
                &rsu.metadata.steering_mode,
            );

            // Step 4b: If borderline and Step B enabled, run 1-token judge
            let final_fidelity = match &fidelity {
                FidelityResult::RetryNeeded { score, .. }
                    if self.config.enable_step_b
                        && *score >= self.config.step_b_range.0
                        && *score <= self.config.step_b_range.1 =>
                {
                    // Run the 1-token judge
                    let judge_score = self
                        .run_judge(llm, &response, &rsu.phases.accuracy_check)
                        .await;

                    if judge_score >= 4 {
                        // Judge says it's fine — override Step A
                        FidelityResult::Valid
                    } else {
                        // Judge confirms drift — keep the retry
                        fidelity
                    }
                }
                _ => fidelity,
            };

            // Step 5: React
            let action = FeedbackDispatcher::dispatch(final_fidelity, rsu);

            match action {
                Action::CommitSegment => {
                    rsu.output = Some(response.clone());
                    return Ok(response);
                }
                Action::RetrySegment => {
                    tracing::info!(
                        "SCM: Retrying segment {} (attempt {})",
                        rsu.id,
                        rsu.metadata.retries
                    );
                    continue; // Loop back with [STEERING ALERT] injected
                }
                Action::SignalInterrupt(reason) => {
                    return Err(anyhow::anyhow!(
                        "SCM HALT on segment {}: {}",
                        rsu.id,
                        reason
                    ));
                }
            }
        }
    }

    /// Run the 1-token judge — asks the model to rate fidelity 1-5
    async fn run_judge(&self, llm: &LlmClient, output: &str, constraints: &str) -> u8 {
        let prompt = JUDGE_PROMPT_TEMPLATE
            .replace("{constraints}", &constraints[..constraints.len().min(200)])
            .replace("{output}", &output[..output.len().min(300)]);

        match llm.steered_completion("", &prompt).await {
            Ok(response) => {
                // Parse the single digit from the response
                response
                    .trim()
                    .chars()
                    .find(|c| c.is_ascii_digit())
                    .and_then(|c| c.to_digit(10))
                    .unwrap_or(3) as u8 // Default to 3 (neutral) if parse fails
            }
            Err(_) => 3, // If judge call fails, assume neutral
        }
    }

    /// Map steering mode to a role hint for nautivecs queries
    fn mode_to_role_hint(&self, mode: &SteeringMode) -> &'static str {
        match mode {
            SteeringMode::Precision => "sensor",
            SteeringMode::Exploratory => "research",
        }
    }

    /// Get current system drift status
    pub fn system_drift_status(&self) -> (f64, bool) {
        let avg = self.drift_monitor.rolling_average();
        let needs_increase = self.drift_monitor.needs_increased_steering();
        (avg, needs_increase)
    }
}
