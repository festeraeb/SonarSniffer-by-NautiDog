//! Chat / cluster routing — resolves live endpoints from mode_state + cluster_config.

use crate::model_scorecard::{self, TaskType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use tracing::{info, warn};

/// Legacy constants (T440 deploy path). Prefer `cfg_path()` / `routing_state_path()`.
pub const CFG_PATH: &str = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
pub const MODE_STATE_PATH: &str = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/mode_state.json";
pub const ROUTING_STATE_PATH: &str = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/routing_state.json";

/// Forge v2 root (env `FORGE_V2_DIR`, then NFS mount, then /codebase).
pub fn forge_v2_dir() -> String {
    if let Ok(d) = std::env::var("FORGE_V2_DIR") {
        if !d.is_empty() {
            return d;
        }
    }
    for p in [
        "/mnt/t440/codebase/repos/wreckhunter2000-1/cesarops-forge-v2",
        "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2",
    ] {
        if std::path::Path::new(p).exists() {
            return p.to_string();
        }
    }
    ROUTING_STATE_PATH
        .trim_end_matches("/routing_state.json")
        .to_string()
}

pub fn cfg_path() -> String {
    std::env::var("FORGE_CLUSTER_CONFIG")
        .unwrap_or_else(|_| format!("{}/cluster_config.toml", forge_v2_dir()))
}

pub fn mode_state_path() -> String {
    std::env::var("FORGE_MODE_STATE")
        .unwrap_or_else(|_| format!("{}/mode_state.json", forge_v2_dir()))
}

pub fn routing_state_path() -> String {
    std::env::var("FORGE_ROUTING_STATE")
        .unwrap_or_else(|_| format!("{}/routing_state.json", forge_v2_dir()))
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RoutingState {
    /// Agent name from [[agent]] table (e.g. gemma, qwen-moe)
    pub chat_agent: String,
    /// Optional override; empty = use agent endpoint
    pub coder_endpoint: String,
    pub reviewer_endpoint: String,
    pub thinker_endpoint: String,
    pub corrector_endpoint: String,
    pub draft_endpoint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentDef {
    pub name: String,
    pub endpoint: String,
    pub hardware: String,
    pub role: String,
    pub model: String,
    #[serde(default)]
    pub template: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoutingPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub chat_agent: String,
    pub coder_endpoint: String,
    #[serde(default)]
    pub reviewer_endpoint: String,
    #[serde(default)]
    pub thinker_endpoint: String,
    #[serde(default)]
    pub corrector_endpoint: String,
    #[serde(default)]
    pub draft_endpoint: String,
    #[serde(default)]
    pub workers_start: Vec<String>,
    #[serde(default)]
    pub workers_stop: Vec<String>,
    /// When set, Forge POSTs fleet-ops to n8n (or NFS queue) for remote nodes.
    #[serde(default)]
    pub fleet_node: String,
    #[serde(default)]
    pub fleet_action: String,
    /// Baseline id for PAMP / golden tests (interactive_fast, pamp_moe_test, …).
    #[serde(default)]
    pub baseline_id: String,
    /// Fire coder + reviewer in parallel; thinker on :5200 picks the better draft.
    #[serde(default)]
    pub parallel_dual_grade: bool,
    /// Forge tool-loop rounds that run parallel grade (default [1, 3] when empty).
    #[serde(default)]
    pub parallel_dual_grade_rounds: Vec<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct ClusterRouting {
    pub agents: Vec<AgentDef>,
    pub presets: Vec<RoutingPreset>,
    pub nicknames: HashMap<String, String>,
    pub workers: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedEndpoints {
    pub coder_url: String,
    pub reviewer_url: String,
    pub thinker_url: String,
    pub corrector_url: String,
    pub validator_url: String,
    pub chat_agent: String,
    pub chat_template: String,
    pub chat_model: String,
    pub parallel_dual_grade: bool,
    pub parallel_dual_grade_rounds: Vec<u32>,
}

pub fn load_routing_state() -> RoutingState {
    let path = routing_state_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_routing_state(state: &RoutingState) {
    let path = routing_state_path();
    let _ = std::fs::write(path, serde_json::to_string_pretty(state).unwrap_or_default());
}

pub fn load_cluster_routing() -> ClusterRouting {
    let content = std::fs::read_to_string(cfg_path()).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();

    let agents: Vec<AgentDef> = table
        .get("agent")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    let t = a.as_table()?;
                    Some(AgentDef {
                        name: t.get("name")?.as_str()?.to_string(),
                        endpoint: t.get("endpoint")?.as_str()?.to_string(),
                        hardware: t.get("hardware").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        role: t.get("role").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        model: t.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        template: template_for_agent(t),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let presets: Vec<RoutingPreset> = table
        .get("routing_preset")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    let t = p.as_table()?;
                    Some(RoutingPreset {
                        id: t.get("id")?.as_str()?.to_string(),
                        name: t.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        description: t
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        chat_agent: t.get("chat_agent").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        coder_endpoint: t.get("coder_endpoint").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        reviewer_endpoint: t
                            .get("reviewer_endpoint")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        thinker_endpoint: t
                            .get("thinker_endpoint")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        corrector_endpoint: t
                            .get("corrector_endpoint")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        draft_endpoint: t
                            .get("draft_endpoint")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        workers_start: t
                            .get("workers_start")
                            .and_then(|v| v.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        workers_stop: t
                            .get("workers_stop")
                            .and_then(|v| v.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        fleet_node: t
                            .get("fleet_node")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        fleet_action: t
                            .get("fleet_action")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        baseline_id: t
                            .get("baseline_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        parallel_dual_grade: t
                            .get("parallel_dual_grade")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        parallel_dual_grade_rounds: t
                            .get("parallel_dual_grade_rounds")
                            .and_then(|v| v.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_integer().map(|n| n as u32))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let mut nicknames = HashMap::new();
    if let Some(n) = table.get("nicknames").and_then(|v| v.as_table()) {
        for (k, v) in n {
            if let Some(s) = v.as_str() {
                nicknames.insert(k.clone(), s.to_string());
            }
        }
    }

    let workers: Vec<serde_json::Value> = table
        .get("worker")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(|w| toml_to_json(w)).collect())
        .unwrap_or_default();

    ClusterRouting {
        agents,
        presets,
        nicknames,
        workers,
    }
}

fn template_for_agent(t: &toml::Table) -> String {
    if let Some(tp) = t.get("template").and_then(|v| v.as_str()) {
        return tp.to_string();
    }
    let model = t.get("model").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
    if model.contains("gemma") {
        "gemma".to_string()
    } else if model.contains("deepseek") || model.contains("r1") {
        "deepseek-r1".to_string()
    } else if model.contains("tinyllama") || model.contains("phi") {
        "llama3".to_string()
    } else {
        "qwen2.5".to_string()
    }
}

fn toml_to_json(v: &toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::json!(s),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::json!(b),
        toml::Value::Array(a) => {
            serde_json::json!(a.iter().map(toml_to_json).collect::<Vec<_>>())
        }
        toml::Value::Table(t) => {
            let mut m = serde_json::Map::new();
            for (k, val) in t {
                m.insert(k.clone(), toml_to_json(val));
            }
            serde_json::Value::Object(m)
        }
        toml::Value::Datetime(d) => serde_json::json!(d.to_string()),
    }
}

pub fn agent_by_name<'a>(cluster: &'a ClusterRouting, name: &str) -> Option<&'a AgentDef> {
    cluster.agents.iter().find(|a| a.name == name)
}

pub fn template_for_endpoint(cluster: &ClusterRouting, endpoint: &str) -> String {
    cluster
        .agents
        .iter()
        .find(|a| a.endpoint == endpoint)
        .map(|a| a.template.clone())
        .unwrap_or_else(|| {
            if endpoint.contains("5571") || endpoint.contains("5200") {
                "llama3".to_string()
            } else {
                "qwen2.5".to_string()
            }
        })
}

/// Merge mode_state.json extras, routing_state.json, cluster [[agent]], [nicknames].
pub fn resolve_endpoints() -> ResolvedEndpoints {
    let cluster = load_cluster_routing();
    let routing = load_routing_state();
    let role_eps = load_role_endpoints();

    let mode_extras: serde_json::Value = std::fs::read_to_string(mode_state_path())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("extras").cloned())
        .unwrap_or_else(|| serde_json::json!({}));

    let chat_agent = if !routing.chat_agent.is_empty() {
        routing.chat_agent.clone()
    } else {
        mode_extras
            .get("chat_agent")
            .and_then(|v| v.as_str())
            .unwrap_or("gemma")
            .to_string()
    };

    let agent = agent_by_name(&cluster, &chat_agent);

    let mut coder_url = routing.coder_endpoint.clone();
    if coder_url.is_empty() {
        coder_url = mode_extras
            .get("coder_endpoint")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| {
                agent
                    .map(|a| a.endpoint.clone())
                    .unwrap_or_else(|| {
                        cluster
                            .nicknames
                            .get("coder")
                            .cloned()
                            .unwrap_or_else(|| "http://127.0.0.1:5001".to_string())
                    })
            });
    }
    let coder_cands = role_candidates("coder", &role_eps, &routing, &mode_extras);
    coder_url = pick_available_endpoint(&coder_cands, &coder_url);

    let thinker_seed = pick_url(
        &routing.thinker_endpoint,
        mode_extras.get("thinker_endpoint"),
        cluster.nicknames.get("thinker"),
        "http://10.0.0.201:5200",
    );
    let thinker_cands = role_candidates("thinker", &role_eps, &routing, &mode_extras);
    let thinker_url = pick_available_endpoint(&thinker_cands, &thinker_seed);

    let corrector_seed = pick_url(
        &routing.corrector_endpoint,
        mode_extras.get("corrector_endpoint"),
        cluster.nicknames.get("corrector"),
        "http://127.0.0.1:5002",
    );
    let corrector_cands = role_candidates("corrector", &role_eps, &routing, &mode_extras);
    let corrector_url = pick_available_endpoint(&corrector_cands, &corrector_seed);

    let validator_seed = pick_url(
        "",
        mode_extras.get("draft_endpoint"),
        cluster.nicknames.get("draft"),
        "http://10.0.0.201:5202",
    );
    let validator_cands = role_candidates("validator", &role_eps, &routing, &mode_extras);
    let validator_url = pick_available_endpoint(&validator_cands, &validator_seed);

    let reviewer_seed = pick_url(
        &routing.reviewer_endpoint,
        mode_extras.get("reviewer_endpoint"),
        cluster.nicknames.get("mtp_reviewer"),
        "http://10.0.0.201:5202",
    );
    let reviewer_cands = role_candidates("reviewer", &role_eps, &routing, &mode_extras);
    let reviewer_url = pick_available_endpoint(&reviewer_cands, &reviewer_seed);

    let (parallel_dual_grade, parallel_dual_grade_rounds) =
        active_preset_parallel_dual(&cluster);

    // Main /send loop always talks to coder_url — prompt template must match the coder, not chat_agent.
    let chat_template = template_for_endpoint(&cluster, &coder_url);
    let coder_agent = cluster.agents.iter().find(|a| a.endpoint == coder_url);
    let chat_model = coder_agent
        .map(|a| a.model.clone())
        .or_else(|| agent.map(|a| a.model.clone()))
        .unwrap_or_else(|| "unknown".to_string());

    if let Some(agent) = agent {
        let agent_tpl = &agent.template;
        if agent_tpl != &chat_template && agent.endpoint != coder_url {
            warn!(
                "chat_agent={} uses template {} but coder {} uses {} — formatting prompts for coder",
                chat_agent, agent_tpl, coder_url, chat_template
            );
        }
    }

    info!(
        "Routing resolved: chat_agent={} coder={} reviewer={} thinker={} corrector={} parallel_dual_grade={} template={} model={}",
        chat_agent, coder_url, reviewer_url, thinker_url, corrector_url, parallel_dual_grade, chat_template, chat_model
    );

    ResolvedEndpoints {
        coder_url,
        reviewer_url,
        thinker_url,
        corrector_url,
        validator_url,
        chat_agent,
        chat_template,
        chat_model,
        parallel_dual_grade,
        parallel_dual_grade_rounds,
    }
}

/// Active preset parallel-grade flag and rounds (default rounds `[1, 3]` when list empty).
fn active_preset_parallel_dual(cluster: &ClusterRouting) -> (bool, Vec<u32>) {
    let preset_id = std::fs::read_to_string(mode_state_path())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("extras")
                .and_then(|e| e.get("routing_preset"))
                .and_then(|p| p.as_str())
                .map(String::from)
        })
        .unwrap_or_default();
    if preset_id.is_empty() {
        return (false, vec![1, 3]);
    }
    cluster
        .presets
        .iter()
        .find(|p| p.id == preset_id)
        .map(|p| {
            let rounds = if p.parallel_dual_grade_rounds.is_empty() {
                vec![1, 3]
            } else {
                p.parallel_dual_grade_rounds.clone()
            };
            (p.parallel_dual_grade, rounds)
        })
        .unwrap_or((false, vec![1, 3]))
}

pub fn default_parallel_dual_rounds() -> Vec<u32> {
    vec![1, 3]
}

fn pick_url(
    override_str: &str,
    mode_field: Option<&serde_json::Value>,
    nickname: Option<&String>,
    default: &str,
) -> String {
    if !override_str.is_empty() {
        return override_str.to_string();
    }
    if let Some(s) = mode_field.and_then(|v| v.as_str()) {
        if !s.is_empty() {
            return s.to_string();
        }
    }
    nickname
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

fn push_unique_candidate(out: &mut Vec<String>, u: &str) {
    let s = u.trim().trim_end_matches('/');
    if s.is_empty() {
        return;
    }
    if !out.iter().any(|x| x == s) {
        out.push(s.to_string());
    }
}

fn endpoint_host_port(endpoint: &str) -> Option<(String, u16)> {
    let e = endpoint.trim();
    let no_scheme = e
        .strip_prefix("http://")
        .or_else(|| e.strip_prefix("https://"))
        .unwrap_or(e);
    let host_port = no_scheme.split('/').next().unwrap_or(no_scheme);
    let idx = host_port.rfind(':')?;
    let host = host_port[..idx].to_string();
    let port: u16 = host_port[idx + 1..].parse().ok()?;
    Some((host, port))
}

fn endpoint_port_open(endpoint: &str) -> bool {
    let Some((host, port)) = endpoint_host_port(endpoint) else {
        return false;
    };
    let addr = format!("{}:{}", host, port);
    let Ok(addrs) = addr.to_socket_addrs() else {
        return false;
    };
    let timeout = std::time::Duration::from_millis(850);
    for a in addrs {
        if std::net::TcpStream::connect_timeout(&a, timeout).is_ok() {
            return true;
        }
    }
    false
}

fn pick_available_endpoint(candidates: &[String], fallback: &str) -> String {
    for c in candidates {
        if endpoint_port_open(c) {
            return c.clone();
        }
    }
    fallback.trim_end_matches('/').to_string()
}

fn role_candidates(
    role: &str,
    role_eps: &HashMap<String, String>,
    routing: &RoutingState,
    mode_extras: &serde_json::Value,
) -> Vec<String> {
    let mut cands = Vec::new();
    match role {
        "coder" | "coding" | "general" => {
            if let Some(u) = mode_extras.get("coder_endpoint").and_then(|v| v.as_str()) {
                push_unique_candidate(&mut cands, u);
            }
            push_unique_candidate(&mut cands, &routing.coder_endpoint);
            for key in ["coding", "coder", "general"] {
                if let Some(u) = role_eps.get(key) {
                    push_unique_candidate(&mut cands, u);
                }
            }
            // P106 (:5201) acts as a fast coding fallback.
            for u in [
                "http://127.0.0.1:5001",
                "http://10.0.0.201:5201",
                "http://10.0.0.201:5200",
                "http://127.0.0.1:5002",
            ] {
                push_unique_candidate(&mut cands, u);
            }
        }
        "thinker" => {
            if let Some(u) = mode_extras.get("thinker_endpoint").and_then(|v| v.as_str()) {
                push_unique_candidate(&mut cands, u);
            }
            push_unique_candidate(&mut cands, &routing.thinker_endpoint);
            for key in ["thinker", "thinker_fast", "reviewer_moe"] {
                if let Some(u) = role_eps.get(key) {
                    push_unique_candidate(&mut cands, u);
                }
            }
            for u in [
                "http://10.0.0.201:5200",
                "http://10.0.0.201:5201",
                "http://127.0.0.1:5002",
            ] {
                push_unique_candidate(&mut cands, u);
            }
        }
        "reviewer" | "corrector" | "validator" => {
            if let Some(u) = mode_extras.get("reviewer_endpoint").and_then(|v| v.as_str()) {
                push_unique_candidate(&mut cands, u);
            }
            if let Some(u) = mode_extras.get("draft_endpoint").and_then(|v| v.as_str()) {
                push_unique_candidate(&mut cands, u);
            }
            push_unique_candidate(&mut cands, &routing.reviewer_endpoint);
            push_unique_candidate(&mut cands, &routing.corrector_endpoint);
            push_unique_candidate(&mut cands, &routing.draft_endpoint);
            for key in ["reviewer", "corrector", "validator", "validator_phi"] {
                if let Some(u) = role_eps.get(key) {
                    push_unique_candidate(&mut cands, u);
                }
            }
            for u in [
                "http://10.0.0.201:5202",
                "http://10.0.0.201:5201",
                "http://10.0.0.201:5571",
                "http://127.0.0.1:5002",
                "http://127.0.0.1:5001",
            ] {
                push_unique_candidate(&mut cands, u);
            }
        }
        "bootstrap" => {
            if let Some(u) = mode_extras.get("bootstrap_endpoint").and_then(|v| v.as_str()) {
                push_unique_candidate(&mut cands, u);
            }
            for key in ["bootstrap", "thinker_fast", "thinker", "coding", "coder", "general"] {
                if let Some(u) = role_eps.get(key) {
                    push_unique_candidate(&mut cands, u);
                }
            }
            for u in [
                "http://10.0.0.201:5201",
                "http://10.0.0.201:5200",
                "http://10.0.0.201:5202",
                "http://127.0.0.1:5001",
                "http://127.0.0.1:5002",
            ] {
                push_unique_candidate(&mut cands, u);
            }
        }
        _ => {
            if let Some(u) = role_eps.get(role) {
                push_unique_candidate(&mut cands, u);
            }
            for key in ["general", "coder", "thinker"] {
                if let Some(u) = role_eps.get(key) {
                    push_unique_candidate(&mut cands, u);
                }
            }
            for u in ["http://127.0.0.1:5001", "http://127.0.0.1:5002"] {
                push_unique_candidate(&mut cands, u);
            }
        }
    }
    cands
}

pub fn apply_preset_to_state(preset_id: &str) -> Result<RoutingPreset, String> {
    let cluster = load_cluster_routing();
    let preset = cluster
        .presets
        .iter()
        .find(|p| p.id == preset_id)
        .cloned()
        .ok_or_else(|| format!("Unknown routing preset: {}", preset_id))?;

    let routing = RoutingState {
        chat_agent: preset.chat_agent.clone(),
        coder_endpoint: preset.coder_endpoint.clone(),
        reviewer_endpoint: preset.reviewer_endpoint.clone(),
        thinker_endpoint: preset.thinker_endpoint.clone(),
        corrector_endpoint: preset.corrector_endpoint.clone(),
        draft_endpoint: preset.draft_endpoint.clone(),
    };
    save_routing_state(&routing);

    // Sync mode_state for UI /mode display
    let mode_payload = serde_json::json!({
        "mode": "coding",
        "activated_at": chrono_unix(),
        "extras": {
            "chat_agent": preset.chat_agent,
            "coder_endpoint": preset.coder_endpoint,
            "reviewer_endpoint": preset.reviewer_endpoint,
            "thinker_endpoint": preset.thinker_endpoint,
            "corrector_endpoint": preset.corrector_endpoint,
            "draft_endpoint": preset.draft_endpoint,
            "routing_preset": preset.id,
            "parallel_dual_grade": preset.parallel_dual_grade,
        }
    });
    let _ = std::fs::write(
        mode_state_path(),
        serde_json::to_string_pretty(&mode_payload).unwrap_or_default(),
    );

    Ok(preset)
}

/// Cake fleet API (:8081); P100#1 contributes VRAM via `cake worker`, not a second HTTP port.
pub const DEFAULT_CAKE_POOL: &str = "http://127.0.0.1:8081";

/// Default MTP-capable endpoints: Cake :8081 first, then cesarops2, then T440 coder (:5001).
pub const DEFAULT_MTP_POOL: &str =
    "http://127.0.0.1:8081,http://10.0.0.201:5202,http://10.0.0.200:5202,http://10.0.0.201:5201,http://10.0.0.200:5201,http://10.0.0.201:5200,http://10.0.0.200:5200,http://127.0.0.1:5001";

/// Load a comma-separated URL list from `[endpoint_pool.<name>]` in cluster_config.toml.
pub fn load_endpoint_pool(pool_name: &str, defaults: &str) -> Vec<String> {
    let content = std::fs::read_to_string(cfg_path()).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    if let Some(pool) = table.get("endpoint_pool").and_then(|v| v.as_table()) {
        if let Some(entry) = pool.get(pool_name).and_then(|v| v.as_table()) {
            if let Some(urls) = entry.get("urls").and_then(|v| v.as_array()) {
                let parsed: Vec<String> = urls
                    .iter()
                    .filter_map(|u| u.as_str().map(String::from))
                    .collect();
                if !parsed.is_empty() {
                    return parsed;
                }
            }
        }
    }
    defaults
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn load_mtp_pool() -> Vec<String> {
    load_endpoint_pool("mtp", DEFAULT_MTP_POOL)
}

pub fn load_cake_pool() -> Vec<String> {
    load_endpoint_pool("cake", DEFAULT_CAKE_POOL)
}

/// True when llama-server (OpenAI API) responds on `/v1/models`.
pub async fn probe_llama_online(client: &reqwest::Client, base: &str) -> bool {
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        return false;
    }
    let url = format!("{}/v1/models", base);
    match client
        .get(&url)
        .timeout(std::time::Duration::from_secs(4))
        .send()
        .await
    {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

/// First responsive llama-server in the pool (cesarops2 augment before T440).
pub async fn first_online_llama(client: &reqwest::Client, pool: &[String]) -> Option<String> {
    for url in pool {
        let base = url.trim_end_matches('/');
        if probe_llama_online(client, base).await {
            info!("MTP/llama pool hit: {}", base);
            return Some(base.to_string());
        }
    }
    None
}

/// Mode extras from mode_state.json (reviewer_endpoint, draft_endpoint, models, etc.).
pub fn load_mode_extras() -> serde_json::Value {
    std::fs::read_to_string(mode_state_path())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("extras").cloned())
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Reviewer for coding pipeline / HUD: mode_state → routing_state → MTP pool.
pub fn resolve_reviewer_url_sync() -> String {
    let extras = load_mode_extras();
    if let Some(u) = extras.get("reviewer_endpoint").and_then(|v| v.as_str()) {
        if !u.is_empty() {
            return u.to_string();
        }
    }
    let routing = load_routing_state();
    if !routing.reviewer_endpoint.is_empty() {
        return routing.reviewer_endpoint;
    }
    "http://127.0.0.1:5002".to_string()
}

pub fn resolve_draft_url_sync() -> String {
    let extras = load_mode_extras();
    pick_url(
        "",
        extras.get("draft_endpoint"),
        load_cluster_routing().nicknames.get("draft"),
        "http://10.0.0.201:5202",
    )
}

/// Human label for HUD: "Gemma P100#0 :5001" from URL + mode_state model names.
pub fn endpoint_display(url: &str, role: &str) -> serde_json::Value {
    let extras = load_mode_extras();
    let port = url
        .trim_end_matches('/')
        .rsplit(':')
        .next()
        .unwrap_or("?");
    let host = if url.contains("127.0.0.1") || url.contains("10.0.0.61") {
        "T440"
    } else if url.contains("10.0.0.201") {
        "cesarops2"
    } else if url.contains("10.0.0.200") {
        "cesarops2-alt"
    } else {
        "remote"
    };

    let model_key = match role {
        "reviewer" => "reviewer_model",
        "coder" => "coder_model",
        _ => "",
    };
    let model_path = extras.get(model_key).and_then(|v| v.as_str()).unwrap_or("");
    let model_short = model_path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .replace(".gguf", "")
        .chars()
        .take(22)
        .collect::<String>();

    let cluster = load_cluster_routing();
    let agent_name = cluster
        .agents
        .iter()
        .find(|a| a.endpoint.trim_end_matches('/') == url.trim_end_matches('/'))
        .map(|a| a.name.clone());

    let worker_name = cluster.workers.iter().find_map(|w| {
        let p = w.get("port")?.as_i64()?;
        if format!("http://127.0.0.1:{}", p) == url.trim_end_matches('/')
            || url.ends_with(&format!(":{}", p))
        {
            w.get("name").and_then(|n| n.as_str()).map(String::from)
        } else {
            None
        }
    });

    let chip = agent_name
        .or(worker_name)
        .unwrap_or_else(|| model_short.clone());
    let label = if chip.is_empty() {
        format!("{} :{}", role, port)
    } else {
        format!("{} {}:{}", chip, host, port)
    };

    serde_json::json!({
        "role": role,
        "label": label,
        "host": host,
        "port": port,
        "url": url,
        "model": if model_short.is_empty() { serde_json::Value::Null } else { serde_json::json!(model_short) },
    })
}

/// Reviewer URL: routing preset → MTP pool → default cesarops2 :5202.
pub async fn resolve_reviewer_endpoint() -> String {
    let client = reqwest::Client::new();
    let preferred = resolve_reviewer_url_sync();
    if probe_llama_online(&client, &preferred).await {
        return preferred;
    }
    let routing = load_routing_state();
    if !routing.reviewer_endpoint.is_empty()
        && probe_llama_online(&client, &routing.reviewer_endpoint).await
    {
        return routing.reviewer_endpoint.clone();
    }
    first_online_llama(&client, &load_mtp_pool())
        .await
        .unwrap_or(preferred)
}

fn chrono_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Forge 3-lane UI + role registry ─────────────────────────────────────────

pub const LANE_ROLE_CATALOG: &[(&str, &str)] = &[
    ("idle", "idle"),
    ("coder", "coder"),
    ("thinker", "thinker"),
    ("reviewer", "reviewer"),
    ("corrector", "corrector"),
    ("validator", "validator"),
    ("general", "general"),
    ("coding", "coding"),
    ("bootstrap", "bootstrap"),
];

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct LanesState {
    #[serde(default)]
    pub lane_roles: HashMap<String, String>,
}

pub fn lanes_state_path() -> String {
    format!("{}/lanes_state.json", forge_v2_dir())
}

pub fn load_lanes_state() -> LanesState {
    std::fs::read_to_string(lanes_state_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(default_lanes_state)
}

pub fn save_lanes_state(state: &LanesState) {
    let _ = std::fs::write(
        lanes_state_path(),
        serde_json::to_string_pretty(state).unwrap_or_default(),
    );
}

pub fn default_lanes_state() -> LanesState {
    let mut lane_roles = HashMap::new();
    lane_roles.insert("lane-a".into(), "general".into());
    lane_roles.insert("lane-b".into(), "coding".into());
    lane_roles.insert("lane-c".into(), "thinker".into());
    LanesState { lane_roles }
}

pub fn load_role_endpoints() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let content = std::fs::read_to_string(cfg_path()).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();

    if let Some(roles) = table.get("roles").and_then(|v| v.as_table()) {
        for (k, v) in roles {
            if let Some(ep) = v.as_str() {
                map.insert(k.clone(), ep.to_string());
            } else if let Some(t) = v.as_table() {
                if let Some(ep) = t.get("endpoint").and_then(|x| x.as_str()) {
                    map.insert(k.clone(), ep.to_string());
                }
            }
        }
    }
    if let Some(nick) = table.get("nicknames").and_then(|v| v.as_table()) {
        for (k, v) in nick {
            if let Some(ep) = v.as_str() {
                map.entry(k.clone()).or_insert(ep.to_string());
            }
        }
    }
    map
}

pub fn endpoint_for_role(role: &str) -> Option<String> {
    let role_eps = load_role_endpoints();
    let routing = load_routing_state();
    let extras = load_mode_extras();
    let cands = role_candidates(role, &role_eps, &routing, &extras);
    if cands.is_empty() {
        return role_eps.get(role).cloned();
    }
    let fallback = role_eps
        .get(role)
        .cloned()
        .unwrap_or_else(|| "http://127.0.0.1:5001".to_string());
    Some(pick_available_endpoint(&cands, &fallback))
}

fn scorecard_routing_enabled() -> bool {
    std::env::var("FORGE_SCORECARD_ROUTING")
        .map(|v| {
            let v = v.trim().to_lowercase();
            !(v == "0" || v == "false" || v == "no" || v == "off")
        })
        .unwrap_or(true)
}

fn scorecard_routing_scope() -> String {
    std::env::var("FORGE_SCORECARD_ROUTING_SCOPE")
        .ok()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "all".to_string())
}

fn scorecard_applies_for_role(role: &str) -> bool {
    if !scorecard_routing_enabled() {
        return false;
    }
    let scope = scorecard_routing_scope();
    if scope == "coder" {
        return role == "coding" || role == "coder";
    }
    matches!(
        role,
        "coding" | "coder" | "reviewer" | "validator" | "corrector" | "thinker" | "general" | "bootstrap"
    )
}

fn task_for_role(role: &str) -> TaskType {
    match role {
        "coding" | "coder" => TaskType::RustCode,
        "reviewer" | "validator" => TaskType::Analysis,
        "corrector" => TaskType::JsonRepair,
        "thinker" => TaskType::Research,
        "bootstrap" => TaskType::Analysis,
        _ => TaskType::Analysis,
    }
}

fn best_endpoint_by_scorecard(role: &str, candidates: &[String]) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }
    let cluster = load_cluster_routing();
    let scorecard = model_scorecard::load();

    let mut model_to_endpoint: HashMap<String, String> = HashMap::new();
    let mut model_candidates: Vec<String> = Vec::new();

    for ep in candidates {
        if let Some(agent) = cluster
            .agents
            .iter()
            .find(|a| a.endpoint.trim_end_matches('/') == ep.trim_end_matches('/'))
        {
            if !model_to_endpoint.contains_key(&agent.model) {
                model_to_endpoint.insert(agent.model.clone(), ep.clone());
                model_candidates.push(agent.model.clone());
            }
        }
    }

    if model_candidates.is_empty() {
        return None;
    }

    let task = task_for_role(role);
    let (best_model, confidence) = scorecard.pick_best(&model_candidates, task);
    if let Some(endpoint) = model_to_endpoint.get(best_model) {
        if endpoint_port_open(endpoint) {
            info!(
                "scorecard routing role={} task={} model={} conf={:.3} endpoint={}",
                role,
                task.name(),
                best_model,
                confidence,
                endpoint
            );
            return Some(endpoint.clone());
        }
    }
    None
}

pub fn resolve_lane_coder_url(lane_id: &str) -> String {
    let lanes = load_lanes_state();
    let role = lanes
        .lane_roles
        .get(lane_id)
        .cloned()
        .unwrap_or_else(|| "general".to_string());

    if scorecard_applies_for_role(&role) {
        let role_eps = load_role_endpoints();
        let routing = load_routing_state();
        let extras = load_mode_extras();
        let cands = role_candidates(&role, &role_eps, &routing, &extras);
        if let Some(ep) = best_endpoint_by_scorecard(&role, &cands) {
            return ep;
        }
    }

    if let Some(ep) = endpoint_for_role(&role) {
        if !ep.is_empty() {
            return ep;
        }
    }
    let routing = load_routing_state();
    if !routing.coder_endpoint.is_empty() {
        return routing.coder_endpoint;
    }
    load_role_endpoints()
        .get("coder")
        .cloned()
        .unwrap_or_else(|| "http://127.0.0.1:5001".to_string())
}

pub fn set_lane_role(lane_id: &str, role: &str) {
    let mut lanes = load_lanes_state();
    lanes.lane_roles.insert(lane_id.to_string(), role.to_string());
    save_lanes_state(&lanes);
    info!("Lane {} → role {}", lane_id, role);
}

pub fn netdata_url() -> Option<String> {
    let content = std::fs::read_to_string(cfg_path()).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    table
        .get("monitoring")
        .and_then(|m| m.get("netdata_url"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

pub fn netdata_cesarops2_url() -> Option<String> {
    let content = std::fs::read_to_string(cfg_path()).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    table
        .get("monitoring")
        .and_then(|m| m.get("netdata_cesarops2_url"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Netdata on the Tailscale t440-p100 link (when not bound to 127.0.0.1 on Forge host).
pub fn netdata_tailscale_url() -> Option<String> {
    let content = std::fs::read_to_string(cfg_path()).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    table
        .get("monitoring")
        .and_then(|m| m.get("netdata_tailscale_url"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

pub fn netdata_probe_candidates() -> Vec<String> {
    let mut out = Vec::new();
    for u in [
        netdata_url(),
        netdata_tailscale_url(),
        netdata_cesarops2_url(),
    ] {
        if let Some(url) = u {
            if !out.iter().any(|x| x == &url) {
                out.push(url);
            }
        }
    }
    out
}
