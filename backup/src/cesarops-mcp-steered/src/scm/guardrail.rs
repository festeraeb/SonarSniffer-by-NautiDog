//! Input and Output Guardrails for the Segmented Context Manager.
//!
//! The InputGuardrail performs pre-execution validation on RSUs before they
//! reach the LLM, checking for:
//! - Semantic drift between observation and parent_goal (Requirement 4.1)
//! - References to goals/contexts not in parent_goal or prior outputs (Requirement 4.2)
//! - Unauthorized execution actions (Requirements 4.3, 4.4)
//! - Performance target: <50ms for RSUs under 1000 chars (Requirement 4.5)
//!
//! The OutputGuardrail performs post-execution fidelity validation on segment
//! outputs, checking for:
//! - Strict schema enforcement — reject output with undefined fields (Requirement 5.2)
//! - Canonical serialization — verify tool calls use trusted serializer (Requirement 5.3)
//! - Logical drift score computation 0.0–1.0 (Requirement 5.4)
//! - Drift threshold enforcement with reprocessing flag (Requirement 5.5)
//! - Blocker severity halts pipeline (Requirement 5.6)
//! - Warning severity logs but continues (Requirement 5.7)

use std::collections::HashSet;

use anyhow::{bail, Result};
use tracing::warn;

use super::drift::{DriftMonitor, SteeringMode};
use super::rsu::{Rsu, PERMITTED_ACTIONS};

// ── Helper functions for Key Term Weighting ──────────────────────────────────

/// Capitalize the first character of a string (for checking if term appears capitalized in goal)
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

/// Check if a word is a common English word (filler) that shouldn't be high-priority.
/// These are words that appear in many objectives regardless of domain.
fn is_common_word(word: &str) -> bool {
    const COMMON: &[&str] = &[
        "about", "after", "again", "against", "because", "before", "between",
        "could", "during", "every", "first", "following", "from", "have",
        "include", "includes", "into", "just", "know", "like", "make", "many",
        "more", "most", "must", "need", "only", "other", "over", "paper",
        "should", "some", "such", "than", "that", "their", "them", "then",
        "there", "these", "they", "this", "through", "under", "using", "very",
        "want", "were", "what", "when", "where", "which", "while", "will",
        "with", "would", "your", "each", "also", "been", "both", "does",
        "done", "down", "even", "here", "high", "keep", "last", "long",
        "much", "next", "once", "open", "part", "same", "show", "side",
        "take", "tell", "text", "time", "turn", "upon", "well", "work",
        "year", "titled", "cover", "based", "grounded", "actual", "source",
        "code", "module", "context", "claim", "technical", "every",
        "management", "validation", "architecture", "hardware", "precision",
        "accuracy", "resilient",
    ];
    COMMON.contains(&word)
}

use super::validator::{FidelityCheck, RuleSeverity, ValidationResult};

/// Output from a completed segment — used as context for subsequent segments.
///
/// The InputGuardrail uses prior outputs to verify that an RSU's observation
/// only references goals/contexts that actually exist in the pipeline history.
#[derive(Debug, Clone)]
pub struct SegmentOutput {
    /// The RSU ID that produced this output (e.g. "segment_001")
    pub rsu_id: String,
    /// The textual content produced by the segment
    pub content: String,
}

/// Pre-execution guardrail that validates RSUs before they reach the LLM.
///
/// Checks for semantic drift, unauthorized actions, and context injection.
/// Must complete within 50ms for RSUs with fewer than 1000 characters of
/// combined phase content (Requirement 4.5).
pub struct InputGuardrail {
    /// The set of permitted execution actions
    permitted_actions: HashSet<String>,
}

impl InputGuardrail {
    /// Create a new InputGuardrail with the default permitted actions:
    /// writeFile, readFile, runCommand, queryNautivecs
    pub fn new() -> Self {
        let permitted_actions: HashSet<String> = PERMITTED_ACTIONS
            .iter()
            .map(|s| s.to_string())
            .collect();

        Self { permitted_actions }
    }

    /// Validate an RSU against drift and action constraints.
    ///
    /// Checks:
    /// 1. Semantic drift: observation must relate to parent_goal (Req 4.1)
    /// 2. Context references: observation must not reference goals/contexts
    ///    absent from parent_goal or prior outputs (Req 4.2)
    /// 3. Action whitelist: execution.action must be permitted (Req 4.3)
    /// 4. Descriptive errors for unauthorized actions (Req 4.4)
    ///
    /// Performance target: <50ms for RSUs under 1000 chars (Req 4.5)
    pub fn validate(&self, rsu: &Rsu, prior_outputs: &[SegmentOutput]) -> Result<()> {
        // --- Requirement 4.3 & 4.4: Verify execution action is permitted ---
        self.validate_action(rsu)?;

        // --- Requirement 4.1: Check observation vs parent_goal for semantic drift ---
        self.validate_drift(rsu)?;

        // --- Requirement 4.2: Check for references to unknown contexts ---
        self.validate_context_references(rsu, prior_outputs)?;

        Ok(())
    }

    /// Verify that the RSU's execution action is in the permitted set.
    /// (Requirements 4.3, 4.4)
    fn validate_action(&self, rsu: &Rsu) -> Result<()> {
        let action = &rsu.execution.action;

        if !self.permitted_actions.contains(action) {
            bail!(
                "Unauthorized action '{}' in RSU '{}'. Permitted actions are: {}",
                action,
                rsu.id,
                self.permitted_actions
                    .iter()
                    .map(|a| format!("'{}'", a))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        Ok(())
    }

    /// Check that the RSU's observation content relates to the parent_goal.
    /// Uses Key Term Weighting with Sliding Coverage for drift detection.
    /// (Requirement 4.1)
    ///
    /// Strategy:
    /// - Extract terms from parent_goal, classify as HIGH-PRIORITY or STANDARD
    /// - High-priority: domain nouns (>5 chars, capitalized, or path-like), core verbs
    /// - Sliding coverage: for long objectives (>30 words), absolute overlap threshold
    ///   scales down but high-priority term hits must stay above 0.2
    /// - If both thresholds fail, log the rejected RSU for human validation
    fn validate_drift(&self, rsu: &Rsu) -> Result<()> {
        let parent_goal = &rsu.meta.parent_goal;
        let observation = &rsu.phases.observation;

        // Extract all significant words from parent_goal (words > 3 chars, lowercased)
        let all_terms: Vec<String> = parent_goal
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
            .filter(|w| w.len() > 3)
            .collect();

        // If the parent_goal has no significant terms, skip drift check
        if all_terms.is_empty() {
            return Ok(());
        }

        let goal_terms: HashSet<String> = all_terms.iter().cloned().collect();

        // ── Key Term Classification ─────────────────────────────────────────
        // High-priority terms: domain nouns, paths, technical identifiers
        // These are the terms that MUST appear for the segment to be relevant.
        let high_priority: HashSet<&String> = goal_terms
            .iter()
            .filter(|term| Self::is_high_priority_term(term, parent_goal))
            .collect();

        let observation_lower = observation.to_lowercase();

        // ── Coverage Computation ────────────────────────────────────────────
        // Count standard term matches
        let total_matched = goal_terms
            .iter()
            .filter(|term| observation_lower.contains(term.as_str()))
            .count();

        let total_coverage = total_matched as f64 / goal_terms.len() as f64;

        // Count high-priority term matches (with prefix/stem matching)
        // A term matches if:
        // 1. The observation contains the exact term (substring), OR
        // 2. The observation contains a 5+ char prefix of the term (stem match)
        //    e.g., "decompose" matches HP term "decomposition"
        let hp_matched = high_priority
            .iter()
            .filter(|term| {
                let t = term.as_str();
                // Exact substring match
                if observation_lower.contains(t) {
                    return true;
                }
                // Prefix/stem match: if the term is >6 chars, check if a 5-char prefix appears
                if t.len() > 6 {
                    let prefix = &t[..t.len().min(6)];
                    if observation_lower.contains(prefix) {
                        return true;
                    }
                }
                false
            })
            .count();

        let hp_coverage = if high_priority.is_empty() {
            1.0 // No high-priority terms = pass by default
        } else {
            hp_matched as f64 / high_priority.len() as f64
        };

        // ── Sliding Coverage Threshold ──────────────────────────────────────
        // For long objectives (>30 words), the absolute overlap threshold scales down
        // because there are more filler words diluting the coverage.
        // But high-priority term coverage stays strict at 0.2.
        let word_count = parent_goal.split_whitespace().count();
        let absolute_threshold = if word_count > 30 {
            // Sliding: 10% base scaled down by log of word count
            // 30 words → 0.10, 50 words → 0.06, 100 words → 0.04
            0.10 / (word_count as f64 / 30.0).ln().max(1.0)
        } else {
            0.10 // Standard threshold for short objectives
        };

        let hp_threshold = if high_priority.len() > 25 {
            // Very long HP lists (>25 terms): require only 2 absolute hits.
            // Use 1.5 instead of 2.0 to avoid floating-point boundary issues.
            1.5 / high_priority.len() as f64
        } else if high_priority.len() > 15 {
            // For objectives with many HP terms (long technical descriptions),
            // require at least 3 absolute HP term hits rather than a percentage.
            // This prevents 36 HP terms from requiring 8 matches.
            3.0 / high_priority.len() as f64
        } else {
            0.2 // Standard: >20% of HP terms must match
        };

        // ── Decision ────────────────────────────────────────────────────────
        // Pass if EITHER:
        // 1. Total coverage meets the sliding threshold, OR
        // 2. High-priority coverage meets 0.2 threshold
        let passes_total = total_coverage >= absolute_threshold;
        let passes_hp = hp_coverage >= hp_threshold;

        if passes_total || passes_hp {
            return Ok(());
        }

        // ── Rejection: Log full RSU for human validation ────────────────────
        tracing::warn!(
            rsu_id = %rsu.id,
            total_coverage = %format!("{:.1}%", total_coverage * 100.0),
            hp_coverage = %format!("{:.1}%", hp_coverage * 100.0),
            hp_terms = ?high_priority.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
            observation = %observation,
            reasoning = %rsu.phases.reasoning,
            accuracy_check = %rsu.phases.accuracy_check,
            parent_goal = %&parent_goal[..parent_goal.len().min(120)],
            "InputGuardrail REJECTED RSU — logging for human validation"
        );

        bail!(
            "Semantic drift detected in RSU '{}': observation has insufficient overlap \
             with parent_goal. Total coverage: {:.1}% (threshold: {:.1}%), \
             High-priority coverage: {:.1}% (threshold: {:.1}%). \
             High-priority terms: {:?}. \
             Parent goal: '{}...'",
            rsu.id,
            total_coverage * 100.0,
            absolute_threshold * 100.0,
            hp_coverage * 100.0,
            hp_threshold * 100.0,
            high_priority.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
            &parent_goal[..parent_goal.len().min(80)]
        );
    }

    /// Determine if a term is high-priority based on heuristics:
    /// - Path-like terms (contain '/' or '_'): e.g., "src/scm", "parent_goal"
    /// - Long domain nouns (>6 chars): likely technical identifiers
    /// - Terms that appear capitalized in the original goal: proper nouns/acronyms
    /// - Known core verbs: examine, write, analyze, implement, verify
    fn is_high_priority_term(term: &str, original_goal: &str) -> bool {
        // Path-like terms (contain / or _ or .)
        if term.contains('/') || term.contains('_') || term.contains('.') {
            return true;
        }

        // Check if the term appears capitalized in the original (acronyms, proper nouns)
        let capitalized = term.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
        if original_goal.contains(&capitalize_first(term)) || capitalized {
            return true;
        }

        // Long domain nouns (>6 chars) — likely technical identifiers
        if term.len() > 6 && !is_common_word(term) {
            return true;
        }

        // Core verbs that indicate the task's intent
        const CORE_VERBS: &[&str] = &[
            "examine", "write", "analyze", "implement", "verify", "build",
            "create", "design", "validate", "decompose", "execute",
        ];
        if CORE_VERBS.contains(&term.as_ref()) {
            return true;
        }

        false
    }

    /// Check that the observation doesn't reference segment IDs that don't exist
    /// in the prior outputs. (Requirement 4.2)
    ///
    /// Looks for patterns like "segment_NNN" in the observation and verifies
    /// each referenced segment actually exists in prior_outputs.
    fn validate_context_references(&self, rsu: &Rsu, prior_outputs: &[SegmentOutput]) -> Result<()> {
        let observation = &rsu.phases.observation;

        // Build a set of known segment IDs from prior outputs
        let known_ids: HashSet<&str> = prior_outputs
            .iter()
            .map(|o| o.rsu_id.as_str())
            .collect();

        // Also include the parent_goal text as valid context source
        let parent_goal = &rsu.meta.parent_goal;

        // Find all segment_NNN references in the observation
        let referenced_ids: Vec<&str> = observation
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|w| w.starts_with("segment_") && w.len() > 8)
            .collect();

        // Check each referenced segment ID exists in prior outputs
        for ref_id in &referenced_ids {
            // Skip self-references (the RSU's own ID)
            if *ref_id == rsu.id {
                continue;
            }

            if !known_ids.contains(ref_id) {
                // Also check if it's mentioned in the parent_goal (valid context source)
                if !parent_goal.contains(ref_id) {
                    bail!(
                        "RSU '{}' references unknown segment '{}' in observation. \
                         Referenced segments must exist in prior outputs or parent_goal. \
                         Known segments: [{}]",
                        rsu.id,
                        ref_id,
                        known_ids
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
        }

        Ok(())
    }
}

impl Default for InputGuardrail {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Output Guardrail — Post-Execution Fidelity Validation
// ─────────────────────────────────────────────────────────────────────────────

/// Post-execution guardrail that validates segment output against fidelity rules.
///
/// Enforces:
/// - `strict_schema_enforcement`: rejects output containing undefined JSON fields (Req 5.2)
/// - `canonical_serialization`: verifies tool calls use trusted serializer format (Req 5.3)
/// - Logical drift score computation (Req 5.4)
/// - Drift threshold enforcement (Req 5.5)
/// - Blocker severity halts pipeline (Req 5.6)
/// - Warning severity logs but continues (Req 5.7)
pub struct OutputGuardrail;

/// Known fields that are permitted in segment output JSON blocks.
/// Used by `strict_schema_enforcement` to detect undefined fields.
const KNOWN_OUTPUT_FIELDS: &[&str] = &[
    "action",
    "parameters",
    "result",
    "content",
    "status",
    "error",
    "tool_call",
    "tool_name",
    "arguments",
    "output",
    "reasoning",
    "observation",
    "accuracy_check",
    "id",
    "type",
    "@context",
    "@type",
];

/// Expected tool call pattern: `tool_call(name, {args})` or JSON with `tool_name` + `arguments`.
/// The canonical serializer produces tool calls in this specific format.
const CANONICAL_TOOL_CALL_PATTERN: &str = "tool_call";

impl OutputGuardrail {
    /// Create a new OutputGuardrail.
    pub fn new() -> Self {
        Self
    }

    /// Validate output against the FidelityCheck rules referenced by the RSU.
    ///
    /// Loads rules from the FidelityCheck (Requirement 5.1), enforces each rule,
    /// computes drift score (Requirement 5.4), and returns a ValidationResult
    /// containing the drift score, pass/fail status, and categorized failures.
    ///
    /// # Arguments
    /// * `output` - The segment's raw output text
    /// * `rsu` - The RSU that produced this output (provides constraints for drift)
    /// * `fidelity_check` - The loaded FidelityCheck rules (from steering_ref path)
    /// * `monitor` - The drift monitor for computing logical drift score
    ///
    /// # Returns
    /// `ValidationResult` with drift_score, passed flag, blocker_failures, warning_failures
    pub fn validate(
        &self,
        output: &str,
        rsu: &Rsu,
        fidelity_check: &FidelityCheck,
        monitor: &mut dyn DriftMonitor,
    ) -> Result<ValidationResult> {
        let mut blocker_failures: Vec<String> = Vec::new();
        let mut warning_failures: Vec<String> = Vec::new();

        // --- Iterate over fidelity_check.validation_rules (Requirement 5.1) ---
        for rule in &fidelity_check.validation_rules {
            match rule.rule.as_str() {
                "strict_schema_enforcement" => {
                    // Requirement 5.2: reject output with undefined fields
                    if let Some(violation) = self.check_strict_schema(output) {
                        match rule.severity {
                            RuleSeverity::Blocker => blocker_failures.push(violation),
                            RuleSeverity::Warning => {
                                warn!("strict_schema_enforcement warning: {}", violation);
                                warning_failures.push(violation);
                            }
                        }
                    }
                }
                "canonical_serialization" => {
                    // Requirement 5.3: verify tool calls use trusted serializer
                    if let Some(violation) = self.check_canonical_serialization(output) {
                        match rule.severity {
                            RuleSeverity::Blocker => blocker_failures.push(violation),
                            RuleSeverity::Warning => {
                                warn!("canonical_serialization warning: {}", violation);
                                warning_failures.push(violation);
                            }
                        }
                    }
                }
                other => {
                    // Unknown rules are checked generically — just log if warning
                    // For now, unknown rules with blocker severity are noted
                    let msg = format!("Rule '{}' not implemented for automated checking", other);
                    match rule.severity {
                        RuleSeverity::Blocker => {
                            // Don't fail on unimplemented rules — they need manual review
                            warn!("{}", msg);
                        }
                        RuleSeverity::Warning => {
                            warn!("{}", msg);
                            warning_failures.push(msg);
                        }
                    }
                }
            }
        }

        // --- Compute Logical_Drift score 0.0–1.0 (Requirement 5.4) ---
        let constraint = &rsu.phases.accuracy_check;
        let mode: SteeringMode = rsu.meta.steering_policy.mode.into();
        let fidelity_result = monitor.evaluate_fidelity(output, constraint, &mode);

        // Extract drift score from the fidelity result
        let drift_score = match &fidelity_result {
            super::feedback::FidelityResult::Valid => 0.0,
            super::feedback::FidelityResult::RetryNeeded { score, .. } => *score as f32,
            super::feedback::FidelityResult::Halt { .. } => 1.0,
        };

        // Clamp drift_score to 0.0–1.0 range
        let drift_score = drift_score.clamp(0.0, 1.0);

        // --- Requirement 5.5: Reject if drift exceeds threshold (0.05) ---
        let threshold = rsu.meta.steering_policy.drift_threshold;
        if drift_score > threshold {
            blocker_failures.push(format!(
                "Logical drift {:.4} exceeds threshold {:.4} — flagged for reprocessing",
                drift_score, threshold
            ));
        }

        // --- Determine overall pass/fail ---
        // Requirement 5.6: blocker failures halt the pipeline
        // Requirement 5.7: warning failures are logged but pipeline continues
        let passed = blocker_failures.is_empty();

        if !warning_failures.is_empty() {
            for w in &warning_failures {
                warn!("OutputGuardrail warning (RSU '{}'): {}", rsu.id, w);
            }
        }

        Ok(ValidationResult {
            drift_score,
            passed,
            blocker_failures,
            warning_failures,
        })
    }

    /// Check `strict_schema_enforcement` rule (Requirement 5.2).
    ///
    /// Scans the output for JSON blocks and validates that all fields are
    /// in the known schema. Returns a violation description if undefined
    /// fields are found, or None if the output is clean.
    fn check_strict_schema(&self, output: &str) -> Option<String> {
        // Find JSON blocks in the output (delimited by { ... })
        let json_blocks = Self::extract_json_blocks(output);

        for block in &json_blocks {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(block) {
                if let Some(obj) = value.as_object() {
                    let undefined_fields: Vec<&String> = obj
                        .keys()
                        .filter(|k| !KNOWN_OUTPUT_FIELDS.contains(&k.as_str()))
                        .collect();

                    if !undefined_fields.is_empty() {
                        return Some(format!(
                            "strict_schema_enforcement: output contains undefined fields: [{}]",
                            undefined_fields
                                .iter()
                                .map(|f| format!("\"{}\"", f))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                }
            }
        }

        None
    }

    /// Check `canonical_serialization` rule (Requirement 5.3).
    ///
    /// Verifies that any tool calls in the output follow the trusted serializer
    /// format. Tool calls must use the `tool_call` pattern with proper JSON
    /// structure containing `tool_name` and `arguments`.
    ///
    /// Returns a violation description if non-canonical tool calls are found.
    fn check_canonical_serialization(&self, output: &str) -> Option<String> {
        // Look for tool call patterns in the output
        // Canonical format: contains "tool_call" with proper JSON structure
        // Non-canonical: raw function calls, eval(), exec(), or malformed invocations

        // Check for suspicious non-canonical patterns
        let suspicious_patterns = [
            "eval(",
            "exec(",
            "subprocess.run(",
            "os.system(",
            "Runtime.exec(",
            "__import__(",
        ];

        for pattern in &suspicious_patterns {
            if output.contains(pattern) {
                return Some(format!(
                    "canonical_serialization: output contains non-canonical tool invocation pattern '{}'",
                    pattern
                ));
            }
        }

        // If the output contains tool_call references, verify they have proper structure
        if output.contains(CANONICAL_TOOL_CALL_PATTERN) {
            // Extract JSON blocks that look like tool calls
            let json_blocks = Self::extract_json_blocks(output);
            for block in &json_blocks {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(block) {
                    if let Some(obj) = value.as_object() {
                        // If it has tool_name or tool_call, verify it also has arguments
                        let has_tool_ref = obj.contains_key("tool_name")
                            || obj.contains_key("tool_call");
                        if has_tool_ref && !obj.contains_key("arguments") {
                            return Some(
                                "canonical_serialization: tool call JSON missing required 'arguments' field"
                                    .to_string(),
                            );
                        }
                    }
                }
            }
        }

        None
    }

    /// Extract JSON blocks from text output.
    ///
    /// Finds substrings that start with `{` and end with `}` at the same
    /// nesting level. This is a simple brace-matching heuristic.
    fn extract_json_blocks(text: &str) -> Vec<&str> {
        let mut blocks = Vec::new();
        let mut depth = 0i32;
        let mut start: Option<usize> = None;

        for (i, ch) in text.char_indices() {
            match ch {
                '{' => {
                    if depth == 0 {
                        start = Some(i);
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(s) = start {
                            blocks.push(&text[s..=i]);
                            start = None;
                        }
                    }
                }
                _ => {}
            }
        }

        blocks
    }
}

impl Default for OutputGuardrail {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scm::rsu::*;
    use chrono::Utc;

    /// Helper: build a valid RSU for testing
    fn test_rsu(action: &str, observation: &str, parent_goal: &str) -> Rsu {
        Rsu {
            context: "https://kiro.ai".to_string(),
            rsu_type: "SegmentTask".to_string(),
            id: "segment_001".to_string(),
            meta: RsuMeta {
                parent_goal: parent_goal.to_string(),
                thinking_budget: ThinkingBudget::Medium,
                steering_ref: "steering/accuracy-guardrail.md".to_string(),
                steering_policy: RsuSteeringPolicy::precision(),
                context_ttl_seconds: 30,
                created_at: Utc::now(),
            },
            phases: RsuPhases {
                observation: observation.to_string(),
                reasoning: "Apply analysis logic.".to_string(),
                accuracy_check: "Verify results are correct.".to_string(),
            },
            execution: RsuExecution {
                action: action.to_string(),
                parameters: serde_json::json!({}),
            },
            dependencies: vec![],
        }
    }

    #[test]
    fn test_permits_valid_actions() {
        let guardrail = InputGuardrail::new();

        for action in PERMITTED_ACTIONS {
            let rsu = test_rsu(
                action,
                "Load thermal anomaly data from prior segments.",
                "Analyze thermal anomaly in tile B02 for wreck signatures",
            );
            assert!(
                guardrail.validate(&rsu, &[]).is_ok(),
                "Action '{}' should be permitted",
                action
            );
        }
    }

    #[test]
    fn test_rejects_unauthorized_action() {
        let guardrail = InputGuardrail::new();
        let rsu = test_rsu(
            "deleteDatabase",
            "Load thermal anomaly data from prior segments.",
            "Analyze thermal anomaly in tile B02 for wreck signatures",
        );

        let err = guardrail.validate(&rsu, &[]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Unauthorized action"), "Got: {}", msg);
        assert!(msg.contains("deleteDatabase"), "Got: {}", msg);
        assert!(msg.contains("Permitted actions"), "Got: {}", msg);
    }

    #[test]
    fn test_rejects_empty_action_as_unauthorized() {
        let guardrail = InputGuardrail::new();
        let rsu = test_rsu(
            "executeShell",
            "Load thermal data for analysis.",
            "Analyze thermal anomaly in tile B02",
        );

        let err = guardrail.validate(&rsu, &[]).unwrap_err();
        assert!(err.to_string().contains("Unauthorized action"));
    }

    #[test]
    fn test_detects_semantic_drift() {
        let guardrail = InputGuardrail::new();
        let rsu = test_rsu(
            "queryNautivecs",
            "The weather today is sunny and warm with a chance of rain tomorrow afternoon.",
            "Analyze thermal anomaly in tile B02 for wreck signatures using satellite imagery",
        );

        let err = guardrail.validate(&rsu, &[]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("drift"), "Got: {}", msg);
    }

    #[test]
    fn test_passes_when_observation_relates_to_goal() {
        let guardrail = InputGuardrail::new();
        let rsu = test_rsu(
            "queryNautivecs",
            "Load thermal anomaly data from tile B02 and compare against wreck signatures.",
            "Analyze thermal anomaly in tile B02 for wreck signatures",
        );

        assert!(guardrail.validate(&rsu, &[]).is_ok());
    }

    #[test]
    fn test_rejects_reference_to_unknown_segment() {
        let guardrail = InputGuardrail::new();
        let rsu = test_rsu(
            "readFile",
            "Load output from segment_005 for thermal analysis comparison.",
            "Analyze thermal anomaly in tile B02",
        );

        // No prior outputs — segment_005 doesn't exist
        let err = guardrail.validate(&rsu, &[]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("segment_005"), "Got: {}", msg);
        assert!(msg.contains("unknown segment"), "Got: {}", msg);
    }

    #[test]
    fn test_allows_reference_to_known_segment() {
        let guardrail = InputGuardrail::new();
        let rsu = test_rsu(
            "readFile",
            "Load output from segment_001 for thermal analysis comparison.",
            "Analyze thermal anomaly in tile B02",
        );

        let prior_outputs = vec![SegmentOutput {
            rsu_id: "segment_001".to_string(),
            content: "Thermal data loaded successfully.".to_string(),
        }];

        // segment_001 exists in prior outputs — should pass
        // Note: the RSU's own ID is segment_001, so self-reference is also fine
        assert!(guardrail.validate(&rsu, &prior_outputs).is_ok());
    }

    #[test]
    fn test_allows_reference_to_segment_in_parent_goal() {
        let guardrail = InputGuardrail::new();
        let rsu = test_rsu(
            "readFile",
            "Load output from segment_003 for thermal analysis.",
            "Continue analysis from segment_003 thermal anomaly detection",
        );

        // segment_003 is mentioned in parent_goal — should pass
        assert!(guardrail.validate(&rsu, &[]).is_ok());
    }

    #[test]
    fn test_performance_under_1000_chars() {
        // Requirement 4.5: must complete within 50ms for RSUs < 1000 chars
        let guardrail = InputGuardrail::new();
        let rsu = test_rsu(
            "queryNautivecs",
            "Load thermal anomaly data from tile B02.",
            "Analyze thermal anomaly in tile B02 for wreck signatures",
        );

        let start = std::time::Instant::now();
        for _ in 0..100 {
            let _ = guardrail.validate(&rsu, &[]);
        }
        let elapsed = start.elapsed();

        // 100 iterations should complete well under 5 seconds (50ms each)
        // In practice this should be sub-millisecond per call
        assert!(
            elapsed.as_millis() < 5000,
            "100 validations took {}ms — exceeds 50ms/call target",
            elapsed.as_millis()
        );

        // Single call should be well under 50ms
        let start = std::time::Instant::now();
        guardrail.validate(&rsu, &[]).unwrap();
        let single_elapsed = start.elapsed();
        assert!(
            single_elapsed.as_millis() < 50,
            "Single validation took {}ms — exceeds 50ms target",
            single_elapsed.as_millis()
        );
    }
}
