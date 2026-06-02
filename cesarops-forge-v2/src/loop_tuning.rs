//! Loop-engine tuning persisted in `cluster_config.toml` `[tuning]`.
//! Exposed via GET/POST `/cluster/loop-tuning` for the Forge cluster panel.

use serde::{Deserialize, Serialize};
use tracing::warn;

pub const CFG_PATH: &str = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopTuning {
    pub max_think_rounds: u32,
    pub generation_timeout_secs: u64,
    pub thinker_timeout_secs: u64,
    pub max_generation_tokens: u32,
    pub temperature: f32,
    /// Disable all corrector nudges (think-only ladder + optional LLM fixes).
    pub skip_corrector: bool,
    /// Consecutive forge rounds without a tool call before think-only nudges.
    pub corrector_think_only_after: u32,
    /// Same tool called this many times → demand a progress report (no instant kill).
    pub corrector_hard_repeat_cap: u32,
    /// After this many identical tool calls, force plain-text final answer.
    pub corrector_hard_terminate_after: u32,
    /// Route malformed tool JSON through corrector LLM cascade and auto-execute.
    pub corrector_tool_fix: bool,
    /// Ask corrector LLM whether repeated tool calls are justified.
    pub corrector_llm_loop_judge: bool,
}

impl Default for LoopTuning {
    fn default() -> Self {
        Self {
            max_think_rounds: 50,
            generation_timeout_secs: 1200,
            thinker_timeout_secs: 60,
            max_generation_tokens: 16384,
            temperature: 0.4,
            skip_corrector: false,
            corrector_think_only_after: 20,
            corrector_hard_repeat_cap: 12,
            corrector_hard_terminate_after: 18,
            corrector_tool_fix: false,
            corrector_llm_loop_judge: false,
        }
    }
}

impl LoopTuning {
    fn from_toml_table(tuning: &toml::Table) -> Self {
        let d = Self::default();
        Self {
            max_think_rounds: int(tuning, "max_think_rounds", d.max_think_rounds as i64) as u32,
            generation_timeout_secs: int(tuning, "generation_timeout_secs", d.generation_timeout_secs as i64) as u64,
            thinker_timeout_secs: int(tuning, "thinker_timeout_secs", d.thinker_timeout_secs as i64) as u64,
            max_generation_tokens: int(tuning, "max_generation_tokens", d.max_generation_tokens as i64) as u32,
            temperature: tuning
                .get("temperature")
                .and_then(|v| v.as_float())
                .unwrap_or(d.temperature as f64) as f32,
            skip_corrector: bool_val(tuning, "skip_corrector", d.skip_corrector),
            corrector_think_only_after: int(
                tuning,
                "corrector_think_only_after",
                d.corrector_think_only_after as i64,
            ) as u32,
            corrector_hard_repeat_cap: int(
                tuning,
                "corrector_hard_repeat_cap",
                d.corrector_hard_repeat_cap as i64,
            ) as u32,
            corrector_hard_terminate_after: int(
                tuning,
                "corrector_hard_terminate_after",
                d.corrector_hard_terminate_after as i64,
            ) as u32,
            corrector_tool_fix: bool_val(tuning, "corrector_tool_fix", d.corrector_tool_fix),
            corrector_llm_loop_judge: bool_val(
                tuning,
                "corrector_llm_loop_judge",
                d.corrector_llm_loop_judge,
            ),
        }
    }
}

fn int(table: &toml::Table, key: &str, default: i64) -> i64 {
    table
        .get(key)
        .and_then(|v| v.as_integer())
        .unwrap_or(default)
}

fn bool_val(table: &toml::Table, key: &str, default: bool) -> bool {
    table
        .get(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

pub fn load() -> LoopTuning {
    let content = std::fs::read_to_string(CFG_PATH).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    table
        .get("tuning")
        .and_then(|v| v.as_table())
        .map(LoopTuning::from_toml_table)
        .unwrap_or_default()
}

/// Merge partial JSON updates into `[tuning]` and persist.
pub fn save_partial(patch: &serde_json::Value) -> Result<LoopTuning, String> {
    let mut current = load();
    if let Some(obj) = patch.as_object() {
        if let Some(v) = obj.get("max_think_rounds").and_then(|v| v.as_u64()) {
            current.max_think_rounds = v as u32;
        }
        if let Some(v) = obj.get("generation_timeout_secs").and_then(|v| v.as_u64()) {
            current.generation_timeout_secs = v;
        }
        if let Some(v) = obj.get("thinker_timeout_secs").and_then(|v| v.as_u64()) {
            current.thinker_timeout_secs = v;
        }
        if let Some(v) = obj.get("max_generation_tokens").and_then(|v| v.as_u64()) {
            current.max_generation_tokens = v as u32;
        }
        if let Some(v) = obj.get("temperature").and_then(|v| v.as_f64()) {
            current.temperature = v as f32;
        }
        if let Some(v) = obj.get("skip_corrector").and_then(|v| v.as_bool()) {
            current.skip_corrector = v;
        }
        if let Some(v) = obj.get("corrector_think_only_after").and_then(|v| v.as_u64()) {
            current.corrector_think_only_after = v.clamp(1, 200) as u32;
        }
        if let Some(v) = obj.get("corrector_hard_repeat_cap").and_then(|v| v.as_u64()) {
            current.corrector_hard_repeat_cap = v.clamp(2, 100) as u32;
        }
        if let Some(v) = obj.get("corrector_hard_terminate_after").and_then(|v| v.as_u64()) {
            current.corrector_hard_terminate_after = v.clamp(3, 200) as u32;
        }
        if let Some(v) = obj.get("corrector_tool_fix").and_then(|v| v.as_bool()) {
            current.corrector_tool_fix = v;
        }
        if let Some(v) = obj.get("corrector_llm_loop_judge").and_then(|v| v.as_bool()) {
            current.corrector_llm_loop_judge = v;
        }
    }

    let content = std::fs::read_to_string(CFG_PATH).map_err(|e| e.to_string())?;
    let mut doc: toml_edit::DocumentMut =
        content.parse::<toml_edit::DocumentMut>().map_err(|e| e.to_string())?;
    let tuning = doc
        .entry("tuning")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let table = tuning
        .as_table_mut()
        .ok_or_else(|| "tuning is not a table".to_string())?;

    write_tuning_table(table, &current);

    std::fs::write(CFG_PATH, doc.to_string()).map_err(|e| e.to_string())?;
    Ok(current)
}

fn write_tuning_table(table: &mut toml_edit::Table, t: &LoopTuning) {
    use toml_edit::{Formatted, Item, Value};

    table.insert(
        "max_think_rounds",
        Item::Value(Value::Integer(Formatted::new(t.max_think_rounds as i64))),
    );
    table.insert(
        "generation_timeout_secs",
        Item::Value(Value::Integer(Formatted::new(t.generation_timeout_secs as i64))),
    );
    table.insert(
        "thinker_timeout_secs",
        Item::Value(Value::Integer(Formatted::new(t.thinker_timeout_secs as i64))),
    );
    table.insert(
        "max_generation_tokens",
        Item::Value(Value::Integer(Formatted::new(t.max_generation_tokens as i64))),
    );
    table.insert(
        "temperature",
        Item::Value(Value::Float(Formatted::new(t.temperature as f64))),
    );
    table.insert(
        "skip_corrector",
        Item::Value(Value::Boolean(Formatted::new(t.skip_corrector))),
    );
    table.insert(
        "corrector_think_only_after",
        Item::Value(Value::Integer(Formatted::new(t.corrector_think_only_after as i64))),
    );
    table.insert(
        "corrector_hard_repeat_cap",
        Item::Value(Value::Integer(Formatted::new(t.corrector_hard_repeat_cap as i64))),
    );
    table.insert(
        "corrector_hard_terminate_after",
        Item::Value(Value::Integer(Formatted::new(t.corrector_hard_terminate_after as i64))),
    );
    table.insert(
        "corrector_tool_fix",
        Item::Value(Value::Boolean(Formatted::new(t.corrector_tool_fix))),
    );
    table.insert(
        "corrector_llm_loop_judge",
        Item::Value(Value::Boolean(Formatted::new(t.corrector_llm_loop_judge))),
    );
}

pub fn load_for_loop(preset_think_tolerance: u32) -> LoopRuntimeTuning {
    let t = load();
    let think_escalate_after = if t.skip_corrector {
        u32::MAX
    } else {
        t.corrector_think_only_after.max(preset_think_tolerance)
    };
    let hard_repeat_cap = t.corrector_hard_repeat_cap.max(2);
    let hard_terminate_after = t
        .corrector_hard_terminate_after
        .max(hard_repeat_cap.saturating_add(1));
    LoopRuntimeTuning {
        max_diagnosis: t.max_think_rounds,
        skip_corrector: t.skip_corrector,
        think_escalate_after,
        hard_repeat_cap: hard_repeat_cap as usize,
        hard_terminate_after: hard_terminate_after as usize,
        corrector_tool_fix: t.corrector_tool_fix,
        corrector_llm_loop_judge: t.corrector_llm_loop_judge && !t.skip_corrector,
        temperature: t.temperature,
    }
}

/// Subset used by `loop_engine` each `/send` turn.
pub struct LoopRuntimeTuning {
    pub max_diagnosis: u32,
    pub skip_corrector: bool,
    pub think_escalate_after: u32,
    pub hard_repeat_cap: usize,
    pub hard_terminate_after: usize,
    pub corrector_tool_fix: bool,
    pub corrector_llm_loop_judge: bool,
    pub temperature: f32,
}

pub fn sync_skip_corrector_file(enable: bool) {
    let content = match std::fs::read_to_string(CFG_PATH) {
        Ok(s) => s,
        Err(e) => {
            warn!("sync_skip_corrector read: {}", e);
            return;
        }
    };
    let updated = if enable {
        content.replace("skip_corrector = true", "skip_corrector = false")
    } else {
        content.replace("skip_corrector = false", "skip_corrector = true")
    };
    let _ = std::fs::write(CFG_PATH, &updated);
}
