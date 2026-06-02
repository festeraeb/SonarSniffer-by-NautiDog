use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

const RECENT_CAP: usize = 200;

#[derive(Clone, Debug, Serialize)]
pub struct ToolCallEvent {
    pub ts: u64,
    pub tool: String,
    pub delegated_to_mcp: bool,
    pub ok: bool,
    pub duration_ms: u128,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolTelemetrySnapshot {
    pub total_calls: u64,
    pub counts_by_tool: HashMap<String, u64>,
    pub recent: Vec<ToolCallEvent>,
}

#[derive(Default)]
pub struct ToolTelemetry {
    total_calls: u64,
    counts_by_tool: HashMap<String, u64>,
    recent: VecDeque<ToolCallEvent>,
}

impl ToolTelemetry {
    pub fn record(&mut self, tool: &str, delegated_to_mcp: bool, ok: bool, duration_ms: u128) {
        self.total_calls += 1;
        *self.counts_by_tool.entry(tool.to_string()).or_insert(0) += 1;
        self.recent.push_back(ToolCallEvent {
            ts: now_unix(),
            tool: tool.to_string(),
            delegated_to_mcp,
            ok,
            duration_ms,
        });
        while self.recent.len() > RECENT_CAP {
            self.recent.pop_front();
        }
    }

    pub fn snapshot(&self) -> ToolTelemetrySnapshot {
        ToolTelemetrySnapshot {
            total_calls: self.total_calls,
            counts_by_tool: self.counts_by_tool.clone(),
            recent: self.recent.iter().cloned().collect(),
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
