//! Cross-segment validation and FidelityCheck schema types.
//!
//! Defines the FidelityCheck JSON-LD schema for post-execution fidelity validation,
//! including validation rules with severity levels and monitoring configuration.
//! Also provides the CrossValidationResult for inter-segment consistency checks.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Severity level for a fidelity rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleSeverity {
    /// Pipeline halts immediately if this rule fails.
    Blocker,
    /// Violation is logged but pipeline continues.
    Warning,
}

/// A single validation rule within a FidelityCheck.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    pub rule: String,
    pub description: String,
    pub severity: RuleSeverity,
}

/// Monitoring configuration within a FidelityCheck.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// Drift threshold (0.0–1.0). Overridden by RSU's steering_policy.
    pub threshold: f32,
    /// Number of segments in the rolling average window.
    pub rolling_window: usize,
    /// Rolling average threshold that triggers increased steering.
    pub escalation_threshold: f32,
}

/// The complete FidelityCheck schema (JSON-LD).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FidelityCheck {
    #[serde(rename = "@context")]
    pub context: String, // "https://kiro.ai"
    #[serde(rename = "@type")]
    pub check_type: String, // "FidelityCheck"
    pub validation_rules: Vec<ValidationRule>,
    pub monitoring: MonitoringConfig,
}

impl FidelityCheck {
    /// Serialize this FidelityCheck to a JSON-LD serde_json::Value.
    ///
    /// Ensures `@context` is set to `https://kiro.ai` and `@type` is set to `FidelityCheck`.
    pub fn to_json_ld(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self)
            .expect("FidelityCheck serialization should never fail");

        // Guarantee JSON-LD fields are correct regardless of struct field values
        if let Some(obj) = value.as_object_mut() {
            obj.insert(
                "@context".to_string(),
                serde_json::Value::String("https://kiro.ai".to_string()),
            );
            obj.insert(
                "@type".to_string(),
                serde_json::Value::String("FidelityCheck".to_string()),
            );
        }

        value
    }

    /// Parse a FidelityCheck from a JSON-LD value with full validation.
    ///
    /// Validates:
    /// - Each validation_rule has `rule`, `description`, and `severity` fields
    /// - Severity is only "blocker" or "warning"
    /// - monitoring.threshold is between 0.0 and 1.0 inclusive
    ///
    /// Returns descriptive errors identifying which field is missing or invalid.
    pub fn from_json_ld(value: &serde_json::Value) -> Result<FidelityCheck> {
        let obj = value
            .as_object()
            .context("FidelityCheck payload must be a JSON object")?;

        // Validate validation_rules array exists and each rule is complete
        let rules_value = obj
            .get("validation_rules")
            .context("FidelityCheck is missing required field 'validation_rules'")?;

        let rules_array = rules_value
            .as_array()
            .context("FidelityCheck 'validation_rules' must be an array")?;

        for (i, rule_value) in rules_array.iter().enumerate() {
            let rule_obj = rule_value.as_object().with_context(|| {
                format!("validation_rules[{}] must be a JSON object", i)
            })?;

            // Check required fields
            if !rule_obj.contains_key("rule") {
                bail!(
                    "validation_rules[{}]: missing required field 'rule'",
                    i
                );
            }
            if !rule_obj.contains_key("description") {
                bail!(
                    "validation_rules[{}]: missing required field 'description'",
                    i
                );
            }
            if !rule_obj.contains_key("severity") {
                bail!(
                    "validation_rules[{}]: missing required field 'severity'",
                    i
                );
            }

            // Validate severity value
            let severity_str = rule_obj["severity"].as_str().with_context(|| {
                format!(
                    "validation_rules[{}]: 'severity' must be a string",
                    i
                )
            })?;

            if severity_str != "blocker" && severity_str != "warning" {
                bail!(
                    "validation_rules[{}]: invalid severity '{}' — must be 'blocker' or 'warning'",
                    i,
                    severity_str
                );
            }
        }

        // Validate monitoring section
        let monitoring_value = obj
            .get("monitoring")
            .context("FidelityCheck is missing required field 'monitoring'")?;

        let monitoring_obj = monitoring_value
            .as_object()
            .context("FidelityCheck 'monitoring' must be a JSON object")?;

        if let Some(threshold_value) = monitoring_obj.get("threshold") {
            let threshold = threshold_value.as_f64().with_context(|| {
                "monitoring.threshold must be a number"
            })? as f32;

            if threshold < 0.0 || threshold > 1.0 {
                bail!(
                    "monitoring.threshold must be between 0.0 and 1.0 inclusive, got {}",
                    threshold
                );
            }
        }

        // All validations passed — deserialize via serde
        let check: FidelityCheck = serde_json::from_value(value.clone())
            .context("Failed to deserialize FidelityCheck from JSON-LD payload")?;

        Ok(check)
    }
}

/// Validation result from the output guardrail.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub drift_score: f32,
    pub passed: bool,
    pub blocker_failures: Vec<String>,
    pub warning_failures: Vec<String>,
}

/// Result from cross-segment validation.
#[derive(Debug, Clone)]
pub enum CrossValidationResult {
    /// Output is consistent with all constraints.
    Pass,
    /// Output contradicts a specific constraint — include the violation detail.
    Fail {
        violation: String,
        contradicted_segment: Option<String>,
    },
}

/// Validates that Segment N's output is consistent with:
/// 1. The original high-level objective (Requirement 6.1)
/// 2. All prior segment outputs (1..N-1) (Requirement 6.5)
///
/// Uses keyword overlap heuristics for objective consistency and
/// negation/numeric contradiction detection for prior output consistency.
/// A production system would use LLM-as-judge for deeper semantic checks.
pub struct CrossSegmentValidator;

impl CrossSegmentValidator {
    /// Compare segment output against the original objective constraints and
    /// verify it does not contradict any prior segment output.
    ///
    /// Returns `CrossValidationResult::Pass` if both checks pass, or
    /// `CrossValidationResult::Fail` with the specific violation detail.
    pub fn validate(
        &self,
        segment_output: &str,
        original_objective: &str,
        prior_outputs: &[super::guardrail::SegmentOutput],
    ) -> Result<CrossValidationResult> {
        // --- Requirement 6.1: Check output against original objective constraints ---
        if let Some(violation) = self.check_objective_consistency(segment_output, original_objective) {
            return Ok(CrossValidationResult::Fail {
                violation,
                contradicted_segment: None,
            });
        }

        // --- Requirement 6.5: Check output does not contradict prior segments ---
        if let Some((violation, contradicted_id)) =
            self.check_prior_output_consistency(segment_output, prior_outputs)
        {
            return Ok(CrossValidationResult::Fail {
                violation,
                contradicted_segment: Some(contradicted_id),
            });
        }

        Ok(CrossValidationResult::Pass)
    }

    /// Check that the segment output relates to the original objective.
    /// Uses keyword overlap heuristic: extracts significant words from the objective
    /// and checks that at least some appear in the output.
    ///
    /// Returns `Some(violation_message)` if drift is detected, `None` if consistent.
    fn check_objective_consistency(
        &self,
        segment_output: &str,
        original_objective: &str,
    ) -> Option<String> {
        // Extract significant words from the objective (words > 3 chars, lowercased)
        let objective_terms: std::collections::HashSet<String> = original_objective
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
            .filter(|w| w.len() > 3)
            .collect();

        // If the objective has too few significant terms, skip the check
        if objective_terms.len() < 3 {
            return None;
        }

        let output_lower = segment_output.to_lowercase();

        // Count how many objective terms appear in the output
        let matched = objective_terms
            .iter()
            .filter(|term| output_lower.contains(term.as_str()))
            .count();

        let coverage = matched as f64 / objective_terms.len() as f64;

        // If less than 10% of objective terms appear in the output, flag as drift.
        // This mirrors the InputGuardrail's drift detection threshold.
        if coverage < 0.1 {
            Some(format!(
                "Segment output has insufficient relevance to original objective. \
                 Coverage: {:.1}% of {} key terms from objective. \
                 Objective: '{}'",
                coverage * 100.0,
                objective_terms.len(),
                if original_objective.len() > 80 {
                    format!("{}...", &original_objective[..80])
                } else {
                    original_objective.to_string()
                }
            ))
        } else {
            None
        }
    }

    /// Check that the segment output does not contradict any prior segment output.
    /// Detects:
    /// 1. Explicit negation patterns (output says "not X" when a prior said "X")
    /// 2. Conflicting numeric values for the same measurement
    ///
    /// Returns `Some((violation_message, contradicted_segment_id))` if a contradiction
    /// is found, `None` if consistent.
    fn check_prior_output_consistency(
        &self,
        segment_output: &str,
        prior_outputs: &[super::guardrail::SegmentOutput],
    ) -> Option<(String, String)> {
        let output_lower = segment_output.to_lowercase();

        for prior in prior_outputs {
            let prior_lower = prior.content.to_lowercase();

            // Check for explicit negation contradictions
            if let Some(violation) =
                self.detect_negation_contradiction(&output_lower, &prior_lower, &prior.rsu_id)
            {
                return Some(violation);
            }

            // Check for conflicting numeric values
            if let Some(violation) =
                self.detect_numeric_contradiction(&output_lower, &prior_lower, &prior.rsu_id)
            {
                return Some(violation);
            }
        }

        None
    }

    /// Detect explicit negation contradictions between the current output and a prior output.
    ///
    /// Looks for patterns where the current output negates a claim from a prior output:
    /// - Prior says "X is Y" → current says "X is not Y"
    /// - Prior says "confirmed X" → current says "no X" or "not X"
    fn detect_negation_contradiction(
        &self,
        output_lower: &str,
        prior_lower: &str,
        prior_id: &str,
    ) -> Option<(String, String)> {
        // Negation prefixes to check
        let negation_patterns = ["not ", "no ", "never ", "cannot ", "isn't ", "doesn't ", "won't "];

        // Extract key phrases from prior output (simple word pairs for context)
        let prior_words: Vec<&str> = prior_lower.split_whitespace().collect();

        for window in prior_words.windows(3) {
            let phrase = window.join(" ");

            // Skip very short or common phrases
            if phrase.len() < 8 {
                continue;
            }

            // Check if the current output negates this phrase
            for neg in &negation_patterns {
                // Look for "not <phrase_fragment>" in the output where <phrase_fragment>
                // is a significant part of the prior's claim
                let key_word = window[window.len() - 1]; // last word of the window
                if key_word.len() <= 3 {
                    continue;
                }

                let negated_pattern = format!("{}{}", neg, key_word);
                if output_lower.contains(&negated_pattern) && prior_lower.contains(key_word) {
                    // Verify the prior output affirms this (not also negating it)
                    let prior_has_negation = negation_patterns
                        .iter()
                        .any(|n| prior_lower.contains(&format!("{}{}", n, key_word)));

                    if !prior_has_negation {
                        return Some((
                            format!(
                                "Output contradicts prior segment {}: output states '{}' \
                                 but prior segment affirms '{}'",
                                prior_id,
                                negated_pattern.trim(),
                                phrase
                            ),
                            prior_id.to_string(),
                        ));
                    }
                }
            }
        }

        None
    }

    /// Detect conflicting numeric values between the current output and a prior output.
    ///
    /// Looks for patterns where both outputs mention the same measurement context
    /// but with different numeric values (e.g., "temperature is 25°C" vs "temperature is 5°C").
    fn detect_numeric_contradiction(
        &self,
        output_lower: &str,
        prior_lower: &str,
        prior_id: &str,
    ) -> Option<(String, String)> {
        // Extract numeric values with their surrounding context from both outputs
        let output_numbers = Self::extract_numbers_with_context(output_lower);
        let prior_numbers = Self::extract_numbers_with_context(prior_lower);

        // Compare: if the same context word appears with different numbers, it's a contradiction
        for (output_ctx, output_val) in &output_numbers {
            for (prior_ctx, prior_val) in &prior_numbers {
                // Context words must match (same measurement being discussed)
                if output_ctx == prior_ctx && output_ctx.len() > 3 {
                    // Values must differ significantly (not just rounding)
                    let diff = (output_val - prior_val).abs();
                    let max_val = output_val.abs().max(prior_val.abs()).max(1.0);

                    // Flag if difference is > 20% of the larger value
                    if diff / max_val > 0.2 && diff > 0.5 {
                        return Some((
                            format!(
                                "Numeric contradiction with prior segment {}: \
                                 output states {} = {} but prior segment states {} = {}",
                                prior_id, output_ctx, output_val, prior_ctx, prior_val
                            ),
                            prior_id.to_string(),
                        ));
                    }
                }
            }
        }

        None
    }

    /// Extract numbers paired with their preceding context word.
    /// Returns Vec<(context_word, numeric_value)>.
    ///
    /// For example, "temperature is 25.5" → [("temperature", 25.5)]
    fn extract_numbers_with_context(text: &str) -> Vec<(String, f64)> {
        let mut results = Vec::new();
        let words: Vec<&str> = text.split_whitespace().collect();

        for i in 0..words.len() {
            // Try to parse the current word as a number (strip common suffixes like °C, %, m)
            let cleaned = words[i]
                .trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-');

            if let Ok(num) = cleaned.parse::<f64>() {
                // Look backward for a context word (skip "is", "was", "=", etc.)
                let skip_words = ["is", "was", "are", "were", "=", "of", "at", "to", "the", "a"];
                let mut ctx_idx = i.saturating_sub(1);
                while ctx_idx > 0 && skip_words.contains(&words[ctx_idx]) {
                    ctx_idx -= 1;
                }

                if ctx_idx < i {
                    let ctx_word = words[ctx_idx]
                        .trim_matches(|c: char| !c.is_alphanumeric())
                        .to_lowercase();
                    if ctx_word.len() > 3 {
                        results.push((ctx_word, num));
                    }
                }
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_severity_serialization() {
        let blocker = RuleSeverity::Blocker;
        let warning = RuleSeverity::Warning;

        let blocker_json = serde_json::to_string(&blocker).unwrap();
        let warning_json = serde_json::to_string(&warning).unwrap();

        assert_eq!(blocker_json, "\"blocker\"");
        assert_eq!(warning_json, "\"warning\"");
    }

    #[test]
    fn test_rule_severity_deserialization() {
        let blocker: RuleSeverity = serde_json::from_str("\"blocker\"").unwrap();
        let warning: RuleSeverity = serde_json::from_str("\"warning\"").unwrap();

        assert_eq!(blocker, RuleSeverity::Blocker);
        assert_eq!(warning, RuleSeverity::Warning);
    }

    #[test]
    fn test_fidelity_check_roundtrip() {
        let check = FidelityCheck {
            context: "https://kiro.ai".to_string(),
            check_type: "FidelityCheck".to_string(),
            validation_rules: vec![
                ValidationRule {
                    rule: "strict_schema_enforcement".to_string(),
                    description: "Output must not contain fields or structures not defined in the validated schema".to_string(),
                    severity: RuleSeverity::Blocker,
                },
                ValidationRule {
                    rule: "citation_required".to_string(),
                    description: "Every code reference must include source file path and line range".to_string(),
                    severity: RuleSeverity::Warning,
                },
            ],
            monitoring: MonitoringConfig {
                threshold: 0.05,
                rolling_window: 5,
                escalation_threshold: 0.03,
            },
        };

        let json = serde_json::to_string(&check).unwrap();
        let deserialized: FidelityCheck = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.context, "https://kiro.ai");
        assert_eq!(deserialized.check_type, "FidelityCheck");
        assert_eq!(deserialized.validation_rules.len(), 2);
        assert_eq!(deserialized.validation_rules[0].severity, RuleSeverity::Blocker);
        assert_eq!(deserialized.validation_rules[1].severity, RuleSeverity::Warning);
        assert_eq!(deserialized.monitoring.threshold, 0.05);
        assert_eq!(deserialized.monitoring.rolling_window, 5);
        assert_eq!(deserialized.monitoring.escalation_threshold, 0.03);
    }

    #[test]
    fn test_fidelity_check_json_ld_fields() {
        let check = FidelityCheck {
            context: "https://kiro.ai".to_string(),
            check_type: "FidelityCheck".to_string(),
            validation_rules: vec![],
            monitoring: MonitoringConfig {
                threshold: 0.05,
                rolling_window: 5,
                escalation_threshold: 0.03,
            },
        };

        let value = serde_json::to_value(&check).unwrap();
        // Verify JSON-LD field names are correctly renamed
        assert_eq!(value["@context"], "https://kiro.ai");
        assert_eq!(value["@type"], "FidelityCheck");
    }

    #[test]
    fn test_fidelity_check_to_json_ld() {
        let check = FidelityCheck {
            context: "https://kiro.ai".to_string(),
            check_type: "FidelityCheck".to_string(),
            validation_rules: vec![
                ValidationRule {
                    rule: "strict_schema_enforcement".to_string(),
                    description: "Output must not contain undefined fields".to_string(),
                    severity: RuleSeverity::Blocker,
                },
                ValidationRule {
                    rule: "citation_required".to_string(),
                    description: "Code references must include file path".to_string(),
                    severity: RuleSeverity::Warning,
                },
            ],
            monitoring: MonitoringConfig {
                threshold: 0.05,
                rolling_window: 5,
                escalation_threshold: 0.03,
            },
        };

        let json_ld = check.to_json_ld();

        // Verify JSON-LD envelope
        assert_eq!(json_ld["@context"], "https://kiro.ai");
        assert_eq!(json_ld["@type"], "FidelityCheck");

        // Verify validation_rules are present
        let rules = json_ld["validation_rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["rule"], "strict_schema_enforcement");
        assert_eq!(rules[0]["severity"], "blocker");
        assert_eq!(rules[1]["rule"], "citation_required");
        assert_eq!(rules[1]["severity"], "warning");

        // Verify monitoring config (use approximate comparison for f32 precision)
        let threshold = json_ld["monitoring"]["threshold"].as_f64().unwrap();
        assert!((threshold - 0.05).abs() < 1e-6, "threshold was {}", threshold);
        assert_eq!(json_ld["monitoring"]["rolling_window"], 5);
        let escalation = json_ld["monitoring"]["escalation_threshold"].as_f64().unwrap();
        assert!((escalation - 0.03).abs() < 1e-6, "escalation_threshold was {}", escalation);
    }

    #[test]
    fn test_fidelity_check_to_json_ld_overrides_incorrect_fields() {
        // Even if struct fields have wrong values, to_json_ld enforces correct JSON-LD envelope
        let check = FidelityCheck {
            context: "wrong_context".to_string(),
            check_type: "WrongType".to_string(),
            validation_rules: vec![],
            monitoring: MonitoringConfig {
                threshold: 0.1,
                rolling_window: 3,
                escalation_threshold: 0.05,
            },
        };

        let json_ld = check.to_json_ld();

        // Must always be correct regardless of struct field values
        assert_eq!(json_ld["@context"], "https://kiro.ai");
        assert_eq!(json_ld["@type"], "FidelityCheck");
    }

    #[test]
    fn test_cross_validation_result_variants() {
        let pass = CrossValidationResult::Pass;
        assert!(matches!(pass, CrossValidationResult::Pass));

        let fail = CrossValidationResult::Fail {
            violation: "Output claims temperature is 5°C but segment_001 measured 25°C".to_string(),
            contradicted_segment: Some("segment_001".to_string()),
        };
        match fail {
            CrossValidationResult::Fail { violation, contradicted_segment } => {
                assert!(violation.contains("temperature"));
                assert_eq!(contradicted_segment, Some("segment_001".to_string()));
            }
            _ => panic!("Expected Fail variant"),
        }
    }

    // ─── from_json_ld tests ─────────────────────────────────────────────────

    fn valid_fidelity_check_json() -> serde_json::Value {
        serde_json::json!({
            "@context": "https://kiro.ai",
            "@type": "FidelityCheck",
            "validation_rules": [
                {
                    "rule": "strict_schema_enforcement",
                    "description": "Output must not contain undefined fields",
                    "severity": "blocker"
                },
                {
                    "rule": "citation_required",
                    "description": "Code references must include file path",
                    "severity": "warning"
                }
            ],
            "monitoring": {
                "threshold": 0.05,
                "rolling_window": 5,
                "escalation_threshold": 0.03
            }
        })
    }

    #[test]
    fn test_from_json_ld_valid_payload() {
        let json = valid_fidelity_check_json();
        let check = FidelityCheck::from_json_ld(&json).unwrap();

        assert_eq!(check.context, "https://kiro.ai");
        assert_eq!(check.check_type, "FidelityCheck");
        assert_eq!(check.validation_rules.len(), 2);
        assert_eq!(check.validation_rules[0].rule, "strict_schema_enforcement");
        assert_eq!(check.validation_rules[0].severity, RuleSeverity::Blocker);
        assert_eq!(check.validation_rules[1].rule, "citation_required");
        assert_eq!(check.validation_rules[1].severity, RuleSeverity::Warning);
        assert_eq!(check.monitoring.threshold, 0.05);
        assert_eq!(check.monitoring.rolling_window, 5);
    }

    #[test]
    fn test_from_json_ld_rejects_invalid_severity() {
        let json = serde_json::json!({
            "@context": "https://kiro.ai",
            "@type": "FidelityCheck",
            "validation_rules": [
                {
                    "rule": "some_rule",
                    "description": "Some description",
                    "severity": "critical"
                }
            ],
            "monitoring": {
                "threshold": 0.05,
                "rolling_window": 5,
                "escalation_threshold": 0.03
            }
        });

        let err = FidelityCheck::from_json_ld(&json).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("invalid severity 'critical'"), "Got: {}", msg);
        assert!(msg.contains("blocker") || msg.contains("warning"), "Got: {}", msg);
    }

    #[test]
    fn test_from_json_ld_rejects_threshold_above_one() {
        let json = serde_json::json!({
            "@context": "https://kiro.ai",
            "@type": "FidelityCheck",
            "validation_rules": [],
            "monitoring": {
                "threshold": 1.5,
                "rolling_window": 5,
                "escalation_threshold": 0.03
            }
        });

        let err = FidelityCheck::from_json_ld(&json).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("between 0.0 and 1.0"), "Got: {}", msg);
    }

    #[test]
    fn test_from_json_ld_rejects_threshold_below_zero() {
        let json = serde_json::json!({
            "@context": "https://kiro.ai",
            "@type": "FidelityCheck",
            "validation_rules": [],
            "monitoring": {
                "threshold": -0.1,
                "rolling_window": 5,
                "escalation_threshold": 0.03
            }
        });

        let err = FidelityCheck::from_json_ld(&json).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("between 0.0 and 1.0"), "Got: {}", msg);
    }

    #[test]
    fn test_from_json_ld_accepts_threshold_boundary_values() {
        // threshold = 0.0 should be valid
        let json_zero = serde_json::json!({
            "@context": "https://kiro.ai",
            "@type": "FidelityCheck",
            "validation_rules": [],
            "monitoring": {
                "threshold": 0.0,
                "rolling_window": 5,
                "escalation_threshold": 0.03
            }
        });
        assert!(FidelityCheck::from_json_ld(&json_zero).is_ok());

        // threshold = 1.0 should be valid
        let json_one = serde_json::json!({
            "@context": "https://kiro.ai",
            "@type": "FidelityCheck",
            "validation_rules": [],
            "monitoring": {
                "threshold": 1.0,
                "rolling_window": 5,
                "escalation_threshold": 0.03
            }
        });
        assert!(FidelityCheck::from_json_ld(&json_one).is_ok());
    }

    #[test]
    fn test_from_json_ld_rejects_missing_rule_field() {
        let json = serde_json::json!({
            "@context": "https://kiro.ai",
            "@type": "FidelityCheck",
            "validation_rules": [
                {
                    "description": "Some description",
                    "severity": "blocker"
                }
            ],
            "monitoring": {
                "threshold": 0.05,
                "rolling_window": 5,
                "escalation_threshold": 0.03
            }
        });

        let err = FidelityCheck::from_json_ld(&json).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing required field 'rule'"), "Got: {}", msg);
        assert!(msg.contains("validation_rules[0]"), "Got: {}", msg);
    }

    #[test]
    fn test_from_json_ld_rejects_missing_description_field() {
        let json = serde_json::json!({
            "@context": "https://kiro.ai",
            "@type": "FidelityCheck",
            "validation_rules": [
                {
                    "rule": "some_rule",
                    "severity": "warning"
                }
            ],
            "monitoring": {
                "threshold": 0.05,
                "rolling_window": 5,
                "escalation_threshold": 0.03
            }
        });

        let err = FidelityCheck::from_json_ld(&json).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing required field 'description'"), "Got: {}", msg);
    }

    #[test]
    fn test_from_json_ld_rejects_missing_severity_field() {
        let json = serde_json::json!({
            "@context": "https://kiro.ai",
            "@type": "FidelityCheck",
            "validation_rules": [
                {
                    "rule": "some_rule",
                    "description": "Some description"
                }
            ],
            "monitoring": {
                "threshold": 0.05,
                "rolling_window": 5,
                "escalation_threshold": 0.03
            }
        });

        let err = FidelityCheck::from_json_ld(&json).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing required field 'severity'"), "Got: {}", msg);
    }

    #[test]
    fn test_from_json_ld_rejects_non_object_payload() {
        let json = serde_json::json!("not an object");
        let err = FidelityCheck::from_json_ld(&json).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("must be a JSON object"), "Got: {}", msg);
    }

    #[test]
    fn test_from_json_ld_rejects_missing_validation_rules() {
        let json = serde_json::json!({
            "@context": "https://kiro.ai",
            "@type": "FidelityCheck",
            "monitoring": {
                "threshold": 0.05,
                "rolling_window": 5,
                "escalation_threshold": 0.03
            }
        });

        let err = FidelityCheck::from_json_ld(&json).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("validation_rules"), "Got: {}", msg);
    }

    #[test]
    fn test_from_json_ld_rejects_missing_monitoring() {
        let json = serde_json::json!({
            "@context": "https://kiro.ai",
            "@type": "FidelityCheck",
            "validation_rules": []
        });

        let err = FidelityCheck::from_json_ld(&json).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("monitoring"), "Got: {}", msg);
    }
}
