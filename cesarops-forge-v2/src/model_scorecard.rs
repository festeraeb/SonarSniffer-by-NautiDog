//! Per-model performance scorecard with task-typed scoring + tool-impact tracking.
//!
//! Tracks which models excel at which task types, and whether the forge's tools
//! (corrector, translator, vector injection, think_harder) are actually moving
//! accuracy in the right direction.
//!
//! Persisted to ~/.cache/cesarops/model_scorecard.json so we keep history across
//! sessions. The cluster panel reads it to surface "best model for task X" and
//! flag underperformers.
//!
//! Scoring dimensions per (model, task_type):
//!   - attempts: how many times this model attempted this task type
//!   - first_pass_compile_rate: % that compiled on round 1 (no fixes needed)
//!   - final_compile_rate: % that compiled within 3 retry rounds
//!   - mean_rounds_to_pass: average retries needed
//!   - tool_uplift: how much each tool moved accuracy
//!
//! Hot-swap logic: if a model's last 5 attempts at a task type all failed,
//! recommend swapping. The conductor honors the recommendation automatically.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Categories of work we dispatch to fleet models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    /// Generate a WGSL compute shader from spec.
    WgslShader,
    /// Generate Rust code (engine, glue, library).
    RustCode,
    /// Generate HTML/JS frontend code.
    FrontendUi,
    /// Read existing code and produce an analysis or design doc.
    Analysis,
    /// Refactor / restructure existing code.
    Refactor,
    /// Fix a malformed JSON tool call.
    JsonRepair,
    /// Judge whether a repeated tool call is justified (corrector judgment).
    LoopJudgment,
    /// Search the knowledge base / web for context (think_harder).
    Research,
    /// Translate output from one model's format to another's.
    Translation,
}

impl TaskType {
    pub fn name(&self) -> &'static str {
        match self {
            Self::WgslShader   => "wgsl_shader",
            Self::RustCode     => "rust_code",
            Self::FrontendUi   => "frontend_ui",
            Self::Analysis     => "analysis",
            Self::Refactor     => "refactor",
            Self::JsonRepair   => "json_repair",
            Self::LoopJudgment => "loop_judgment",
            Self::Research     => "research",
            Self::Translation  => "translation",
        }
    }
}

/// One attempt outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub timestamp: u64,
    pub model: String,
    pub task_type: TaskType,
    /// Round at which it passed (1 = first try, 0 = never passed).
    pub passed_at_round: u32,
    /// Total rounds attempted (max 3).
    pub total_rounds: u32,
    /// Did the corrector help this attempt?
    pub corrector_engaged: bool,
    pub corrector_helped: bool,
    /// Did vector injection (think_harder) get used?
    pub vector_injected: bool,
    pub vector_helped: bool,
    /// Did translator engage?
    pub translator_engaged: bool,
    pub translator_helped: bool,
    /// Free-form failure reason if passed_at_round == 0.
    pub failure_note: Option<String>,
}

/// Aggregated stats for one (model, task_type) cell.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CellStats {
    pub attempts: u32,
    pub first_pass: u32,
    pub final_pass: u32,
    pub never_passed: u32,
    pub total_rounds: u32,
    /// Counts of (engaged, helped) for each tool.
    pub corrector_engaged: u32,
    pub corrector_helped: u32,
    pub vector_engaged: u32,
    pub vector_helped: u32,
    pub translator_engaged: u32,
    pub translator_helped: u32,
    /// Last 5 attempt outcomes (true = passed). Used for hot-swap decisions.
    pub recent_outcomes: Vec<bool>,
}

impl CellStats {
    pub fn first_pass_rate(&self) -> f32 {
        if self.attempts == 0 { 0.0 } else { self.first_pass as f32 / self.attempts as f32 }
    }
    pub fn final_pass_rate(&self) -> f32 {
        if self.attempts == 0 { 0.0 } else { self.final_pass as f32 / self.attempts as f32 }
    }
    pub fn mean_rounds_to_pass(&self) -> f32 {
        if self.final_pass == 0 { 0.0 } else { self.total_rounds as f32 / self.final_pass as f32 }
    }
    /// Tool uplift: did this tool actually help when it engaged?
    pub fn corrector_uplift(&self) -> f32 {
        if self.corrector_engaged == 0 { 0.0 }
        else { self.corrector_helped as f32 / self.corrector_engaged as f32 }
    }
    pub fn vector_uplift(&self) -> f32 {
        if self.vector_engaged == 0 { 0.0 }
        else { self.vector_helped as f32 / self.vector_engaged as f32 }
    }
    pub fn translator_uplift(&self) -> f32 {
        if self.translator_engaged == 0 { 0.0 }
        else { self.translator_helped as f32 / self.translator_engaged as f32 }
    }
    /// Recommend hot-swap: last 5 outcomes all failed.
    pub fn should_swap(&self) -> bool {
        self.recent_outcomes.len() >= 5 && self.recent_outcomes.iter().all(|&p| !p)
    }
    /// Confidence score 0.0–1.0 for using this model for this task.
    /// Combines final_pass_rate and recency.
    pub fn confidence(&self) -> f32 {
        if self.attempts < 2 { return 0.5; } // Insufficient data
        let recent_ok = if self.recent_outcomes.is_empty() {
            self.final_pass_rate()
        } else {
            let n_pass = self.recent_outcomes.iter().filter(|&&p| p).count();
            n_pass as f32 / self.recent_outcomes.len() as f32
        };
        // 70% weight on recent, 30% on lifetime
        0.7 * recent_ok + 0.3 * self.final_pass_rate()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scorecard {
    /// (model_name, task_type_name) → stats
    pub cells: HashMap<String, CellStats>,
    /// All raw attempt records (kept for diagnostics, capped at 1000 latest).
    pub history: Vec<AttemptRecord>,
}

impl Scorecard {
    fn cell_key(model: &str, task: TaskType) -> String {
        format!("{}::{}", model, task.name())
    }

    pub fn record(&mut self, attempt: AttemptRecord) {
        let key = Self::cell_key(&attempt.model, attempt.task_type);
        let cell = self.cells.entry(key).or_default();

        cell.attempts += 1;
        let passed = attempt.passed_at_round > 0;
        if passed {
            cell.final_pass += 1;
            cell.total_rounds += attempt.passed_at_round;
            if attempt.passed_at_round == 1 {
                cell.first_pass += 1;
            }
        } else {
            cell.never_passed += 1;
        }

        if attempt.corrector_engaged {
            cell.corrector_engaged += 1;
            if attempt.corrector_helped { cell.corrector_helped += 1; }
        }
        if attempt.vector_injected {
            cell.vector_engaged += 1;
            if attempt.vector_helped { cell.vector_helped += 1; }
        }
        if attempt.translator_engaged {
            cell.translator_engaged += 1;
            if attempt.translator_helped { cell.translator_helped += 1; }
        }

        // Track recent outcomes (keep last 5)
        cell.recent_outcomes.push(passed);
        while cell.recent_outcomes.len() > 5 {
            cell.recent_outcomes.remove(0);
        }

        // History (cap at 1000 latest)
        self.history.push(attempt);
        while self.history.len() > 1000 {
            self.history.remove(0);
        }
    }

    /// Pick the best model for a task type from a candidate pool.
    /// Returns (model_name, confidence). Falls back to the first candidate
    /// if no data exists for any.
    pub fn pick_best<'a>(&self, candidates: &'a [String], task: TaskType) -> (&'a str, f32) {
        if candidates.is_empty() { return ("", 0.0); }

        let mut best: (&str, f32) = (&candidates[0], 0.0);
        for cand in candidates {
            let key = Self::cell_key(cand, task);
            let conf = self.cells.get(&key).map(|c| c.confidence()).unwrap_or(0.5);
            if conf > best.1 {
                best = (cand.as_str(), conf);
            }
        }
        best
    }

    /// Models the conductor should hot-swap away from for this task.
    pub fn swap_recommendations(&self, task: TaskType) -> Vec<String> {
        self.cells.iter()
            .filter(|(k, c)| k.ends_with(task.name()) && c.should_swap())
            .filter_map(|(k, _)| k.split("::").next().map(String::from))
            .collect()
    }

    /// Snapshot of which tools are improving accuracy globally.
    pub fn tool_impact_summary(&self) -> ToolImpactSummary {
        let mut s = ToolImpactSummary::default();
        for cell in self.cells.values() {
            s.corrector_engaged  += cell.corrector_engaged;
            s.corrector_helped   += cell.corrector_helped;
            s.vector_engaged     += cell.vector_engaged;
            s.vector_helped      += cell.vector_helped;
            s.translator_engaged += cell.translator_engaged;
            s.translator_helped  += cell.translator_helped;
        }
        s
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolImpactSummary {
    pub corrector_engaged: u32,
    pub corrector_helped: u32,
    pub vector_engaged: u32,
    pub vector_helped: u32,
    pub translator_engaged: u32,
    pub translator_helped: u32,
}

impl ToolImpactSummary {
    pub fn corrector_uplift(&self) -> f32 {
        if self.corrector_engaged == 0 { 0.0 }
        else { self.corrector_helped as f32 / self.corrector_engaged as f32 }
    }
    pub fn vector_uplift(&self) -> f32 {
        if self.vector_engaged == 0 { 0.0 }
        else { self.vector_helped as f32 / self.vector_engaged as f32 }
    }
    pub fn translator_uplift(&self) -> f32 {
        if self.translator_engaged == 0 { 0.0 }
        else { self.translator_helped as f32 / self.translator_engaged as f32 }
    }
}

// ── Persistence ──────────────────────────────────────────────────────────────

fn scorecard_path() -> PathBuf {
    let cache_dir = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".cache"))
                .unwrap_or_else(|_| PathBuf::from("/tmp"))
        });
    cache_dir.join("cesarops").join("model_scorecard.json")
}

pub fn load() -> Scorecard {
    let path = scorecard_path();
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Scorecard::default(),
    }
}

pub fn save(card: &Scorecard) -> std::io::Result<()> {
    let path = scorecard_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(card)?;
    std::fs::write(&path, bytes)
}

pub fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn record(model: &str, task: TaskType, pass_round: u32) -> AttemptRecord {
        AttemptRecord {
            timestamp: 0, model: model.into(), task_type: task,
            passed_at_round: pass_round, total_rounds: pass_round.max(1),
            corrector_engaged: false, corrector_helped: false,
            vector_injected: false, vector_helped: false,
            translator_engaged: false, translator_helped: false,
            failure_note: None,
        }
    }

    #[test]
    fn pick_best_chooses_higher_confidence() {
        let mut s = Scorecard::default();
        // model_a: 3 first-pass on rust_code
        for _ in 0..3 { s.record(record("model_a", TaskType::RustCode, 1)); }
        // model_b: 3 attempts, 1 passed
        for _ in 0..2 { s.record(record("model_b", TaskType::RustCode, 0)); }
        s.record(record("model_b", TaskType::RustCode, 2));

        let cands = vec!["model_a".to_string(), "model_b".to_string()];
        let (best, conf) = s.pick_best(&cands, TaskType::RustCode);
        assert_eq!(best, "model_a");
        assert!(conf > 0.8);
    }

    #[test]
    fn swap_recommendation_after_5_failures() {
        let mut s = Scorecard::default();
        for _ in 0..5 { s.record(record("bad_model", TaskType::WgslShader, 0)); }
        let recs = s.swap_recommendations(TaskType::WgslShader);
        assert_eq!(recs, vec!["bad_model".to_string()]);
    }

    #[test]
    fn tool_uplift_tracked() {
        let mut s = Scorecard::default();
        let mut r = record("m", TaskType::RustCode, 2);
        r.corrector_engaged = true;
        r.corrector_helped = true;
        s.record(r);

        let summary = s.tool_impact_summary();
        assert_eq!(summary.corrector_engaged, 1);
        assert_eq!(summary.corrector_helped, 1);
        assert!((summary.corrector_uplift() - 1.0).abs() < 1e-6);
    }
}
