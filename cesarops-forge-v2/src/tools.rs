use crate::AppState;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::process::Command;
use tracing::{info, warn};

/// Counter for think_harder calls per session. Resets on /clear.
static THINK_HARDER_COUNT: AtomicU32 = AtomicU32::new(0);
const THINK_HARDER_LIMIT: u32 = 30;

/// Reset the think_harder counter (called on /clear).
pub fn reset_think_counter() {
    THINK_HARDER_COUNT.store(0, Ordering::Relaxed);
}

/// Execute a tool by name with the given arguments. Returns the tool result as a string.
pub async fn execute(name: &str, arguments: &Value, state: &AppState) -> String {
    info!("Tool call: {} args={}", name, arguments);

    match name {
        "write_file" => { reset_think_counter(); write_file(arguments, state).await },
        "read_file" => { reset_think_counter(); read_file(arguments, state).await },
        "cargo_check" => { reset_think_counter(); cargo_check(arguments, state).await },
        "think_harder" => {
            let count = THINK_HARDER_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if count > THINK_HARDER_LIMIT {
                return format!("[SEARCH LIMIT: {}/{} consecutive searches with no action. Use the results you have — write_file, read_file, or provide your answer. Counter resets when you take action.]", count, THINK_HARDER_LIMIT);
            }
            let result = think_harder(arguments, state).await;
            format!("{}\n[Search {}/{} — counter resets when you call write_file/read_file/cargo_check]", result, count, THINK_HARDER_LIMIT)
        },
        "remember" => { reset_think_counter(); remember(arguments, state).await },
        "run_command" => { reset_think_counter(); run_command(arguments, state).await },
        _ => format!("Unknown tool: '{}'. Available: write_file, read_file, cargo_check, think_harder, remember, run_command", name),
    }
}

/// Write content to a file under project_root.
async fn write_file(args: &Value, state: &AppState) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument required".to_string(),
    };
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'content' argument required".to_string(),
    };

    let full_path = resolve_path(path, state);

    // Create parent directories
    if let Some(parent) = full_path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return format!("Error creating directories: {}", e);
        }
    }

    match tokio::fs::write(&full_path, content).await {
        Ok(_) => format!("Written {} bytes to {}", content.len(), path),
        Err(e) => format!("Error writing file: {}", e),
    }
}

/// Read a file, truncating at 3000 chars.
async fn read_file(args: &Value, state: &AppState) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument required".to_string(),
    };

    let full_path = resolve_path(path, state);

    match tokio::fs::read_to_string(&full_path).await {
        Ok(content) => {
            if content.len() > 3000 {
                format!("{}...\n[truncated at 3000 chars, total {} bytes]", &content[..3000], content.len())
            } else {
                content
            }
        }
        Err(e) => format!("Error reading file: {}", e),
    }
}

/// Run cargo check with --message-format=json, parse errors.
async fn cargo_check(args: &Value, state: &AppState) -> String {
    let dir = args
        .get("dir")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    let full_dir = resolve_path(dir, state);

    let output = match Command::new("cargo")
        .args(["check", "--message-format=json"])
        .current_dir(&full_dir)
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => return format!("Error running cargo check: {}", e),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Extract compiler errors from JSON lines
    let mut errors: Vec<String> = Vec::new();
    for line in stdout.lines() {
        if let Ok(msg) = serde_json::from_str::<Value>(line) {
            if msg.get("reason").and_then(|r| r.as_str()) == Some("compiler-message") {
                if let Some(message) = msg.get("message") {
                    let level = message.get("level").and_then(|l| l.as_str()).unwrap_or("");
                    let text = message.get("message").and_then(|m| m.as_str()).unwrap_or("");
                    if level == "error" {
                        errors.push(text.to_string());
                    }
                }
            }
        }
    }

    if errors.is_empty() && output.status.success() {
        "cargo check: OK (no errors)".to_string()
    } else if errors.is_empty() {
        // Fallback to stderr
        let truncated = truncate_output(&stderr, 2000);
        format!("cargo check FAILED:\n{}", truncated)
    } else {
        let error_list = errors.iter().take(10).cloned().collect::<Vec<_>>().join("\n- ");
        format!("cargo check: {} error(s):\n- {}", errors.len(), error_list)
    }
}

/// Search nautivecs + WSO for information.
async fn think_harder(args: &Value, state: &AppState) -> String {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return "Error: 'query' argument required".to_string(),
    };

    let client = reqwest::Client::new();
    let mut results = Vec::new();

    // Query nautivecs
    let nautivecs_result = client
        .post(&state.config.nautivecs_url)
        .json(&serde_json::json!({"query": query, "top_k": 3}))
        .send()
        .await;

    match nautivecs_result {
        Ok(resp) => {
            if let Ok(body) = resp.text().await {
                let truncated = truncate_output(&body, 1500);
                results.push(format!("[nautivecs]: {}", truncated));
            }
        }
        Err(e) => {
            warn!("nautivecs query failed: {}", e);
            results.push(format!("[nautivecs]: unavailable ({})", e));
        }
    }

    // Query WSO (web search)
    let wso_result = client
        .post(&state.config.wso_url)
        .json(&serde_json::json!({"query": query, "max_results": 3}))
        .send()
        .await;

    match wso_result {
        Ok(resp) => {
            if let Ok(body) = resp.text().await {
                let truncated = truncate_output(&body, 1500);
                results.push(format!("[web search]: {}", truncated));
            }
        }
        Err(e) => {
            warn!("WSO query failed: {}", e);
            results.push(format!("[web search]: unavailable ({})", e));
        }
    }

    if results.is_empty() {
        "No results from knowledge base or web search.".to_string()
    } else {
        results.join("\n\n")
    }
}

/// Append a lesson to research_log/lessons_learned.md and (optionally) nautivecs.
async fn remember(args: &Value, state: &AppState) -> String {
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'content' argument required".to_string(),
    };
    let tags = args
        .get("tags")
        .and_then(|v| v.as_str())
        .unwrap_or("general");

    let log_path = resolve_path("research_log/lessons_learned.md", state);

    // Ensure directory exists
    if let Some(parent) = log_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let entry = format!("\n## [{}] {}\n{}\n", tags, chrono_now(), content);

    match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .await
    {
        Ok(_file) => {
            // Use write since OpenOptions append with tokio is simpler this way
            let existing = tokio::fs::read_to_string(&log_path).await.unwrap_or_default();
            let new_content = format!("{}{}", existing, entry);
            match tokio::fs::write(&log_path, new_content).await {
                Ok(_) => format!("Remembered (tags: {}): {}...", tags, &content[..content.len().min(80)]),
                Err(e) => format!("Error writing memory: {}", e),
            }
        }
        Err(e) => format!("Error opening log: {}", e),
    }
}

/// Run a shell command with K-line guards.
async fn run_command(args: &Value, state: &AppState) -> String {
    let cmd = match args.get("cmd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'cmd' argument required".to_string(),
    };

    // K-line guard: block dangerous commands
    let blocked = ["rm -rf /", "rm -rf /*", "dd if=", "mkfs", "> /dev/sd", "chmod 777 /"];
    for pattern in &blocked {
        if cmd.contains(pattern) {
            warn!("K-LINED command blocked: {}", cmd);
            return format!("BLOCKED: Command '{}' is K-lined (dangerous operation)", cmd);
        }
    }

    let output = match Command::new("bash")
        .args(["-c", cmd])
        .current_dir(&state.config.project_root)
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => return format!("Error executing command: {}", e),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(&truncate_output(&stdout, 2000));
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push_str("\n[stderr]: ");
        }
        result.push_str(&truncate_output(&stderr, 500));
    }

    if result.is_empty() {
        format!("Command completed (exit code: {})", output.status.code().unwrap_or(-1))
    } else {
        result
    }
}

// --- Helpers ---

fn resolve_path(relative: &str, state: &AppState) -> PathBuf {
    let root = PathBuf::from(&state.config.project_root);
    root.join(relative)
}

fn truncate_output(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...[truncated, {} total bytes]", &s[..max], s.len())
    } else {
        s.to_string()
    }
}

/// Simple timestamp without pulling in chrono crate.
fn chrono_now() -> String {
    // Use system time as unix timestamp
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}
