//! OperatorSpec draft / approve gate — wires Gemini + local spec thinker into Forge.
//!
//! Draft runs `scripts/spec_thinker_roundtrip.py` (credentials via `credentials.gemini.local.sh`).
//! Approved specs inject into steering before `/send` executes tools.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecDraftConfig {
    pub enabled: bool,
    /// `gemini` | `local` | `auto` (try gemini, fallback local)
    pub thinker: String,
    pub draft_url: String,
    pub local_fallback_urls: String,
    pub gemini_model: String,
    pub require_approval_before_send: bool,
    pub timeout_secs: u64,
}

impl Default for SpecDraftConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            thinker: "auto".to_string(),
            draft_url: "http://127.0.0.1:5200".to_string(),
            local_fallback_urls: "http://127.0.0.1:5200,http://127.0.0.1:5203".to_string(),
            gemini_model: "gemini-2.5-flash".to_string(),
            require_approval_before_send: true,
            timeout_secs: 300,
        }
    }
}

impl SpecDraftConfig {
    fn from_toml(table: &toml::Table) -> Self {
        let d = Self::default();
        Self {
            enabled: table
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(d.enabled),
            thinker: table
                .get("thinker")
                .and_then(|v| v.as_str())
                .unwrap_or(&d.thinker)
                .to_string(),
            draft_url: table
                .get("draft_url")
                .or_else(|| table.get("spec_draft_url"))
                .and_then(|v| v.as_str())
                .unwrap_or(&d.draft_url)
                .to_string(),
            local_fallback_urls: table
                .get("local_fallback_urls")
                .and_then(|v| v.as_str())
                .unwrap_or(&d.local_fallback_urls)
                .to_string(),
            gemini_model: table
                .get("gemini_model")
                .and_then(|v| v.as_str())
                .unwrap_or(&d.gemini_model)
                .to_string(),
            require_approval_before_send: table
                .get("require_approval_before_send")
                .and_then(|v| v.as_bool())
                .unwrap_or(d.require_approval_before_send),
            timeout_secs: table
                .get("timeout_secs")
                .and_then(|v| v.as_integer())
                .map(|n| n.max(30) as u64)
                .unwrap_or(d.timeout_secs),
        }
    }
}

pub fn load_config() -> SpecDraftConfig {
    let path = paths::cluster_config_path();
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    table
        .get("spec_draft")
        .and_then(|v| v.as_table())
        .map(SpecDraftConfig::from_toml)
        .unwrap_or_default()
}

pub fn repo_root() -> PathBuf {
    for p in [
        "/data/codebase/repos/wreckhunter2000-1",
        "/codebase/repos/wreckhunter2000-1",
        "/codebase/wreckhunter2000-1",
    ] {
        if Path::new(p).is_dir() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from(paths::project_root())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorSpecRecord {
    pub spec: serde_json::Value,
    pub path: String,
    pub approved: bool,
    pub source: String,
    pub title: String,
    pub created_at: u64,
}

#[derive(Debug, Default)]
pub struct OperatorSpecGate {
    pub pending: Option<OperatorSpecRecord>,
}

pub type SharedSpecGate = Arc<Mutex<OperatorSpecGate>>;

pub fn latest_spec_dir(repo: &Path) -> PathBuf {
    repo.join("var/spec_thinker/latest")
}

pub async fn run_spec_draft(
    intent: &str,
    context: &str,
    cfg: &SpecDraftConfig,
) -> Result<OperatorSpecRecord, String> {
    if !cfg.enabled {
        return Err("spec_draft disabled in cluster_config.toml [spec_draft]".to_string());
    }

    let repo = repo_root();
    let script = repo.join("scripts/spec_thinker_roundtrip.py");
    if !script.is_file() {
        return Err(format!("missing {}", script.display()));
    }

    let out_dir = latest_spec_dir(&repo);
    if out_dir.exists() {
        let _ = std::fs::remove_dir_all(&out_dir);
    }
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;

    let thinker = cfg.thinker.to_lowercase();
    let local_only = thinker == "local";

    let mut cmd = tokio::process::Command::new("python3");
    cmd.current_dir(&repo)
        .arg(&script)
        .arg("--intent")
        .arg(intent)
        .arg("--repo")
        .arg(repo.to_string_lossy().as_ref())
        .arg("--out")
        .arg(&out_dir)
        .env("REPO", repo.to_string_lossy().as_ref())
        .env("GEMINI_MODEL", &cfg.gemini_model)
        .env("SPEC_LOCAL_URLS", &cfg.local_fallback_urls)
        .env(
            "SPEC_DRAFT_URL",
            std::env::var("SPEC_DRAFT_URL").unwrap_or_else(|_| cfg.draft_url.clone()),
        );

    if !context.trim().is_empty() {
        cmd.arg("--context").arg(context);
    }
    if local_only {
        cmd.arg("--local-only");
    }

    info!(
        "spec/draft thinker={} intent={}",
        thinker,
        &intent[..intent.len().min(80)]
    );

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(cfg.timeout_secs),
        cmd.output(),
    )
    .await
    .map_err(|_| format!("spec draft timed out after {}s", cfg.timeout_secs))?
    .map_err(|e| format!("failed to run spec_thinker_roundtrip: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "spec_thinker_roundtrip exit {}:\n{}\n{}",
            output.status,
            stderr.trim(),
            stdout.trim()
        ));
    }

    let spec_path = out_dir.join("operator_spec.json");
    if !spec_path.is_file() {
        return Err(format!("missing {}", spec_path.display()));
    }

    let raw = std::fs::read_to_string(&spec_path).map_err(|e| e.to_string())?;
    let spec: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let title = spec
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("OperatorSpec")
        .to_string();
    let source_path = out_dir.join("source.txt");
    let source = std::fs::read_to_string(&source_path)
        .unwrap_or_else(|_| "forge".to_string())
        .trim()
        .to_string();

    Ok(OperatorSpecRecord {
        spec,
        path: spec_path.to_string_lossy().into_owned(),
        approved: false,
        source,
        title,
        created_at: now_unix(),
    })
}

pub fn steering_block(record: &OperatorSpecRecord) -> String {
    let overview = record
        .spec
        .get("architecture_overview")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let constraints = record
        .spec
        .get("constraints")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c.as_str())
                .map(|s| format!("- {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let mut tasks = String::new();
    if let Some(list) = record.spec.get("ordered_tasks").and_then(|v| v.as_array()) {
        for (i, t) in list.iter().enumerate() {
            let path = t
                .get("target_file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let done = t
                .get("done_condition")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            tasks.push_str(&format!("\n{}. {} — done when: {}", i + 1, path, done));
        }
    } else if let Some(list) = record.spec.get("do").and_then(|v| v.as_array()) {
        for t in list {
            let step = t.get("step").and_then(|v| v.as_u64()).unwrap_or(0);
            let action = t.get("action").and_then(|v| v.as_str()).unwrap_or("");
            tasks.push_str(&format!("\n{step}. {action}"));
        }
    }

    format!(
        "[APPROVED OPERATOR SPEC — execute closed-world; do not expand scope]\n\
         Title: {}\n\
         Overview: {}\n\
         Constraints:\n{}\n\
         Tasks:{}\n\
         Spec file: {}",
        record.title, overview, constraints, tasks, record.path
    )
}

pub fn send_blocked_message(title: &str) -> String {
    format!(
        "[Spec gate] Operator spec \"{title}\" is drafted but not approved.\n\
         1. Review var/spec_thinker/latest/operator_spec.json\n\
         2. POST /spec/approve  (or click Spec Approve in the UI)\n\
         3. Then /send again\n\
         Or POST /spec/clear to discard and chat without a spec."
    )
}

pub async fn inject_approved_spec(
    gate: &SharedSpecGate,
    steering: &Arc<Mutex<Vec<String>>>,
) -> bool {
    let record = {
        let g = gate.lock().await;
        match &g.pending {
            Some(r) if r.approved => r.clone(),
            _ => return false,
        }
    };
    let block = steering_block(&record);
    let mut st = steering.lock().await;
    if st.iter().any(|s| s.contains("APPROVED OPERATOR SPEC")) {
        return false;
    }
    st.push(block);
    info!("injected approved operator spec into steering: {}", record.title);
    true
}

pub fn status_json(gate: &OperatorSpecGate, cfg: &SpecDraftConfig) -> serde_json::Value {
    serde_json::json!({
        "enabled": cfg.enabled,
        "thinker": cfg.thinker,
        "draft_url": cfg.draft_url,
        "local_fallback_urls": cfg.local_fallback_urls,
        "require_approval_before_send": cfg.require_approval_before_send,
        "pending": gate.pending.as_ref().map(|p| serde_json::json!({
            "title": p.title,
            "path": p.path,
            "approved": p.approved,
            "source": p.source,
            "created_at": p.created_at,
        })),
    })
}
