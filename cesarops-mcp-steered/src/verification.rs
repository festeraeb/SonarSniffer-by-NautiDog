//! Verification & Self-RAG — detects [MISSING_CONTEXT] signals and routes
//! responses through the Speculative RAG pipeline (1.5B → 7B → Human).
//!
//! Three layers of quality control:
//! 1. Self-RAG: LLM outputs [MISSING_CONTEXT: ...] when it knows it's guessing
//! 2. Confidence Gate: vector match score determines routing
//! 3. Speculative RAG: borderline responses get verified by larger model

use serde::Serialize;

// ── Missing Context Detection (Self-RAG) ─────────────────────────────────────

/// Action to take when [MISSING_CONTEXT] is detected in LLM output
#[derive(Debug, Clone, Serialize)]
pub enum MissingContextAction {
    /// Log to n8n for manual indexing — human needs to point us at the right code
    N8nLog(String),
    /// Trigger a secondary nautivecs search with the missing term and retry
    AutoRetry(String),
}

/// Scan LLM response for [MISSING_CONTEXT: ...] self-awareness signals.
/// If found, determine whether to auto-retry (for types/structs) or ask human.
pub fn detect_missing_context(response: &str) -> Option<MissingContextAction> {
    // Look for the pattern: [MISSING_CONTEXT: some description]
    let start_marker = "[MISSING_CONTEXT:";
    let start_idx = response.find(start_marker)?;
    let after_marker = &response[start_idx + start_marker.len()..];
    let end_idx = after_marker.find(']')?;
    let missing_term = after_marker[..end_idx].trim().to_string();

    if missing_term.is_empty() {
        return None;
    }

    // Strategy: If it looks like a type/struct name (contains uppercase),
    // we can auto-retry by searching nautivecs for that symbol.
    // Otherwise, it's a conceptual gap — send to human.
    if missing_term.chars().any(|c| c.is_uppercase()) && missing_term.len() < 60 {
        Some(MissingContextAction::AutoRetry(missing_term))
    } else {
        Some(MissingContextAction::N8nLog(missing_term))
    }
}

// ── Speculative RAG Routing ──────────────────────────────────────────────────

/// What to do with a response based on confidence and context
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum VerifyAction {
    /// Response is well-grounded — deliver to user as-is
    Accept,
    /// Borderline confidence — send to 7B model for verification
    SendTo7B,
    /// Low confidence or critical operation — require human approval
    AskHuman,
}

/// Determine the verification path for a response.
///
/// Inputs:
/// - `confidence`: nautivecs RRF match score (0.0 - 1.0)
/// - `tool_name`: which MCP tool generated this response
/// - `missing_context_detected`: whether [MISSING_CONTEXT] was found in output
///
/// The routing logic prioritizes safety:
/// 1. If the model ITSELF says it's missing context → always ask human
/// 2. If it's a critical operation (tune_parameters) → higher bar
/// 3. General queries → standard confidence thresholds
pub fn determine_verification_path(
    confidence: f32,
    tool_name: &str,
    missing_context_detected: bool,
) -> VerifyAction {
    // Priority 1: Model admitted it's missing something — never trust a guess
    if missing_context_detected {
        return VerifyAction::AskHuman;
    }

    // Priority 2: Critical system changes (parameter tuning affects GPU compute)
    if tool_name == "tune_parameters" || tool_name == "execute_shader" {
        return if confidence > 0.9 {
            VerifyAction::SendTo7B // 7B double-checks the math even at high confidence
        } else {
            VerifyAction::AskHuman // Human must approve knob turns below 0.9
        };
    }

    // Priority 3: General queries — standard confidence routing
    match confidence {
        x if x >= 0.8 => VerifyAction::Accept,   // Solid grounding, deliver directly
        x if x >= 0.6 => VerifyAction::SendTo7B,  // "I think I know, but verify"
        _ => VerifyAction::AskHuman,               // "I'm lost, trigger n8n"
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_missing_context_struct() {
        let response = "The function uses [MISSING_CONTEXT: P100Node struct definition] for GPU management.";
        let action = detect_missing_context(response);
        assert!(matches!(action, Some(MissingContextAction::AutoRetry(ref s)) if s.contains("P100Node")));
    }

    #[test]
    fn test_detect_missing_context_concept() {
        let response = "I need [MISSING_CONTEXT: how the weather tagging system classifies post-storm tiles] to answer this.";
        let action = detect_missing_context(response);
        assert!(matches!(action, Some(MissingContextAction::N8nLog(_))));
    }

    #[test]
    fn test_detect_no_missing_context() {
        let response = "The glint_score threshold is 0.5 based on the tpu_client.py implementation.";
        assert!(detect_missing_context(response).is_none());
    }

    #[test]
    fn test_routing_missing_context_always_human() {
        assert_eq!(
            determine_verification_path(0.95, "steered_query", true),
            VerifyAction::AskHuman
        );
    }

    #[test]
    fn test_routing_tune_parameters_high_confidence() {
        assert_eq!(
            determine_verification_path(0.95, "tune_parameters", false),
            VerifyAction::SendTo7B
        );
    }

    #[test]
    fn test_routing_tune_parameters_low_confidence() {
        assert_eq!(
            determine_verification_path(0.7, "tune_parameters", false),
            VerifyAction::AskHuman
        );
    }

    #[test]
    fn test_routing_general_high() {
        assert_eq!(
            determine_verification_path(0.85, "steered_query", false),
            VerifyAction::Accept
        );
    }

    #[test]
    fn test_routing_general_borderline() {
        assert_eq!(
            determine_verification_path(0.65, "analyze_code", false),
            VerifyAction::SendTo7B
        );
    }

    #[test]
    fn test_routing_general_low() {
        assert_eq!(
            determine_verification_path(0.4, "steered_query", false),
            VerifyAction::AskHuman
        );
    }
}
