//! Per-model corrector presets with escalation ladder.
//!
//! Different models need different correction strategies:
//! - Reasoning models (R1, Qwen3): think a lot natively, give them rope
//! - Direct coders (Qwen2.5-Coder): flag loops fast, terse expectations
//! - Plausible-but-buggy (Gemma): heavy translator, vector inject critical
//! - Small models (Phi-3 mini): drift fast, all corrections on
//!
//! Escalation ladder (4 levels):
//! 1. SUGGEST  — soft nudge: "I see you thinking a lot. Have you tried wso_search?"
//! 2. DEMAND   — require ack: "You must answer: did the search help, or what's next?"
//! 3. REBOOT   — hard reset, keep only original task, log lesson to nautivecs
//! 4. SWAP     — if reboot doesn't help, request a model swap from the operator
//!
//! Tunables per preset live in cluster_config.toml under [corrector_preset.{name}]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Strength of automatic correction the corrector applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CorrectionStrength {
    None,
    Light,
    Medium,
    Heavy,
}

/// One preset describing how to handle a model family.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectorPreset {
    /// How many rounds of pure thinking are allowed before flagging.
    /// Reasoning models like R1 / Qwen3 think a lot natively → high tolerance.
    pub think_tolerance: u32,

    /// How many same-tool repetitions before escalation starts.
    pub loop_threshold: u32,

    /// Round at which we softly suggest think_harder/wso_search.
    pub suggest_after: u32,

    /// Round at which we demand an explicit acknowledgment from the model.
    pub demand_ack_after: u32,

    /// Round at which we hard-reset the conversation.
    pub reboot_after: u32,

    /// Whether n8n auto-fires a WSO web search if the model stalls.
    pub wso_auto: bool,

    /// Whether to inject nautivecs context into the corrector's prompts.
    pub vector_inject: bool,

    /// How aggressive the JSON/code fixer should be.
    #[serde(default = "default_strength")]
    pub correction_strength: CorrectionStrength,

    /// Human-readable description.
    #[serde(default)]
    pub description: String,
}

fn default_strength() -> CorrectionStrength { CorrectionStrength::Medium }

impl Default for CorrectorPreset {
    fn default() -> Self {
        // Sensible default: qwen_coder-like
        Self {
            think_tolerance: 4,
            loop_threshold: 3,
            suggest_after: 3,
            demand_ack_after: 5,
            reboot_after: 7,
            wso_auto: true,
            vector_inject: true,
            correction_strength: CorrectionStrength::Medium,
            description: "Default preset (qwen_coder-style)".into(),
        }
    }
}

/// Pattern → preset name mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetMatch {
    pub pattern: String,
    pub preset: String,
}

/// Full registry of presets, loaded once at startup from cluster_config.toml.
#[derive(Debug, Clone)]
pub struct PresetRegistry {
    pub presets: HashMap<String, CorrectorPreset>,
    pub matches: Vec<PresetMatch>,
}

impl PresetRegistry {
    /// Load presets from cluster_config.toml.
    pub fn load(config_path: &str) -> Self {
        let content = std::fs::read_to_string(config_path).unwrap_or_default();
        let table: toml::Table = content.parse().unwrap_or_default();

        // [corrector_preset.{name}] sections
        let mut presets: HashMap<String, CorrectorPreset> = HashMap::new();
        if let Some(toml::Value::Table(presets_table)) = table.get("corrector_preset") {
            for (name, val) in presets_table {
                if let Ok(p) = val.clone().try_into::<CorrectorPreset>() {
                    presets.insert(name.clone(), p);
                }
            }
        }
        // Always have a default
        presets.entry("default".to_string()).or_insert_with(CorrectorPreset::default);

        // [[corrector_preset_match]] array
        let mut matches: Vec<PresetMatch> = Vec::new();
        if let Some(toml::Value::Array(arr)) = table.get("corrector_preset_match") {
            for v in arr {
                if let Ok(m) = v.clone().try_into::<PresetMatch>() {
                    matches.push(m);
                }
            }
        }

        Self { presets, matches }
    }

    /// Resolve a model name to a preset. First pattern match wins; falls back to default.
    pub fn resolve(&self, model_name: &str) -> &CorrectorPreset {
        let lower = model_name.to_lowercase();
        for m in &self.matches {
            if let Ok(re) = regex::Regex::new(&format!("(?i){}", m.pattern)) {
                if re.is_match(&lower) {
                    if let Some(p) = self.presets.get(&m.preset) {
                        return p;
                    }
                }
            }
        }
        self.presets.get("default").unwrap()
    }

    /// Override a preset for a specific worker (manual config from cluster_config).
    pub fn for_worker<'a>(&'a self, model_name: &str, override_preset: Option<&str>) -> &'a CorrectorPreset {
        if let Some(name) = override_preset {
            if let Some(p) = self.presets.get(name) {
                return p;
            }
        }
        self.resolve(model_name)
    }
}

// ── Escalation ladder ────────────────────────────────────────────────────────

/// What the escalator decided to do this round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Escalation {
    /// No action — model is making progress or under tolerance.
    Continue,
    /// Soft suggestion to try a different tool (no ack required).
    Suggest(String),
    /// Demand an explicit ack — model MUST address this before proceeding.
    Demand(String),
    /// Hard reset the conversation. Inject lesson to nautivecs.
    Reboot(String),
    /// Suggest swapping the model itself (after reboot didn't help).
    SwapModel(String),
}

/// Decide what to do given the current state.
///
/// `repetition_count`: how many times the same tool has been called in a row
/// `think_rounds`: how many consecutive rounds without a tool call (pure thinking)
/// `total_rounds`: total rounds in this turn
/// `last_escalation`: what we said last round (so we don't repeat suggestions)
pub fn decide_escalation(
    preset: &CorrectorPreset,
    repetition_count: u32,
    think_rounds: u32,
    total_rounds: u32,
    last_escalation: Option<&Escalation>,
    last_tool_name: &str,
) -> Escalation {
    // ── Loop detection (same tool repeated) ─────────────────────────────────
    if repetition_count >= preset.loop_threshold {
        let level = repetition_count - preset.loop_threshold + 1;
        match level {
            1..=2 => return Escalation::Suggest(format!(
                "I see you've called '{}' {} times. Try a different approach — \
                 maybe `think_harder` to query the knowledge base, or `wso_search` \
                 for current docs?",
                last_tool_name, repetition_count
            )),
            3 => return Escalation::Demand(format!(
                "STOP. You've called '{}' {} times with no progress. \
                 You MUST answer in your next response: \
                 (a) WHY did the previous attempts fail? \
                 (b) What is your concrete next step? \
                 No more tool calls until you answer both questions.",
                last_tool_name, repetition_count
            )),
            4..=5 => return Escalation::Reboot(format!(
                "Loop hit on '{}' ({}x). Hard reset — keeping only the original task. \
                 Lesson logged to memory for future runs.",
                last_tool_name, repetition_count
            )),
            _ => return Escalation::SwapModel(format!(
                "Cannot recover from loop on '{}' even after reboot. \
                 Recommend swapping to a different model.",
                last_tool_name
            )),
        }
    }

    // ── Thinking-without-tool-calls detection ───────────────────────────────
    if think_rounds >= preset.think_tolerance {
        let depth = think_rounds - preset.think_tolerance + 1;

        if depth == 1 && total_rounds >= preset.suggest_after {
            return Escalation::Suggest(format!(
                "I see you're thinking a lot on this ({} rounds). \
                 Have you tried `think_harder` to search the knowledge base, \
                 or `wso_search` for current documentation? \
                 If you're confident in your answer, just produce it.",
                think_rounds
            ));
        }

        if total_rounds >= preset.demand_ack_after && !matches!(last_escalation, Some(Escalation::Demand(_))) {
            return Escalation::Demand(format!(
                "You've been reasoning for {} rounds. Answer in your next message: \
                 (a) Did you search the knowledge base or web? \
                 (b) What is your concrete output or next action? \
                 No more pure reasoning — give a tool call OR a final answer.",
                think_rounds
            ));
        }

        if total_rounds >= preset.reboot_after {
            return Escalation::Reboot(format!(
                "Reasoning loop detected ({} rounds, demand ignored). \
                 Hard reset — restating original task only. \
                 Lesson: model preset '{}' may need lower thresholds.",
                think_rounds, "current"
            ));
        }
    }

    Escalation::Continue
}

/// Format an escalation as a system message to inject into the conversation.
pub fn format_escalation_message(esc: &Escalation) -> Option<String> {
    match esc {
        Escalation::Continue => None,
        Escalation::Suggest(s) => Some(format!("[SUGGESTION FROM CORRECTOR]: {}", s)),
        Escalation::Demand(s)  => Some(format!("[DEMAND — YOU MUST ADDRESS THIS]: {}", s)),
        Escalation::Reboot(s)  => Some(format!("[CONVERSATION RESET]: {}", s)),
        Escalation::SwapModel(s) => Some(format!("[OPERATOR ALERT — MODEL SWAP RECOMMENDED]: {}", s)),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_preset() -> CorrectorPreset {
        CorrectorPreset {
            think_tolerance: 5,
            loop_threshold: 3,
            suggest_after: 3,
            demand_ack_after: 5,
            reboot_after: 7,
            wso_auto: true,
            vector_inject: true,
            correction_strength: CorrectionStrength::Medium,
            description: "test".into(),
        }
    }

    #[test]
    fn no_escalation_when_progressing() {
        let p = test_preset();
        let e = decide_escalation(&p, 0, 0, 1, None, "read_file");
        assert_eq!(e, Escalation::Continue);
    }

    #[test]
    fn loop_triggers_suggest() {
        let p = test_preset();
        let e = decide_escalation(&p, 3, 0, 5, None, "read_file");
        assert!(matches!(e, Escalation::Suggest(_)));
    }

    #[test]
    fn loop_triggers_demand() {
        let p = test_preset();
        let e = decide_escalation(&p, 5, 0, 7, None, "read_file");
        assert!(matches!(e, Escalation::Demand(_)));
    }

    #[test]
    fn loop_triggers_reboot() {
        let p = test_preset();
        let e = decide_escalation(&p, 6, 0, 8, None, "read_file");
        assert!(matches!(e, Escalation::Reboot(_)));
    }

    #[test]
    fn thinking_long_triggers_suggest() {
        let p = test_preset();
        let e = decide_escalation(&p, 0, 5, 5, None, "");
        assert!(matches!(e, Escalation::Suggest(_)));
    }
}
