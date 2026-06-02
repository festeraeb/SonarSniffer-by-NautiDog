//! Probe fleet LLM endpoints and send commands to one or many loaded models.

use crate::routing;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::info;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoadedEndpoint {
    pub id: String,
    pub url: String,
    pub host: String,
    pub port: u16,
    pub role: String,
    pub agent: Option<String>,
    pub label: String,
    pub online: bool,
    pub models: Vec<String>,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandResult {
    pub id: String,
    pub url: String,
    pub label: String,
    pub online: bool,
    pub ok: bool,
    pub response: Option<String>,
    pub error: Option<String>,
    pub latency_ms: u64,
}

fn host_from_url(url: &str) -> String {
    if url.contains("127.0.0.1") || url.contains("10.0.0.61") {
        "T440".to_string()
    } else if url.contains("10.0.0.201") {
        "cesarops2".to_string()
    } else if url.contains("10.0.0.200") {
        "cesarops2-alt".to_string()
    } else if url.contains("100.") {
        "tailscale".to_string()
    } else {
        "remote".to_string()
    }
}

fn port_from_url(url: &str) -> u16 {
    url.trim_end_matches('/')
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or(0)
}

fn endpoint_id(url: &str, role: &str) -> String {
    format!("{}:{}", host_from_url(url), port_from_url(url))
}

/// Collect probe targets from agents, fleet roles, workers, and known_nodes.
pub fn probe_candidates() -> Vec<(String, String, String, Option<String>)> {
    let mut out: Vec<(String, String, String, Option<String>)> = Vec::new();
    let mut seen = HashMap::new();

    let mut push = |url: &str, role: &str, agent: Option<&str>, source: &str| {
        let u = url.trim().trim_end_matches('/').to_string();
        if u.is_empty() || seen.contains_key(&u) {
            return;
        }
        seen.insert(u.clone(), ());
        out.push((
            u,
            role.to_string(),
            source.to_string(),
            agent.map(String::from),
        ));
    };

    let cluster = routing::load_cluster_routing();
    for a in &cluster.agents {
        push(&a.endpoint, &a.role, Some(&a.name), "agent");
    }

    let extras = routing::load_mode_extras();
    let coder = extras
        .get("coder_endpoint")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| "http://127.0.0.1:5001".to_string());
    let reviewer = routing::resolve_reviewer_url_sync();
    let thinker = extras
        .get("thinker_endpoint")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| "http://10.0.0.201:5200".to_string());
    let draft = routing::resolve_draft_url_sync();

    push(&coder, "coder", None, "fleet_role");
    push(&reviewer, "reviewer", None, "fleet_role");
    push(&thinker, "thinker", None, "fleet_role");
    push(&draft, "draft", None, "fleet_role");

    let cfg = std::fs::read_to_string(routing::cfg_path()).unwrap_or_default();
    let table: toml::Table = cfg.parse().unwrap_or_default();

    if let Some(workers) = table.get("worker").and_then(|v| v.as_array()) {
        for w in workers {
            let enabled = w.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            if !enabled {
                continue;
            }
            let port = w.get("port").and_then(|v| v.as_integer()).unwrap_or(0);
            if port <= 0 {
                continue;
            }
            let name = w.get("name").and_then(|v| v.as_str()).unwrap_or("worker");
            let role = w.get("role").and_then(|v| v.as_str()).unwrap_or("worker");
            push(
                &format!("http://127.0.0.1:{}", port),
                role,
                Some(name),
                "worker",
            );
        }
    }

    if let Some(nodes) = table.get("known_nodes").and_then(|v| v.as_array()) {
        for node in nodes {
            let ip = node.get("ip").and_then(|v| v.as_str()).unwrap_or("");
            if ip.is_empty() {
                continue;
            }
            let name = node.get("name").and_then(|v| v.as_str()).unwrap_or("node");
            if let Some(ports) = node.get("ports").and_then(|v| v.as_array()) {
                for p in ports {
                    let port = p.as_integer().unwrap_or(0);
                    if port < 5000 || port > 6000 {
                        continue;
                    }
                    push(
                        &format!("http://{}:{}", ip, port),
                        "llm",
                        Some(name),
                        "known_node",
                    );
                }
            }
        }
    }

    out
}

async fn fetch_models_at(client: &reqwest::Client, base: &str) -> (bool, Vec<String>) {
    let url = format!("{}/v1/models", base.trim_end_matches('/'));
    let Ok(resp) = client.get(&url).send().await else {
        return (false, vec![]);
    };
    if !resp.status().is_success() {
        return (false, vec![]);
    }
    let Ok(body) = resp.json::<serde_json::Value>().await else {
        return (true, vec![]);
    };
    let mut names = Vec::new();
    let arr = body
        .get("data")
        .or_else(|| body.get("models"))
        .and_then(|v| v.as_array());
    if let Some(items) = arr {
        for item in items {
            let id = item
                .get("id")
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !id.is_empty() {
                names.push(id.to_string());
            }
        }
    }
    (true, names)
}

/// GET /cluster/models/loaded — every probed LLM port (T440 + cesarops2 + known_nodes).
pub async fn list_loaded_endpoints() -> serde_json::Value {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let candidates = probe_candidates();
    let mut endpoints: Vec<LoadedEndpoint> = Vec::new();

    for (url, role, source, agent) in candidates {
        let (online, models) = fetch_models_at(&client, &url).await;
        let display = routing::endpoint_display(&url, &role);
        let label = display
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or(&url)
            .to_string();
        let host = host_from_url(&url);
        let port = port_from_url(&url);
        let id = endpoint_id(&url, &role);
        endpoints.push(LoadedEndpoint {
            id: id.clone(),
            url: url.clone(),
            host,
            port,
            role,
            agent,
            label,
            online,
            models,
            source,
        });
    }

    endpoints.sort_by(|a, b| {
        (a.host.clone(), a.port)
            .cmp(&(b.host.clone(), b.port))
            .then_with(|| b.online.cmp(&a.online))
    });

    let online_count = endpoints.iter().filter(|e| e.online).count();
    serde_json::json!({
        "endpoints": endpoints,
        "online_count": online_count,
        "total_probed": endpoints.len(),
    })
}

fn push_ep(selected: &mut Vec<LoadedEndpoint>, seen: &mut HashMap<String, ()>, ep: LoadedEndpoint) {
    let key = ep.url.trim_end_matches('/').to_string();
    if seen.contains_key(&key) {
        return;
    }
    seen.insert(key, ());
    selected.push(ep);
}

fn push_url_str(
    selected: &mut Vec<LoadedEndpoint>,
    seen: &mut HashMap<String, ()>,
    catalog: &[LoadedEndpoint],
    url: &str,
) {
    let u = url.trim().trim_end_matches('/');
    if u.is_empty() || seen.contains_key(u) {
        return;
    }
    if let Some(ep) = catalog.iter().find(|e| e.url.trim_end_matches('/') == u) {
        push_ep(selected, seen, ep.clone());
    } else if u.starts_with("http") {
        push_ep(
            selected,
            seen,
            LoadedEndpoint {
                id: endpoint_id(u, "custom"),
                url: u.to_string(),
                host: host_from_url(u),
                port: port_from_url(u),
                role: "custom".to_string(),
                agent: None,
                label: format!("custom {}", port_from_url(u)),
                online: false,
                models: vec![],
                source: "url".to_string(),
            },
        );
    }
}

fn resolve_target_urls(body: &serde_json::Value, catalog: &[LoadedEndpoint]) -> Vec<LoadedEndpoint> {
    let mut selected: Vec<LoadedEndpoint> = Vec::new();
    let mut seen = HashMap::new();

    if let Some(ids) = body.get("ids").and_then(|v| v.as_array()) {
        for id in ids {
            if let Some(s) = id.as_str() {
                if let Some(ep) = catalog.iter().find(|e| e.id == s) {
                    push_ep(&mut selected, &mut seen, ep.clone());
                }
            }
        }
    }

    if let Some(targets) = body.get("targets").and_then(|v| v.as_array()) {
        for t in targets {
            let s = t.as_str().unwrap_or("").to_lowercase();
            if s == "all" {
                for ep in catalog.iter().filter(|e| e.online) {
                    push_ep(&mut selected, &mut seen, ep.clone());
                }
                continue;
            }
            if s.starts_with("http") {
                push_url_str(&mut selected, &mut seen, catalog, &s);
                continue;
            }
            for ep in catalog.iter() {
                if ep.role.to_lowercase() == s
                    || ep.agent.as_deref().unwrap_or("").to_lowercase() == s
                    || ep.id.to_lowercase() == s
                {
                    push_ep(&mut selected, &mut seen, ep.clone());
                }
            }
        }
    }

    if let Some(urls) = body.get("urls").and_then(|v| v.as_array()) {
        for u in urls {
            if let Some(s) = u.as_str() {
                push_url_str(&mut selected, &mut seen, catalog, s);
            }
        }
    }

    if selected.is_empty() {
        if let Some(t) = body.get("target").and_then(|v| v.as_str()) {
            let fake = serde_json::json!({ "targets": [t] });
            return resolve_target_urls(&fake, catalog);
        }
    }

    selected
}

/// POST /cluster/command — send a prompt to one or many endpoints in parallel.
pub async fn send_command(body: serde_json::Value) -> serde_json::Value {
    let message = body
        .get("message")
        .or_else(|| body.get("command"))
        .or_else(|| body.get("prompt"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if message.is_empty() {
        return serde_json::json!({ "error": "message (or command/prompt) required" });
    }

    let max_tokens = body
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(128) as u32;
    let temperature = body
        .get("temperature")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.2) as f32;
    let parallel = body.get("parallel").and_then(|v| v.as_bool()).unwrap_or(true);

    let catalog_val = list_loaded_endpoints().await;
    let catalog: Vec<LoadedEndpoint> =
        serde_json::from_value(catalog_val.get("endpoints").cloned().unwrap_or_default())
            .unwrap_or_default();

    let targets = resolve_target_urls(&body, &catalog);
    if targets.is_empty() {
        return serde_json::json!({
            "error": "no targets — use targets: [\"all\"|\"coder\"|\"reviewer\"|...], ids: [\"cesarops2:5200\"], or urls: [\"http://...\"]"
        });
    }

    info!(
        "cluster/command → {} target(s), parallel={}",
        targets.len(),
        parallel
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let run_one = |ep: LoadedEndpoint| {
        let client = client.clone();
        let message = message.to_string();
        async move {
            let start = std::time::Instant::now();
            let (online, _) = fetch_models_at(&client, &ep.url).await;
            if !online {
                return CommandResult {
                    id: ep.id,
                    url: ep.url,
                    label: ep.label,
                    online: false,
                    ok: false,
                    response: None,
                    error: Some("endpoint offline".to_string()),
                    latency_ms: start.elapsed().as_millis() as u64,
                };
            }
            let prompt = format!(
                "<|im_start|>user\n{}\n\n<|im_start|>assistant\n",
                message
            );
            match crate::inference_client::complete_prompt(
                &client,
                &ep.url,
                &prompt,
                max_tokens,
                temperature,
                vec!["".to_string(), "</s>".to_string()],
                None,
            )
            .await
            {
                Ok(text) => CommandResult {
                    id: ep.id,
                    url: ep.url,
                    label: ep.label,
                    online: true,
                    ok: true,
                    response: Some(text.trim().to_string()),
                    error: None,
                    latency_ms: start.elapsed().as_millis() as u64,
                },
                Err(e) => CommandResult {
                    id: ep.id,
                    url: ep.url,
                    label: ep.label,
                    online: true,
                    ok: false,
                    response: None,
                    error: Some(e),
                    latency_ms: start.elapsed().as_millis() as u64,
                },
            }
        }
    };

    let results: Vec<CommandResult> = if parallel {
        let futs: Vec<_> = targets.into_iter().map(run_one).collect();
        futures_util::future::join_all(futs).await
    } else {
        let mut out = Vec::new();
        for ep in targets {
            out.push(run_one(ep).await);
        }
        out
    };

    let ok_count = results.iter().filter(|r| r.ok).count();
    serde_json::json!({
        "ok": ok_count > 0,
        "ok_count": ok_count,
        "total": results.len(),
        "results": results,
    })
}

/// Resolve a single fleet target to URL + label (for sub-agent tools).
pub async fn resolve_fleet_target(target: &str) -> Option<LoadedEndpoint> {
    let t = target.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    let catalog_val = list_loaded_endpoints().await;
    let catalog: Vec<LoadedEndpoint> =
        serde_json::from_value(catalog_val.get("endpoints").cloned().unwrap_or_default())
            .unwrap_or_default();

    if t.starts_with("http") {
        let body = serde_json::json!({ "targets": [target.trim()] });
        return resolve_target_urls(&body, &catalog).into_iter().next();
    }

    let body = serde_json::json!({ "targets": [t] });
    let picked = resolve_target_urls(&body, &catalog);
    picked
        .iter()
        .find(|e| e.online)
        .cloned()
        .or_else(|| picked.into_iter().next())
}

/// Tool: one-shot message to a fleet sub-agent (coder, thinker, reviewer, draft, agent name, or URL).
pub async fn tool_call_sub_agent(args: &serde_json::Value) -> String {
    let message = args
        .get("message")
        .or_else(|| args.get("task"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if message.is_empty() {
        return "Error: message or task required".to_string();
    }
    let target = args
        .get("target")
        .or_else(|| args.get("role"))
        .or_else(|| args.get("agent"))
        .and_then(|v| v.as_str())
        .unwrap_or("thinker");

    let max_tokens = args.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(512) as u32;

    let body = serde_json::json!({
        "message": message,
        "targets": [target],
        "max_tokens": max_tokens,
        "temperature": args.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.3),
        "parallel": false,
    });
    let out = send_command(body).await;
    if let Some(err) = out.get("error").and_then(|v| v.as_str()) {
        return format!("call_sub_agent failed: {}", err);
    }
    let results = out.get("results").and_then(|v| v.as_array());
    match results.and_then(|a| a.first()) {
        Some(r) => {
            let label = r.get("label").and_then(|v| v.as_str()).unwrap_or(target);
            if r.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                format!(
                    "[sub-agent {}]\n{}",
                    label,
                    r.get("response").and_then(|v| v.as_str()).unwrap_or("")
                )
            } else {
                format!(
                    "[sub-agent {} FAILED]\n{}",
                    label,
                    r.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error")
                )
            }
        }
        None => "call_sub_agent: no result".to_string(),
    }
}

/// Tool: list online fleet LLM endpoints (for picking sub-agents).
pub async fn tool_list_fleet_agents() -> String {
    let val = list_loaded_endpoints().await;
    serde_json::to_string_pretty(&val).unwrap_or_else(|_| "{}".to_string())
}

/// Tool: run full agent loop on a remote fleet endpoint (sub-agent with tools).
pub async fn tool_delegate_sub_agent(
    args: &serde_json::Value,
    project_root: &str,
    mcp_worker_url: Option<String>,
    fleet_delegate_depth: u32,
) -> String {
    let task = args
        .get("task")
        .or_else(|| args.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if task.is_empty() {
        return "Error: task required".to_string();
    }
    let target = args
        .get("target")
        .or_else(|| args.get("role"))
        .or_else(|| args.get("agent"))
        .and_then(|v| v.as_str())
        .unwrap_or("thinker");

    let Some(ep) = resolve_fleet_target(target).await else {
        return format!("Error: fleet target '{}' not found or offline", target);
    };
    if !ep.online {
        return format!("Error: {} ({}) is offline", ep.label, ep.url);
    }

    let cluster = routing::load_cluster_routing();
    let template = routing::template_for_endpoint(&cluster, &ep.url);
    let config = crate::agent_dispatch::AgentConfig {
        endpoint_url: ep.url.clone(),
        project_root: project_root.to_string(),
        nautivecs_url: "http://127.0.0.1:5003/query".to_string(),
        wso_url: "http://127.0.0.1:5010/search".to_string(),
        mcp_worker_url,
        max_tokens: args.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(4096) as u32,
        temperature: args.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.35) as f32,
        safe_mode: args.get("safe_mode").and_then(|v| v.as_bool()).unwrap_or(false),
        chat_template: template,
        engine: None,
        fleet_delegate_depth,
    };

    info!(
        "delegate_sub_agent → {} ({}) task={}…",
        ep.label,
        ep.url,
        &task[..task.len().min(60)]
    );
    let result = crate::agent_dispatch::run_sub_agent_loop(&config, task).await;
    format!("[sub-agent {} @ {}]\n{}", ep.label, ep.url, result)
}
