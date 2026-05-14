//! RSU (Region SPEC Unit) — the atomic unit of segmented execution.
//!
//! Each RSU represents one decomposed sub-task with:
//! - Metadata (parent goal, budget, mode)
//! - Phases (observation → reasoning → accuracy_check)
//! - Output (filled after execution)
//!
//! This module contains both the original internal types (`RsuTask`, `RsuMetadata`)
//! and the full JSON-LD compliant types (`Rsu`, `RsuMeta`) from the design spec.

use super::drift::SteeringMode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How much reasoning effort to apply
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingBudget {
    /// Lookups, simple facts — 512 tokens max
    Low,
    /// Logic, moderate reasoning — 2048 tokens max
    Medium,
    /// Architecture decisions, complex synthesis — 4096 tokens max
    High,
}

impl ThinkingBudget {
    /// Max tokens for the reasoning phase
    pub fn max_tokens(&self) -> u32 {
        match self {
            Self::Low => 512,
            Self::Medium => 2048,
            Self::High => 4096,
        }
    }

    /// Total token budget across all phases
    pub fn total_budget(&self) -> u32 {
        match self {
            Self::Low => 1024,
            Self::Medium => 4096,
            Self::High => 8192,
        }
    }

    /// Alias for max_tokens — reasoning phase token limit (design spec naming)
    pub fn reasoning_tokens(&self) -> u32 {
        self.max_tokens()
    }

    /// Alias for total_budget — total token limit across all phases (design spec naming)
    pub fn total_tokens(&self) -> u32 {
        self.total_budget()
    }

    /// Default context TTL in seconds per budget tier.
    /// Aggressive pruning for low-VRAM hardware (Pascal GPUs).
    pub fn default_context_ttl_seconds(&self) -> u32 {
        match self {
            Self::Low => 15,    // Simple lookups — evict fast
            Self::Medium => 30, // Standard reasoning — moderate retention
            Self::High => 60,   // Architecture decisions — keep longer
        }
    }
}

/// RSU metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsuMetadata {
    /// The original high-level objective this RSU serves
    pub parent_goal: String,
    /// How much reasoning effort to apply
    pub thinking_budget: ThinkingBudget,
    /// Precision vs Exploratory mode
    pub steering_mode: SteeringMode,
    /// How many times this RSU has been retried
    pub retries: u32,
}

/// The three phases of RSU execution (strict order)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsuPhases {
    /// What context to gather (from prior segments + nautivecs)
    pub observation: String,
    /// The actual reasoning task
    pub reasoning: String,
    /// The constraint to verify against
    pub accuracy_check: String,
}

/// A complete RSU task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsuTask {
    /// Unique identifier: segment_001, segment_002, etc.
    pub id: String,
    /// Metadata about this segment
    pub metadata: RsuMetadata,
    /// The three execution phases
    pub phases: RsuPhases,
    /// Output (filled after successful execution)
    pub output: Option<String>,
}

/// Steering policy embedded in an RSU (from Gemini's schema addition)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteeringPolicy {
    pub mode: SteeringMode,
    pub drift_threshold: f64,
    pub hardware_profile: String,
    pub context_window_limit: usize,
}

impl Default for SteeringPolicy {
    fn default() -> Self {
        Self {
            mode: SteeringMode::Precision,
            drift_threshold: 0.05,
            hardware_profile: "low-vram-8gb".to_string(),
            context_window_limit: 4096,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Full JSON-LD compliant types (design spec)
// ─────────────────────────────────────────────────────────────────────────────

/// The segment execution mode — controls drift threshold selection.
/// Maps to the existing `SteeringMode` for backward compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentMode {
    /// Logic, math, parameter tuning — strict 0.05 threshold
    Precision,
    /// Research, synthesis, architecture — relaxed 0.15 threshold
    Exploratory,
}

/// Backward compatibility: convert the new SegmentMode into the existing SteeringMode.
impl From<SegmentMode> for SteeringMode {
    fn from(mode: SegmentMode) -> Self {
        match mode {
            SegmentMode::Precision => SteeringMode::Precision,
            SegmentMode::Exploratory => SteeringMode::Exploratory,
        }
    }
}

/// Steering policy for the JSON-LD RSU schema.
/// Controls which drift monitor is selected and its threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsuSteeringPolicy {
    pub mode: SegmentMode,
    pub drift_threshold: f32,
}

impl RsuSteeringPolicy {
    pub fn precision() -> Self {
        Self { mode: SegmentMode::Precision, drift_threshold: 0.05 }
    }

    pub fn exploratory() -> Self {
        Self { mode: SegmentMode::Exploratory, drift_threshold: 0.15 }
    }
}

/// RSU metadata block (JSON-LD schema version).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsuMeta {
    pub parent_goal: String,
    pub thinking_budget: ThinkingBudget,
    /// Path to the steering file for fidelity checks
    pub steering_ref: String,
    /// Steering policy controlling drift threshold selection
    pub steering_policy: RsuSteeringPolicy,
    /// Context TTL in seconds — aggressive pruning for low-VRAM hardware.
    /// After this duration, the segment's context is eligible for eviction.
    pub context_ttl_seconds: u32,
    /// When this RSU was created
    pub created_at: DateTime<Utc>,
}

/// Execution action specification for the JSON-LD RSU.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsuExecution {
    pub action: String,
    pub parameters: serde_json::Value,
}

/// The complete RSU (Region SPEC Unit) — JSON-LD compliant.
///
/// This is the full design-spec type with `@context` and `@type` fields.
/// For the simpler internal representation, see `RsuTask`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rsu {
    #[serde(rename = "@context")]
    pub context: String, // "https://kiro.ai"
    #[serde(rename = "@type")]
    pub rsu_type: String, // "SegmentTask"
    pub id: String,
    pub meta: RsuMeta,
    pub phases: RsuPhases,
    pub execution: RsuExecution,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// Known top-level fields in the RSU JSON-LD schema.
/// Any field not in this set is rejected as unexpected.
const KNOWN_RSU_FIELDS: &[&str] = &[
    "@context",
    "@type",
    "id",
    "meta",
    "phases",
    "execution",
    "dependencies",
];

impl Rsu {
    /// Parse and validate an RSU from a JSON-LD `serde_json::Value`.
    ///
    /// Validates:
    /// - No unexpected top-level fields (Requirement 1.3)
    /// - Required fields present: id, meta.parent_goal, phases.observation,
    ///   phases.reasoning, phases.accuracy_check (Requirement 1.6)
    /// - `thinking_budget` is one of low/medium/high (Requirement 1.5)
    ///
    /// Returns descriptive errors identifying the specific validation failure.
    pub fn from_json_ld(value: &serde_json::Value) -> anyhow::Result<Rsu> {
        use anyhow::{bail, Context};

        let obj = value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("RSU payload must be a JSON object"))?;

        // --- Reject unexpected top-level fields (Requirement 1.3) ---
        let unexpected: Vec<&String> = obj
            .keys()
            .filter(|k| !KNOWN_RSU_FIELDS.contains(&k.as_str()))
            .collect();

        if !unexpected.is_empty() {
            bail!(
                "RSU payload contains unexpected fields: {}",
                unexpected
                    .iter()
                    .map(|k| format!("\"{}\"", k))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        // --- Validate required fields are present (Requirement 1.6) ---
        if !obj.contains_key("id") {
            bail!("RSU payload is missing required field: \"id\"");
        }
        if obj.get("id").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
            bail!("RSU payload field \"id\" must be a non-empty string");
        }

        // Validate meta and meta.parent_goal
        let meta = obj
            .get("meta")
            .and_then(|v| v.as_object())
            .ok_or_else(|| anyhow::anyhow!("RSU payload is missing required field: \"meta\""))?;

        if !meta.contains_key("parent_goal") {
            bail!("RSU payload is missing required field: \"meta.parent_goal\"");
        }
        if meta
            .get("parent_goal")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            bail!("RSU payload field \"meta.parent_goal\" must be a non-empty string");
        }

        // Validate thinking_budget (Requirement 1.5)
        if let Some(budget_val) = meta.get("thinking_budget") {
            let budget_str = budget_val.as_str().ok_or_else(|| {
                anyhow::anyhow!(
                    "RSU payload field \"meta.thinking_budget\" must be a string (one of: low, medium, high)"
                )
            })?;
            match budget_str {
                "low" | "medium" | "high" => {} // valid
                other => bail!(
                    "RSU payload field \"meta.thinking_budget\" has invalid value \"{}\": must be one of low, medium, high",
                    other
                ),
            }
        }

        // Validate phases and required phase fields
        let phases = obj
            .get("phases")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                anyhow::anyhow!("RSU payload is missing required field: \"phases\"")
            })?;

        if !phases.contains_key("observation") {
            bail!("RSU payload is missing required field: \"phases.observation\"");
        }
        if phases
            .get("observation")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            bail!("RSU payload field \"phases.observation\" must be a non-empty string");
        }

        if !phases.contains_key("reasoning") {
            bail!("RSU payload is missing required field: \"phases.reasoning\"");
        }
        if phases
            .get("reasoning")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            bail!("RSU payload field \"phases.reasoning\" must be a non-empty string");
        }

        if !phases.contains_key("accuracy_check") {
            bail!("RSU payload is missing required field: \"phases.accuracy_check\"");
        }
        if phases
            .get("accuracy_check")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            bail!("RSU payload field \"phases.accuracy_check\" must be a non-empty string");
        }

        // --- Deserialize into the Rsu struct ---
        let rsu: Rsu = serde_json::from_value(value.clone())
            .context("Failed to deserialize RSU from JSON-LD payload")?;

        Ok(rsu)
    }

    /// Serialize this RSU to a JSON-LD conformant `serde_json::Value`.
    ///
    /// Guarantees that `@context` is set to `"https://kiro.ai"` and
    /// `@type` is set to `"SegmentTask"` in the output, regardless of
    /// what the struct fields currently hold.
    pub fn to_json_ld(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self)
            .expect("RSU serialization should never fail for a valid struct");

        // Enforce JSON-LD conformance — override context and type
        if let Some(obj) = value.as_object_mut() {
            obj.insert(
                "@context".to_string(),
                serde_json::Value::String("https://kiro.ai".to_string()),
            );
            obj.insert(
                "@type".to_string(),
                serde_json::Value::String("SegmentTask".to_string()),
            );
        }

        value
    }
}

/// Permitted execution actions (whitelist).
pub const PERMITTED_ACTIONS: &[&str] = &[
    "writeFile",
    "readFile",
    "runCommand",
    "queryNautivecs",
];


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper: build a valid RSU JSON-LD payload for testing.
    fn valid_rsu_json() -> serde_json::Value {
        json!({
            "@context": "https://kiro.ai",
            "@type": "SegmentTask",
            "id": "segment_001",
            "meta": {
                "parent_goal": "Analyze thermal anomaly in tile B02",
                "thinking_budget": "medium",
                "steering_ref": "steering/accuracy-guardrail.md",
                "steering_policy": {
                    "mode": "precision",
                    "drift_threshold": 0.05
                },
                "context_ttl_seconds": 30,
                "created_at": "2024-07-15T10:30:00Z"
            },
            "phases": {
                "observation": "Load prior segment outputs for tile B02.",
                "reasoning": "Compare thermal delta against known wreck signatures.",
                "accuracy_check": "Verify anomaly coordinates fall within tile bounds."
            },
            "execution": {
                "action": "queryNautivecs",
                "parameters": { "query": "thermal anomaly", "top_k": 5 }
            },
            "dependencies": ["segment_000"]
        })
    }

    #[test]
    fn test_from_json_ld_valid_payload() {
        let value = valid_rsu_json();
        let rsu = Rsu::from_json_ld(&value).expect("valid payload should parse");
        assert_eq!(rsu.id, "segment_001");
        assert_eq!(rsu.meta.parent_goal, "Analyze thermal anomaly in tile B02");
        assert_eq!(rsu.meta.thinking_budget, ThinkingBudget::Medium);
        assert_eq!(rsu.phases.observation, "Load prior segment outputs for tile B02.");
        assert_eq!(rsu.dependencies, vec!["segment_000"]);
    }

    #[test]
    fn test_from_json_ld_rejects_unexpected_fields() {
        let mut value = valid_rsu_json();
        value.as_object_mut().unwrap().insert(
            "malicious_field".to_string(),
            json!("injected"),
        );
        let err = Rsu::from_json_ld(&value).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unexpected fields"), "got: {}", msg);
        assert!(msg.contains("malicious_field"), "got: {}", msg);
    }

    #[test]
    fn test_from_json_ld_rejects_missing_id() {
        let mut value = valid_rsu_json();
        value.as_object_mut().unwrap().remove("id");
        let err = Rsu::from_json_ld(&value).unwrap_err();
        assert!(err.to_string().contains("\"id\""), "got: {}", err);
    }

    #[test]
    fn test_from_json_ld_rejects_empty_id() {
        let mut value = valid_rsu_json();
        value.as_object_mut().unwrap().insert("id".to_string(), json!(""));
        let err = Rsu::from_json_ld(&value).unwrap_err();
        assert!(err.to_string().contains("\"id\""), "got: {}", err);
    }

    #[test]
    fn test_from_json_ld_rejects_missing_parent_goal() {
        let mut value = valid_rsu_json();
        value["meta"].as_object_mut().unwrap().remove("parent_goal");
        let err = Rsu::from_json_ld(&value).unwrap_err();
        assert!(err.to_string().contains("meta.parent_goal"), "got: {}", err);
    }

    #[test]
    fn test_from_json_ld_rejects_missing_phases_observation() {
        let mut value = valid_rsu_json();
        value["phases"].as_object_mut().unwrap().remove("observation");
        let err = Rsu::from_json_ld(&value).unwrap_err();
        assert!(err.to_string().contains("phases.observation"), "got: {}", err);
    }

    #[test]
    fn test_from_json_ld_rejects_missing_phases_reasoning() {
        let mut value = valid_rsu_json();
        value["phases"].as_object_mut().unwrap().remove("reasoning");
        let err = Rsu::from_json_ld(&value).unwrap_err();
        assert!(err.to_string().contains("phases.reasoning"), "got: {}", err);
    }

    #[test]
    fn test_from_json_ld_rejects_missing_phases_accuracy_check() {
        let mut value = valid_rsu_json();
        value["phases"].as_object_mut().unwrap().remove("accuracy_check");
        let err = Rsu::from_json_ld(&value).unwrap_err();
        assert!(err.to_string().contains("phases.accuracy_check"), "got: {}", err);
    }

    #[test]
    fn test_from_json_ld_rejects_invalid_thinking_budget() {
        let mut value = valid_rsu_json();
        value["meta"].as_object_mut().unwrap().insert(
            "thinking_budget".to_string(),
            json!("extreme"),
        );
        let err = Rsu::from_json_ld(&value).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("thinking_budget"), "got: {}", msg);
        assert!(msg.contains("extreme"), "got: {}", msg);
    }

    #[test]
    fn test_from_json_ld_accepts_all_valid_budgets() {
        for budget in &["low", "medium", "high"] {
            let mut value = valid_rsu_json();
            value["meta"].as_object_mut().unwrap().insert(
                "thinking_budget".to_string(),
                json!(budget),
            );
            assert!(
                Rsu::from_json_ld(&value).is_ok(),
                "budget '{}' should be accepted",
                budget
            );
        }
    }

    #[test]
    fn test_from_json_ld_rejects_non_object_payload() {
        let value = json!("not an object");
        let err = Rsu::from_json_ld(&value).unwrap_err();
        assert!(err.to_string().contains("JSON object"), "got: {}", err);
    }

    #[test]
    fn test_from_json_ld_rejects_missing_meta() {
        let mut value = valid_rsu_json();
        value.as_object_mut().unwrap().remove("meta");
        let err = Rsu::from_json_ld(&value).unwrap_err();
        assert!(err.to_string().contains("\"meta\""), "got: {}", err);
    }

    #[test]
    fn test_from_json_ld_rejects_missing_phases() {
        let mut value = valid_rsu_json();
        value.as_object_mut().unwrap().remove("phases");
        let err = Rsu::from_json_ld(&value).unwrap_err();
        assert!(err.to_string().contains("\"phases\""), "got: {}", err);
    }
}
