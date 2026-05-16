//! Fleet registry — single source of truth for which models are available,
//! which task types they're suited for, and how to reach them.
//!
//! Used by the conductor to:
//!   - Pick the best model for a given task (combines task suitability + scorecard)
//!   - Hot-swap when a model is underperforming
//!   - Track which tools improve which models the most
//!
//! Updates live: as scorecard data flows in, the registry's `pick_for` results shift.

use serde::{Deserialize, Serialize};

use crate::model_scorecard::{Scorecard, TaskType};

/// One fleet member.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetMember {
    /// Unique nickname (e.g. "qwen-coder-14b", "fortytwo-rust-14b").
    pub name: String,
    /// HTTP endpoint root (e.g. "http://localhost:5001").
    pub endpoint: String,
    /// Hardware host info for logs.
    pub hardware: String,
    /// Model file or HF id loaded by koboldcpp (e.g. "Qwen2.5-Coder-14B-Q6_K").
    pub model_id: String,
    /// Task types this model is *suited* for. Empty = generalist.
    pub specialties: Vec<TaskType>,
    /// Estimated tokens/second for time budgeting.
    pub est_tps: f32,
    /// Online flag (set by health probe).
    #[serde(default)]
    pub online: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FleetRegistry {
    pub members: Vec<FleetMember>,
}

impl FleetRegistry {
    /// Build the default fleet for our cluster setup.
    pub fn default_fleet() -> Self {
        Self {
            members: vec![
                FleetMember {
                    name: "qwen-coder-14b".into(),
                    endpoint: "http://localhost:5001".into(),
                    hardware: "T440 P100#0".into(),
                    model_id: "Qwen2.5-Coder-14B-Instruct-abliterated-Q6_K".into(),
                    specialties: vec![
                        TaskType::RustCode, TaskType::Refactor,
                        TaskType::FrontendUi, TaskType::Analysis,
                    ],
                    est_tps: 18.0,
                    online: false,
                },
                FleetMember {
                    name: "fortytwo-rust-14b".into(),
                    endpoint: "http://localhost:5002".into(),
                    hardware: "T440 P100#1".into(),
                    model_id: "Fortytwo_Strand-Rust-Coder-14B-v1-Q6_K".into(),
                    specialties: vec![
                        TaskType::RustCode, TaskType::WgslShader,
                        TaskType::Refactor,
                    ],
                    est_tps: 18.0,
                    online: false,
                },
                FleetMember {
                    name: "deepseek-coder-v2".into(),
                    endpoint: "http://100.102.158.111:5555".into(),
                    hardware: "cesarops2 1070+P1000".into(),
                    model_id: "DeepSeek-Coder-V2-Lite-Q4_K_M".into(),
                    specialties: vec![
                        TaskType::JsonRepair, TaskType::Translation,
                        TaskType::LoopJudgment, TaskType::RustCode,
                    ],
                    est_tps: 22.0,
                    online: false,
                },
                FleetMember {
                    name: "deepseek-r1-7b".into(),
                    endpoint: "http://100.105.77.74:5100".into(),
                    hardware: "cesarops3 P106-100".into(),
                    model_id: "DeepSeek-R1-Distill-Qwen-7B-Q4_K_M".into(),
                    specialties: vec![
                        TaskType::Research, TaskType::Analysis,
                    ],
                    est_tps: 12.0,
                    online: false,
                },
                FleetMember {
                    name: "tinyllama-validator".into(),
                    endpoint: "http://100.102.158.111:5571".into(),
                    hardware: "cesarops2 P1000".into(),
                    model_id: "TinyLlama-1.1B-Chat-v1.0-Q4_K_M".into(),
                    specialties: vec![],
                    est_tps: 35.0,
                    online: false,
                },
            ],
        }
    }

    /// Pick the best fleet member for a task.
    /// Combines: specialty match + scorecard confidence + online status.
    /// Returns None if no online member exists.
    pub fn pick_for<'a>(
        &'a self,
        task: TaskType,
        card: &Scorecard,
    ) -> Option<&'a FleetMember> {
        // Filter to online specialists first
        let mut candidates: Vec<&FleetMember> = self.members.iter()
            .filter(|m| m.online && m.specialties.contains(&task))
            .collect();

        // If no online specialists, fall back to any online generalist
        if candidates.is_empty() {
            candidates = self.members.iter()
                .filter(|m| m.online && m.specialties.is_empty())
                .collect();
        }
        // Last resort: any online member
        if candidates.is_empty() {
            candidates = self.members.iter().filter(|m| m.online).collect();
        }
        if candidates.is_empty() { return None; }

        // Filter out swap-recommended models (5 recent failures)
        let swap_list = card.swap_recommendations(task);
        let pre_swap: Vec<&FleetMember> = candidates.iter()
            .filter(|m| !swap_list.contains(&m.name))
            .copied()
            .collect();
        let pool = if pre_swap.is_empty() { candidates } else { pre_swap };

        // Pick by scorecard confidence; specialty + scorecard tied → prefer higher est_tps
        let names: Vec<String> = pool.iter().map(|m| m.name.clone()).collect();
        let (best_name, _) = card.pick_best(&names, task);
        pool.into_iter().find(|m| m.name == best_name)
    }

    /// Mark a member online/offline.
    pub fn set_online(&mut self, name: &str, online: bool) {
        if let Some(m) = self.members.iter_mut().find(|m| m.name == name) {
            m.online = online;
        }
    }

    /// Probe each member's /v1/models endpoint to refresh online status.
    /// Caller should run this periodically (every 30s) from a background task.
    pub async fn refresh_online(&mut self) {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap_or_default();

        for m in &mut self.members {
            let url = format!("{}/v1/models", m.endpoint.trim_end_matches('/'));
            m.online = client.get(&url).send().await
                .map(|r| r.status().is_success())
                .unwrap_or(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_specialist_when_online() {
        let mut fleet = FleetRegistry::default_fleet();
        let card = Scorecard::default();

        // Mark only the rust specialist online
        fleet.set_online("fortytwo-rust-14b", true);

        let pick = fleet.pick_for(TaskType::WgslShader, &card);
        assert!(pick.is_some());
        assert_eq!(pick.unwrap().name, "fortytwo-rust-14b");
    }

    #[test]
    fn fall_back_when_no_specialist_online() {
        let mut fleet = FleetRegistry::default_fleet();
        let card = Scorecard::default();

        // Only generic model online (qwen-coder is a RustCode specialist, not WgslShader)
        fleet.set_online("qwen-coder-14b", true);

        let pick = fleet.pick_for(TaskType::WgslShader, &card);
        // Falls back to any online member
        assert!(pick.is_some());
    }

    #[test]
    fn no_pick_when_all_offline() {
        let fleet = FleetRegistry::default_fleet();
        let card = Scorecard::default();
        assert!(fleet.pick_for(TaskType::RustCode, &card).is_none());
    }
}
