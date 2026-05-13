//! Segmenter — decomposes high-level objectives into ordered RSU sequences.
//!
//! The Segmenter calls the LLM to break a complex objective into 2–10 atomic
//! RSUs (Region SPEC Units), each with unique IDs, parent_goal propagation,
//! thinking_budget assignment, and observation dependencies from prior segments.
//!
//! Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::Deserialize;

use crate::llm_client::{ChatMessage, LlmClient};
use crate::steering::SteeringEngine;
use super::rsu::{
    Rsu, RsuExecution, RsuMeta, RsuPhases, RsuSteeringPolicy, SegmentMode, ThinkingBudget,
};

/// The Segmenter decomposes high-level objectives into ordered RSU sequences
/// by calling the LLM for planning and then validating/normalizing the output.
pub struct Segmenter;

/// Intermediate representation of an RSU as returned by the LLM.
/// The LLM produces a simplified JSON array; we parse it into this struct
/// and then build full `Rsu` instances with proper IDs, metadata, etc.
#[derive(Debug, Clone, Deserialize)]
struct LlmSegment {
    /// Short description of what this segment does
    description: String,
    /// The observation/context gathering instruction
    observation: String,
    /// The reasoning task
    reasoning: String,
    /// The accuracy check constraint
    accuracy_check: String,
    /// Estimated complexity: "low", "medium", or "high"
    complexity: String,
    /// The execution action (one of: writeFile, readFile, runCommand, queryNautivecs)
    #[serde(default = "default_action")]
    action: String,
}

fn default_action() -> String {
    "queryNautivecs".to_string()
}

/// System prompt instructing the LLM to decompose an objective into segments.
const DECOMPOSITION_SYSTEM_PROMPT: &str = r#"You are a task decomposition engine. Given a high-level objective, break it into 2-10 atomic sub-tasks (segments).

Return ONLY a JSON array of objects. Each object must have these fields:
- "description": short summary of the segment's purpose
- "observation": what context to gather — MUST reference key terms from the objective (e.g., module names, file paths, technical concepts mentioned in the objective). Be specific and detailed, not generic.
- "reasoning": the actual reasoning/work to perform
- "accuracy_check": the constraint to verify the reasoning output against — reference specific terms from the objective
- "complexity": one of "low", "medium", or "high"
- "action": one of "writeFile", "readFile", "runCommand", "queryNautivecs"

Rules:
- Produce between 2 and 10 segments (inclusive)
- Order them logically — later segments may depend on earlier ones
- Simple lookups = "low" complexity, logic/analysis = "medium", architecture/synthesis = "high"
- Each segment must be atomic — one clear action
- The observation field of segment N should reference what it needs from segments before it
- CRITICAL: Each observation MUST include at least 3 key terms from the original objective. Generic observations like "gather context" will be rejected by the guardrail.

Return ONLY the JSON array, no markdown fences, no explanation."#;

impl Segmenter {
    /// Decompose a high-level objective into an ordered sequence of RSUs.
    ///
    /// Calls nautivecs for grounding (concrete technical nouns), then calls
    /// the LLM to plan the decomposition with those nouns injected.
    ///
    /// The grounding step ensures accuracy_check constraints reference real
    /// file names, struct names, and function signatures — not abstract goals.
    ///
    /// Validates and normalizes the output into proper RSU structs with:
    /// - Unique IDs in `segment_NNN` format (zero-padded, Requirement 2.2)
    /// - `parent_goal` set to the original objective (Requirement 2.3)
    /// - `thinking_budget` based on estimated complexity (Requirement 2.4)
    /// - `phases.observation` populated with prior segment dependencies (Requirement 2.5)
    /// - No duplicate IDs (Requirement 2.6)
    /// - 2–10 RSUs in the sequence (Requirement 2.1)
    pub async fn decompose(
        objective: &str,
        llm: &LlmClient,
        steering: &mut SteeringEngine,
    ) -> Result<Vec<Rsu>> {
        // ── Grounding Search: query nautivecs for concrete technical nouns ───
        let grounding_context = Self::grounding_search(objective, steering).await;

        // Build the user prompt with grounding context injected
        let user_prompt = if grounding_context.is_empty() {
            format!("Decompose this objective into atomic segments:\n\n{}", objective)
        } else {
            format!(
                "Decompose this objective into atomic segments:\n\n{}\n\n\
                 GROUNDING CONTEXT (use these exact technical terms in your accuracy_check constraints):\n{}",
                objective, grounding_context
            )
        };

        // Call the LLM to get the decomposition plan
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: DECOMPOSITION_SYSTEM_PROMPT.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt,
            },
        ];

        let raw_response = llm
            .chat_completion(messages)
            .await
            .context("Segmenter: LLM call failed during objective decomposition")?;

        // Parse the LLM response into segments
        let segments = Self::parse_llm_response(&raw_response)
            .context("Segmenter: failed to parse LLM decomposition response")?;

        // Validate segment count (Requirement 2.1)
        if segments.len() < 2 {
            bail!(
                "Segmenter: LLM produced {} segments, minimum is 2",
                segments.len()
            );
        }
        if segments.len() > 10 {
            bail!(
                "Segmenter: LLM produced {} segments, maximum is 10",
                segments.len()
            );
        }

        // Build full RSU structs from the LLM segments
        let rsus = Self::build_rsus(objective, &segments);

        // Final validation: no duplicate IDs (Requirement 2.6)
        Self::validate_unique_ids(&rsus)?;

        Ok(rsus)
    }

    /// Parse the raw LLM response text into a vector of `LlmSegment`.
    /// Handles potential markdown fences or whitespace around the JSON.
    fn parse_llm_response(response: &str) -> Result<Vec<LlmSegment>> {
        // Strip markdown code fences if present
        let trimmed = response.trim();
        let json_str = if trimmed.starts_with("```") {
            // Remove opening fence (possibly with language tag)
            let after_open = trimmed
                .find('\n')
                .map(|i| &trimmed[i + 1..])
                .unwrap_or(trimmed);
            // Remove closing fence
            after_open
                .rfind("```")
                .map(|i| &after_open[..i])
                .unwrap_or(after_open)
                .trim()
        } else {
            trimmed
        };

        // Find the JSON array boundaries in case there's extra text
        let start = json_str.find('[').ok_or_else(|| {
            anyhow::anyhow!("Segmenter: LLM response does not contain a JSON array")
        })?;
        let end = json_str.rfind(']').ok_or_else(|| {
            anyhow::anyhow!("Segmenter: LLM response does not contain a closing bracket")
        })?;

        let array_str = &json_str[start..=end];

        let segments: Vec<LlmSegment> = serde_json::from_str(array_str)
            .context("Segmenter: failed to deserialize LLM response as JSON array of segments")?;

        Ok(segments)
    }

    /// Build full RSU structs from parsed LLM segments.
    ///
    /// Assigns:
    /// - Unique IDs: segment_001, segment_002, ... (Requirement 2.2)
    /// - parent_goal: the original objective (Requirement 2.3)
    /// - thinking_budget: based on complexity field (Requirement 2.4)
    /// - observation: enriched with prior segment dependencies (Requirement 2.5)
    fn build_rsus(objective: &str, segments: &[LlmSegment]) -> Vec<Rsu> {
        let now = Utc::now();

        segments
            .iter()
            .enumerate()
            .map(|(idx, seg)| {
                let id = format!("segment_{:03}", idx + 1);
                let thinking_budget = Self::complexity_to_budget(&seg.complexity);

                // Build observation with prior segment dependencies (Requirement 2.5)
                let observation = if idx == 0 {
                    seg.observation.clone()
                } else {
                    let prior_refs: Vec<String> = (1..=idx)
                        .map(|i| format!("segment_{:03}", i))
                        .collect();
                    format!(
                        "{}. Context dependencies from prior segments: [{}]",
                        seg.observation,
                        prior_refs.join(", ")
                    )
                };

                // Build dependencies list
                let dependencies = if idx == 0 {
                    vec![]
                } else {
                    vec![format!("segment_{:03}", idx)]
                };

                Rsu {
                    context: "https://kiro.ai".to_string(),
                    rsu_type: "SegmentTask".to_string(),
                    id,
                    meta: RsuMeta {
                        parent_goal: objective.to_string(),
                        thinking_budget: thinking_budget.clone(),
                        steering_ref: "steering/accuracy-guardrail.md".to_string(),
                        steering_policy: RsuSteeringPolicy {
                            mode: Self::budget_to_mode(&thinking_budget),
                            drift_threshold: Self::budget_to_drift_threshold(&thinking_budget),
                        },
                        context_ttl_seconds: thinking_budget.default_context_ttl_seconds(),
                        created_at: now,
                    },
                    phases: RsuPhases {
                        observation,
                        reasoning: seg.reasoning.clone(),
                        accuracy_check: seg.accuracy_check.clone(),
                    },
                    execution: RsuExecution {
                        action: Self::normalize_action(&seg.action),
                        parameters: serde_json::json!({}),
                    },
                    dependencies,
                }
            })
            .collect()
    }

    /// Map complexity string to ThinkingBudget (Requirement 2.4).
    fn complexity_to_budget(complexity: &str) -> ThinkingBudget {
        match complexity.to_lowercase().as_str() {
            "low" => ThinkingBudget::Low,
            "high" => ThinkingBudget::High,
            _ => ThinkingBudget::Medium, // default to medium for unknown values
        }
    }

    /// Map thinking budget to segment mode.
    /// Low complexity → Precision (strict threshold, simple lookups).
    /// High complexity → Exploratory (relaxed threshold, synthesis).
    /// Medium → Precision (default to strict).
    fn budget_to_mode(budget: &ThinkingBudget) -> SegmentMode {
        match budget {
            ThinkingBudget::High => SegmentMode::Exploratory,
            _ => SegmentMode::Precision,
        }
    }

    /// Map thinking budget to drift threshold.
    fn budget_to_drift_threshold(budget: &ThinkingBudget) -> f32 {
        match budget {
            ThinkingBudget::High => 0.15,
            _ => 0.05,
        }
    }

    /// Normalize the action string to one of the permitted actions.
    /// Falls back to "queryNautivecs" if unrecognized.
    fn normalize_action(action: &str) -> String {
        match action {
            "writeFile" | "readFile" | "runCommand" | "queryNautivecs" => action.to_string(),
            _ => "queryNautivecs".to_string(),
        }
    }

    /// Validate that all RSU IDs in the sequence are unique (Requirement 2.6).
    fn validate_unique_ids(rsus: &[Rsu]) -> Result<()> {
        let mut seen = std::collections::HashSet::new();
        for rsu in rsus {
            if !seen.insert(&rsu.id) {
                bail!(
                    "Segmenter: duplicate RSU ID detected: \"{}\"",
                    rsu.id
                );
            }
        }
        Ok(())
    }

    /// Grounding Search: query nautivecs for concrete technical nouns related to the objective.
    ///
    /// This is the "Local-First" grounding strategy:
    /// 1. Query nautivecs with the objective text
    /// 2. Extract the top technical nouns (struct names, file names, function names)
    /// 3. Return them as a formatted string for injection into the LLM prompt
    ///
    /// If nautivecs returns nothing useful, falls back to extracting nouns from
    /// the objective itself (Semantic Fallback).
    ///
    /// Future: Web search fallback slot (TODO) for external API docs.
    async fn grounding_search(objective: &str, steering: &mut SteeringEngine) -> String {
        // Phase 1: Query nautivecs for relevant code fragments
        let context_result = steering
            .build_context(objective, Some("research"), Some("grounding_search"))
            .await;

        let mut technical_nouns: Vec<String> = Vec::new();

        match context_result {
            Ok(ctx) => {
                // Phase 2: Extract concrete technical nouns from the fragments
                // Look for: Rust identifiers (CamelCase, snake_case), file paths (.rs),
                // struct/enum/trait names, function signatures
                let fragments = &ctx.system_prompt;

                // Extract CamelCase identifiers (struct/enum/trait names)
                for word in fragments.split_whitespace() {
                    let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '.');
                    if clean.len() > 4 {
                        // CamelCase: starts with uppercase, has lowercase after
                        if clean.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                            && clean.chars().any(|c| c.is_lowercase())
                            && !is_common_english(clean)
                        {
                            technical_nouns.push(clean.to_string());
                        }
                        // File paths: ends with .rs
                        if clean.ends_with(".rs") {
                            technical_nouns.push(clean.to_string());
                        }
                        // snake_case identifiers with underscore
                        if clean.contains('_') && clean.chars().all(|c| c.is_alphanumeric() || c == '_') {
                            technical_nouns.push(clean.to_string());
                        }
                    }
                }

                // Deduplicate and take top 15
                technical_nouns.sort();
                technical_nouns.dedup();
                technical_nouns.truncate(15);

                tracing::info!(
                    noun_count = technical_nouns.len(),
                    nouns = ?&technical_nouns[..technical_nouns.len().min(8)],
                    "Grounding search: extracted technical nouns from nautivecs"
                );
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Grounding search: nautivecs unreachable, falling back to objective extraction"
                );
            }
        }

        // Phase 3: Semantic Fallback — if nautivecs returned < 3 nouns,
        // extract from the objective itself
        if technical_nouns.len() < 3 {
            let objective_nouns: Vec<String> = objective
                .split_whitespace()
                .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '/' && c != '.'))
                .filter(|w| {
                    w.len() > 4
                        && (w.contains('/')
                            || w.contains('_')
                            || w.contains('.')
                            || w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false))
                })
                .map(|w| w.to_string())
                .collect();

            for noun in objective_nouns {
                if !technical_nouns.contains(&noun) {
                    technical_nouns.push(noun);
                }
            }
            technical_nouns.truncate(15);
        }

        if technical_nouns.is_empty() {
            return String::new();
        }

        // Format as grounding context for the LLM
        format!(
            "Technical entities found in the codebase:\n- {}\n\n\
             RULE: Your accuracy_check constraints MUST reference at least 2 of these exact terms. \
             Do NOT use abstract phrases like 'ensure all components are identified'. \
             Instead write: 'Must reference [specific_term_1] and [specific_term_2]'.",
            technical_nouns.join("\n- ")
        )
    }
}

/// Check if a word is common English (not a technical identifier)
fn is_common_english(word: &str) -> bool {
    const COMMON: &[&str] = &[
        "The", "This", "That", "These", "Those", "When", "Where", "Which",
        "While", "With", "From", "Into", "Over", "Under", "After", "Before",
        "Between", "Through", "During", "Without", "Within", "About", "Above",
        "Below", "Each", "Every", "Either", "Neither", "Both", "Some", "None",
        "Returns", "Result", "Option", "String", "Error", "Context",
    ];
    COMMON.contains(&word)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that parse_llm_response handles a clean JSON array.
    #[test]
    fn test_parse_clean_json_array() {
        let response = r#"[
            {
                "description": "Gather context",
                "observation": "Load prior data",
                "reasoning": "Analyze patterns",
                "accuracy_check": "Verify bounds",
                "complexity": "low",
                "action": "readFile"
            },
            {
                "description": "Process data",
                "observation": "Use gathered context",
                "reasoning": "Apply algorithm",
                "accuracy_check": "Check output format",
                "complexity": "medium",
                "action": "runCommand"
            }
        ]"#;

        let segments = Segmenter::parse_llm_response(response).unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].description, "Gather context");
        assert_eq!(segments[1].complexity, "medium");
    }

    /// Test that parse_llm_response strips markdown fences.
    #[test]
    fn test_parse_with_markdown_fences() {
        let response = "```json\n[\n{\"description\":\"A\",\"observation\":\"B\",\"reasoning\":\"C\",\"accuracy_check\":\"D\",\"complexity\":\"low\",\"action\":\"readFile\"}\n]\n```";

        let segments = Segmenter::parse_llm_response(response).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].description, "A");
    }

    /// Test that parse_llm_response handles extra text around the array.
    #[test]
    fn test_parse_with_surrounding_text() {
        let response = "Here are the segments:\n[{\"description\":\"X\",\"observation\":\"Y\",\"reasoning\":\"Z\",\"accuracy_check\":\"W\",\"complexity\":\"high\",\"action\":\"writeFile\"}]\nDone.";

        let segments = Segmenter::parse_llm_response(response).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].complexity, "high");
    }

    /// Test that build_rsus assigns correct segment_NNN IDs (Requirement 2.2).
    #[test]
    fn test_build_rsus_assigns_sequential_ids() {
        let segments = vec![
            LlmSegment {
                description: "First".into(),
                observation: "Obs 1".into(),
                reasoning: "Reason 1".into(),
                accuracy_check: "Check 1".into(),
                complexity: "low".into(),
                action: "readFile".into(),
            },
            LlmSegment {
                description: "Second".into(),
                observation: "Obs 2".into(),
                reasoning: "Reason 2".into(),
                accuracy_check: "Check 2".into(),
                complexity: "medium".into(),
                action: "runCommand".into(),
            },
            LlmSegment {
                description: "Third".into(),
                observation: "Obs 3".into(),
                reasoning: "Reason 3".into(),
                accuracy_check: "Check 3".into(),
                complexity: "high".into(),
                action: "writeFile".into(),
            },
        ];

        let rsus = Segmenter::build_rsus("Test objective", &segments);

        assert_eq!(rsus[0].id, "segment_001");
        assert_eq!(rsus[1].id, "segment_002");
        assert_eq!(rsus[2].id, "segment_003");
    }

    /// Test that parent_goal is set on every RSU (Requirement 2.3).
    #[test]
    fn test_build_rsus_sets_parent_goal() {
        let segments = vec![
            LlmSegment {
                description: "A".into(),
                observation: "O".into(),
                reasoning: "R".into(),
                accuracy_check: "C".into(),
                complexity: "low".into(),
                action: "readFile".into(),
            },
            LlmSegment {
                description: "B".into(),
                observation: "O2".into(),
                reasoning: "R2".into(),
                accuracy_check: "C2".into(),
                complexity: "high".into(),
                action: "writeFile".into(),
            },
        ];

        let objective = "Analyze thermal anomaly in tile B02";
        let rsus = Segmenter::build_rsus(objective, &segments);

        for rsu in &rsus {
            assert_eq!(rsu.meta.parent_goal, objective);
        }
    }

    /// Test thinking_budget assignment based on complexity (Requirement 2.4).
    #[test]
    fn test_build_rsus_assigns_thinking_budget() {
        let segments = vec![
            LlmSegment {
                description: "Simple".into(),
                observation: "O".into(),
                reasoning: "R".into(),
                accuracy_check: "C".into(),
                complexity: "low".into(),
                action: "readFile".into(),
            },
            LlmSegment {
                description: "Moderate".into(),
                observation: "O".into(),
                reasoning: "R".into(),
                accuracy_check: "C".into(),
                complexity: "medium".into(),
                action: "readFile".into(),
            },
            LlmSegment {
                description: "Complex".into(),
                observation: "O".into(),
                reasoning: "R".into(),
                accuracy_check: "C".into(),
                complexity: "high".into(),
                action: "readFile".into(),
            },
        ];

        let rsus = Segmenter::build_rsus("Test", &segments);

        assert_eq!(rsus[0].meta.thinking_budget, ThinkingBudget::Low);
        assert_eq!(rsus[1].meta.thinking_budget, ThinkingBudget::Medium);
        assert_eq!(rsus[2].meta.thinking_budget, ThinkingBudget::High);
    }

    /// Test that observation includes prior segment dependencies (Requirement 2.5).
    #[test]
    fn test_build_rsus_populates_observation_dependencies() {
        let segments = vec![
            LlmSegment {
                description: "First".into(),
                observation: "Initial context".into(),
                reasoning: "R".into(),
                accuracy_check: "C".into(),
                complexity: "low".into(),
                action: "readFile".into(),
            },
            LlmSegment {
                description: "Second".into(),
                observation: "Use first result".into(),
                reasoning: "R".into(),
                accuracy_check: "C".into(),
                complexity: "medium".into(),
                action: "readFile".into(),
            },
            LlmSegment {
                description: "Third".into(),
                observation: "Combine results".into(),
                reasoning: "R".into(),
                accuracy_check: "C".into(),
                complexity: "high".into(),
                action: "readFile".into(),
            },
        ];

        let rsus = Segmenter::build_rsus("Test", &segments);

        // First segment has no prior dependencies
        assert_eq!(rsus[0].phases.observation, "Initial context");
        assert!(!rsus[0].phases.observation.contains("Context dependencies"));

        // Second segment references segment_001
        assert!(rsus[1].phases.observation.contains("segment_001"));
        assert!(rsus[1].phases.observation.contains("Context dependencies"));

        // Third segment references segment_001 and segment_002
        assert!(rsus[2].phases.observation.contains("segment_001"));
        assert!(rsus[2].phases.observation.contains("segment_002"));
    }

    /// Test that validate_unique_ids catches duplicates (Requirement 2.6).
    #[test]
    fn test_validate_unique_ids_rejects_duplicates() {
        let now = Utc::now();
        let make_rsu = |id: &str| Rsu {
            context: "https://kiro.ai".to_string(),
            rsu_type: "SegmentTask".to_string(),
            id: id.to_string(),
            meta: RsuMeta {
                parent_goal: "test".to_string(),
                thinking_budget: ThinkingBudget::Low,
                steering_ref: "steering/test.md".to_string(),
                steering_policy: RsuSteeringPolicy::precision(),
                context_ttl_seconds: 15,
                created_at: now,
            },
            phases: RsuPhases {
                observation: "obs".to_string(),
                reasoning: "reason".to_string(),
                accuracy_check: "check".to_string(),
            },
            execution: RsuExecution {
                action: "readFile".to_string(),
                parameters: serde_json::json!({}),
            },
            dependencies: vec![],
        };

        let rsus = vec![
            make_rsu("segment_001"),
            make_rsu("segment_002"),
            make_rsu("segment_001"), // duplicate!
        ];

        let err = Segmenter::validate_unique_ids(&rsus).unwrap_err();
        assert!(err.to_string().contains("duplicate"));
        assert!(err.to_string().contains("segment_001"));
    }

    /// Test that validate_unique_ids passes for unique IDs.
    #[test]
    fn test_validate_unique_ids_passes_for_unique() {
        let now = Utc::now();
        let make_rsu = |id: &str| Rsu {
            context: "https://kiro.ai".to_string(),
            rsu_type: "SegmentTask".to_string(),
            id: id.to_string(),
            meta: RsuMeta {
                parent_goal: "test".to_string(),
                thinking_budget: ThinkingBudget::Low,
                steering_ref: "steering/test.md".to_string(),
                steering_policy: RsuSteeringPolicy::precision(),
                context_ttl_seconds: 15,
                created_at: now,
            },
            phases: RsuPhases {
                observation: "obs".to_string(),
                reasoning: "reason".to_string(),
                accuracy_check: "check".to_string(),
            },
            execution: RsuExecution {
                action: "readFile".to_string(),
                parameters: serde_json::json!({}),
            },
            dependencies: vec![],
        };

        let rsus = vec![
            make_rsu("segment_001"),
            make_rsu("segment_002"),
            make_rsu("segment_003"),
        ];

        assert!(Segmenter::validate_unique_ids(&rsus).is_ok());
    }

    /// Test complexity_to_budget mapping.
    #[test]
    fn test_complexity_to_budget() {
        assert_eq!(Segmenter::complexity_to_budget("low"), ThinkingBudget::Low);
        assert_eq!(Segmenter::complexity_to_budget("medium"), ThinkingBudget::Medium);
        assert_eq!(Segmenter::complexity_to_budget("high"), ThinkingBudget::High);
        // Unknown defaults to medium
        assert_eq!(Segmenter::complexity_to_budget("extreme"), ThinkingBudget::Medium);
        assert_eq!(Segmenter::complexity_to_budget(""), ThinkingBudget::Medium);
    }

    /// Test normalize_action falls back to queryNautivecs for unknown actions.
    #[test]
    fn test_normalize_action() {
        assert_eq!(Segmenter::normalize_action("writeFile"), "writeFile");
        assert_eq!(Segmenter::normalize_action("readFile"), "readFile");
        assert_eq!(Segmenter::normalize_action("runCommand"), "runCommand");
        assert_eq!(Segmenter::normalize_action("queryNautivecs"), "queryNautivecs");
        assert_eq!(Segmenter::normalize_action("deleteFile"), "queryNautivecs");
        assert_eq!(Segmenter::normalize_action(""), "queryNautivecs");
    }

    /// Test that all generated RSUs have valid JSON-LD fields.
    #[test]
    fn test_build_rsus_json_ld_fields() {
        let segments = vec![
            LlmSegment {
                description: "A".into(),
                observation: "O".into(),
                reasoning: "R".into(),
                accuracy_check: "C".into(),
                complexity: "medium".into(),
                action: "readFile".into(),
            },
        ];

        let rsus = Segmenter::build_rsus("Goal", &segments);

        assert_eq!(rsus[0].context, "https://kiro.ai");
        assert_eq!(rsus[0].rsu_type, "SegmentTask");
    }
}
