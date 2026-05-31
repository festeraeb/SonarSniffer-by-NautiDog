//! n8n orchestration: tool routing and Predictive Async MoE (PAMP).

use crate::routing;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Instant;
use tracing::{info, warn};

pub const CFG_PATH: &str = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";

#[derive(Clone, Debug, Default)]
pub struct OrchestrationConfig {
    pub tools_backend: String,
    pub n8n_tool_url: String,
    pub n8n_pamp_url: String,
    pub n8n_fleet_url: String,
    pub pamp_shadow: bool,
    pub intake_url: String,
    pub default_baseline: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertPlan {
    pub parallel: Vec<String>,
    #[serde(default)]
    pub serial_after: Vec<String>,
    #[serde(default)]
    pub skip: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PampResponse {
    pub expert_plan: ExpertPlan,
    #[serde(default)]
    pub results: Value,
    #[serde(default)]
    pub shadow: bool,
    #[serde(default)]
    pub parallel_ms_saved: Option<u64>,
}

pub fn load_orchestration() -> OrchestrationConfig {
    let content = std::fs::read_to_string(CFG_PATH).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    let o = table.get("orchestration");
    OrchestrationConfig {
        tools_backend: o
            .and_then(|t| t.get("tools_backend"))
            .and_then(|v| v.as_str())
            .unwrap_or("inline")
            .to_string(),
        n8n_tool_url: o
            .and_then(|t| t.get("n8n_tool_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("http://127.0.0.1:5678/webhook/tool-route")
            .to_string(),
        n8n_pamp_url: o
            .and_then(|t| t.get("n8n_pamp_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("http://127.0.0.1:5678/webhook/pamp-route")
            .to_string(),
        n8n_fleet_url: o
            .and_then(|t| t.get("n8n_fleet_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("http://127.0.0.1:5678/webhook/fleet-ops")
            .to_string(),
        pamp_shadow: o
            .and_then(|t| t.get("pamp_shadow"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        intake_url: o
            .and_then(|t| t.get("intake_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("http://10.0.0.201:5599")
            .to_string(),
        default_baseline: o
            .and_then(|t| t.get("default_baseline"))
            .and_then(|v| v.as_str())
            .unwrap_or("interactive_fast")
            .to_string(),
    }
}

pub fn orchestration_json() -> serde_json::Value {
    let o = load_orchestration();
    serde_json::json!({
        "tools_backend": o.tools_backend,
        "n8n_tool_url": o.n8n_tool_url,
        "n8n_pamp_url": o.n8n_pamp_url,
        "n8n_fleet_url": o.n8n_fleet_url,
        "pamp_shadow": o.pamp_shadow,
        "intake_url": o.intake_url,
        "default_baseline": o.default_baseline,
    })
}

/// Persist [orchestration] keys in cluster_config.toml (partial update).
pub fn save_orchestration(partial: &serde_json::Value) -> Result<(), String> {
    use toml_edit::{DocumentMut, Item, value};
    let content = std::fs::read_to_string(CFG_PATH).map_err(|e| e.to_string())?;
    let mut doc = content.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    let orch = doc
        .entry("orchestration")
        .or_insert(Item::Table(toml_edit::Table::new()));
    let table = orch.as_table_mut().ok_or("orchestration not a table")?;
    for key in [
        "tools_backend",
        "n8n_tool_url",
        "n8n_pamp_url",
        "n8n_fleet_url",
        "intake_url",
        "default_baseline",
    ] {
        if let Some(v) = partial.get(key).and_then(|v| v.as_str()) {
            table.insert(key, value(v));
        }
    }
    if let Some(v) = partial.get("pamp_shadow").and_then(|v| v.as_bool()) {
        table.insert("pamp_shadow", value(v));
    }
    std::fs::write(CFG_PATH, doc.to_string()).map_err(|e| e.to_string())
}

pub async fn n8n_reachable(url: &str) -> bool {
    let base = url.trim_end_matches("/webhook/fleet-ops").trim_end_matches("/webhook/pamp-route");
    let health = format!("{}/healthz", base);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok();
    let Some(client) = client else {
        return false;
    };
    client.get(&health).send().await.map(|r| r.status().is_success()).unwrap_or(false)
}

pub async fn request_prompt_trim(task: &str, latest_report: &str) -> Option<String> {
    let orch = load_orchestration();
    let base = orch
        .n8n_fleet_url
        .trim_end_matches("/webhook/fleet-ops")
        .trim_end_matches('/');
    let url = format!("{}/webhook/prompt-tuner-failed", base);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let payload = serde_json::json!({
        "lane": "auto-trim",
        "phase": "EXECUTE",
        "task": task,
        "latest_report": latest_report,
    });

    let resp = client.post(&url).json(&payload).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = resp.json::<serde_json::Value>().await.ok()?;
    body.get("tuned_prompt")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
}

/// Enqueue fleet work via n8n webhook (falls back to NFS pending queue).
pub async fn dispatch_fleet(node: &str, action: &str, extra: serde_json::Value) -> serde_json::Value {
    let orch = load_orchestration();
    let repo = crate::paths::project_root();
    let mut body = serde_json::json!({
        "node": node,
        "action": action,
        "repo": repo,
    });
    if let Some(obj) = extra.as_object() {
        for (k, v) in obj {
            body[k] = v.clone();
        }
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    if let Ok(resp) = client.post(&orch.n8n_fleet_url).json(&body).send().await {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                return serde_json::json!({"ok": true, "via": "n8n", "response": v});
            }
            return serde_json::json!({"ok": true, "via": "n8n"});
        }
    }
    // NFS fallback — same layout as fleet-n8n-dispatch.sh
    let ts = chrono_lite_now();
    let job_id = format!("job_{}_{}", ts, std::process::id());
    let q = format!(
        "{}/var/fleet-jobs/pending/{}",
        repo.trim_end_matches('/'),
        node
    );
    if let Err(e) = std::fs::create_dir_all(&q) {
        return serde_json::json!({"ok": false, "error": format!("mkdir {}: {}", q, e)});
    }
    body["job_id"] = serde_json::json!(job_id);
    let path = format!("{}/{}.json", q, job_id);
    match std::fs::write(&path, serde_json::to_string_pretty(&body).unwrap_or_default()) {
        Ok(_) => serde_json::json!({"ok": true, "via": "nfs", "path": path}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    }
}

pub fn golden_test_tasks() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "B5",
            "interactive_fast",
            "Write a Rust fn add(a:i32,b:i32)->i32 with a unit test.",
        ),
        (
            "B3",
            "interactive_fast",
            "Plan: refactor loop_engine corrector cascade into a trait. Bullet steps only.",
        ),
        (
            "B7",
            "interactive_fast",
            "Execute: list three files in cesarops-forge-v2/src and summarize each in one line.",
        ),
        (
            "PAMP",
            "pamp_moe_test",
            "Golden PAMP MoE predictor test: classify this as architecture debug task.",
        ),
        (
            "CAKE",
            "execute_safe",
            "Golden Cake fleet test: verify idle audit path; summarize what cake_fleet final_audit would check.",
        ),
        (
            "B6",
            "interactive_fast",
            "Golden B6 parallel: write a one-line Rust fn max2(a:i32,b:i32)->i32.",
        ),
    ]
}

/// Rule-based expert plan (mirrors n8n Predict node).
pub fn predict_expert_plan(
    message: &str,
    mode: &str,
    baseline_id: &str,
    failure_count: u32,
    tool_name: Option<&str>,
) -> ExpertPlan {
    let lower = message.to_lowercase();
    let mut parallel = Vec::new();
    let mut serial_after = Vec::new();
    let mut skip = vec!["draft".to_string()];

    if mode == "ask" || message.starts_with("/fast") {
        return ExpertPlan {
            parallel: vec!["coder_gemma".into()],
            serial_after: vec![],
            skip,
        };
    }

    if tool_name == Some("think_harder") {
        parallel = vec!["nautivecs".into(), "wso".into()];
        return ExpertPlan {
            parallel,
            serial_after: vec![],
            skip,
        };
    }

    if lower.contains("architecture")
        || lower.contains("why does")
        || lower.contains("debug")
        || lower.contains("design")
    {
        parallel = vec!["thinker_r1".into(), "nautivecs".into()];
        serial_after = vec!["coder_gemma".into()];
    } else {
        parallel = vec!["nautivecs".into()];
        serial_after = vec!["coder_gemma".into()];
        if baseline_id == "execute_safe" || mode == "execute" {
            serial_after.push("reviewer_mtp".into());
        }
    }

    if failure_count >= 3 {
        if !parallel.contains(&"wso".to_string()) {
            parallel.push("wso".into());
        }
    }

    if lower.contains("speculative") || lower.contains("draft") {
        skip.retain(|s| s != "draft");
        parallel.push("draft_phi".into());
    }

    ExpertPlan {
        parallel,
        serial_after,
        skip,
    }
}

pub async fn post_n8n_pamp(
    client: &reqwest::Client,
    url: &str,
    body: &Value,
) -> Option<PampResponse> {
    let resp = client.post(url).json(body).send().await.ok()?;
    if !resp.status().is_success() {
        warn!("n8n PAMP HTTP {}", resp.status());
        return None;
    }
    resp.json().await.ok()
}

/// Shadow: call n8n, log plan, return None so caller uses inline path.
pub async fn pamp_shadow_call(state: &AppState, message: &str, mode: &str) {
    let orch = load_orchestration();
    if !orch.pamp_shadow && orch.tools_backend != "n8n" {
        return;
    }
    let baseline_id = std::env::var("CESAROPS_BASELINE")
        .unwrap_or_else(|_| load_orchestration().default_baseline.clone());
    let plan = predict_expert_plan(message, mode, &baseline_id, 0, None);
    let body = serde_json::json!({
        "message": message,
        "mode": mode,
        "baseline_id": baseline_id,
        "shadow": true,
        "expert_plan": plan,
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let start = Instant::now();
    if let Some(resp) = post_n8n_pamp(&client, &orch.n8n_pamp_url, &body).await {
        let ms = start.elapsed().as_millis() as u64;
        log_pamp_metrics(&plan, &resp, ms);
        info!("PAMP shadow plan: {:?}", resp.expert_plan);
    } else {
        log_pamp_metrics(&plan, &PampResponse {
            expert_plan: plan.clone(),
            results: Value::Null,
            shadow: true,
            parallel_ms_saved: None,
        }, start.elapsed().as_millis() as u64);
    }
}

fn log_pamp_metrics(plan: &ExpertPlan, resp: &PampResponse, elapsed_ms: u64) {
    let path = format!(
        "{}/.cache/cesarops/audit_runs.jsonl",
        std::env::var("HOME").unwrap_or_else(|_| "/home/cesarops".into())
    );
    let line = serde_json::json!({
        "ts": chrono_lite_now(),
        "kind": "pamp",
        "shadow": resp.shadow,
        "parallel": plan.parallel,
        "serial_after": plan.serial_after,
        "skip": plan.skip,
        "parallel_ms_saved": resp.parallel_ms_saved,
        "elapsed_ms": elapsed_ms,
    });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(f, "{}", line);
    }
}

fn chrono_lite_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub async fn route_tool_via_n8n(
    tool_name: &str,
    arguments: &Value,
    raw_output: &str,
) -> Option<String> {
    let orch = load_orchestration();
    if orch.tools_backend != "n8n" {
        return None;
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .ok()?;
    let body = serde_json::json!({
        "tool_call": { "name": tool_name, "arguments": arguments },
        "raw_output": raw_output,
    });
    let resp = client.post(&orch.n8n_tool_url).json(&body).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    v.get("result").and_then(|r| r.as_str()).map(|s| s.to_string())
}

/// Phase 7d: true when a streamed chunk completes a fenced code block.
pub fn should_trigger_partial_review(accumulated: &str) -> bool {
    let fences = accumulated.matches("```").count();
    fences >= 2 && fences % 2 == 0
}

/// Build n8n body for partial review of in-flight codegen.
pub fn partial_review_body(message: &str, code_so_far: &str) -> Value {
    serde_json::json!({
        "message": message,
        "mode": "chat",
        "shadow": false,
        "partial_review": true,
        "code_block": code_so_far,
    })
}

pub fn worker_inject_vectors_enabled() -> bool {
    let content = std::fs::read_to_string(CFG_PATH).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    table
        .get("worker")
        .and_then(|w| w.as_array())
        .and_then(|arr| {
            arr.iter().find(|w| {
                w.get("port").and_then(|p| p.as_integer()) == Some(5001)
            })
        })
        .and_then(|w| w.get("inject_vectors"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// Live PAMP for /code when tools_backend=n8n and shadow off.
pub async fn execute_pamp_code_pipeline(
    state: &AppState,
    task: &str,
) -> Option<serde_json::Value> {
    let orch = load_orchestration();
    let plan = predict_expert_plan(task, "execute", "interactive_fast", 0, None);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .ok()?;
    let body = serde_json::json!({
        "message": task,
        "mode": "execute",
        "shadow": false,
        "expert_plan": plan,
    });
    if let Some(resp) = post_n8n_pamp(&client, &orch.n8n_pamp_url, &body).await {
        return Some(serde_json::json!({
            "task": task,
            "pipeline": "pamp_n8n",
            "expert_plan": resp.expert_plan,
            "results": resp.results,
        }));
    }
    // Fallback: parallel vec + serial coder/reviewer in Forge
    let vec_fut = fetch_nautivecs_snippet(state, task, 5);
    let cfg = state.config.read().await;
    let coder_url = cfg.coder_url.clone();
    let reviewer = routing::resolve_reviewer_endpoint().await;
    drop(cfg);
    let vec_ctx = vec_fut.await;
    let prompt = format!(
        "<|im_start|>system\nRust+wgpu specialist. Context:\n{}\n<|im_end|>\n\
         <|im_start|>user\n{}\n<|im_end|>\n<|im_start|>assistant\n",
        &vec_ctx[..vec_ctx.len().min(4000)],
        task
    );
    let client = reqwest::Client::new();
    let code = crate::inference_client::complete_prompt(
        &client, &coder_url, &prompt, 8192, 0.3, vec![], Some("llama-server"),
    )
    .await
    .ok()?;
    let review_prompt = format!(
        "<|im_start|>user\nReview:\n{}\n<|im_end|>\n<|im_start|>assistant\n",
        if code.len() > 6000 { &code[..6000] } else { &code[..] }
    );
    let review = crate::inference_client::complete_prompt(
        &client, &reviewer, &review_prompt, 2048, 0.2, vec![], Some("llama-server"),
    )
    .await
    .unwrap_or_default();
    Some(serde_json::json!({
        "task": task,
        "pipeline": "pamp_forge_fallback",
        "expert_plan": plan,
        "initial_code": code,
        "review": review,
        "final": code,
    }))
}

pub async fn fetch_nautivecs_snippet(state: &AppState, query: &str, top_k: u32) -> String {
    let cfg = state.config.read().await;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let body = serde_json::json!({ "query": query, "top_k": top_k });
    match client.post(&cfg.nautivecs_url).json(&body).send().await {
        Ok(r) if r.status().is_success() => {
            r.text().await.unwrap_or_default()
        }
        _ => String::new(),
    }
}
