use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::process::Command;
use tracing::{info, warn};

/// Counter for think_harder calls per session. Resets on /clear.
static THINK_HARDER_COUNT: AtomicU32 = AtomicU32::new(0);
const THINK_HARDER_LIMIT: u32 = u32::MAX; // No limit — let it search as much as it needs

pub fn reset_think_counter() {
    THINK_HARDER_COUNT.store(0, Ordering::SeqCst);
}

/// Execute a tool by name with given arguments and project root.
pub async fn execute(name: &str, arguments: &Value, project_root: &Path) -> String {
    info!("Tool call: {} args={}", name, arguments);

    match name {
        "write_file" => { reset_think_counter(); write_file(arguments, project_root).await },
        "read_file" => { reset_think_counter(); read_file(arguments, project_root).await },
        "cargo_check" => { reset_think_counter(); cargo_check(arguments, project_root).await },
        "think_harder" => { reset_think_counter(); think_harder(arguments).await },
        "remember" => { reset_think_counter(); remember(arguments, project_root).await },
        "run_command" => run_command(arguments, project_root).await,
        _ => format!("Unknown tool: {}. Available tools: write_file, read_file, cargo_check, think_harder, remember, run_command", name),
    }
}

/// Write a file to the project root. Creates parent directories if needed.
async fn write_file(args: &Value, project_root: &Path) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument required".to_string(),
    };
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'content' argument required".to_string(),
    };
    let full_path = resolve_path(path, project_root);
    
    // Create parent directories
    if let Some(parent) = full_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return format!("Error creating directory {}: {}", parent.display(), e);
        }
    }
    
    match fs::File::create(&full_path) {
        Ok(mut file) => {
            match file.write_all(content.as_bytes()) {
                Ok(_) => {
                    info!("Written {} bytes to {}", content.len(), full_path.display());
                    format!("Successfully wrote {} ({} bytes)", full_path.display(), content.len())
                },
                Err(e) => format!("Error writing file: {}", e),
            }
        },
        Err(e) => format!("Error creating file: {}", e),
    }
}

/// Read a file, truncating at 3000 chars.
async fn read_file(args: &Value, project_root: &Path) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument required".to_string(),
    };
    let full_path = resolve_path(path, project_root);
    
    match fs::read_to_string(&full_path) {
        Ok(contents) => {
            let truncated = truncate_output(&contents, 3000);
            format!("Contents of {}:\n{}", full_path.display(), truncated)
        },
        Err(e) => format!("Error reading file {}: {}", full_path.display(), e),
    }
}

/// Run cargo check on the given directory.
async fn cargo_check(args: &Value, project_root: &Path) -> String {
    let dir = match args.get("dir").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => return "Error: 'dir' argument required".to_string(),
    };
    let full_dir = resolve_path(dir, project_root);
    
    info!("Running cargo check in {}", full_dir.display());
    
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&full_dir)
        .output()
        .await;
    
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() {
                format!("Cargo check succeeded:\n{}", truncate_output(&stdout, 2000))
            } else {
                format!("Cargo check failed:\nSTDOUT:{}\nSTDERR:{}", 
                    truncate_output(&stdout, 1500), truncate_output(&stderr, 1500))
            }
        },
        Err(e) => format!("Failed to run cargo check: {}", e),
    }
}

/// Search nautivecs knowledge base + web (same contract as forge-v2).
async fn think_harder(args: &Value) -> String {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return "Error: 'query' argument required".to_string(),
    };

    let count = THINK_HARDER_COUNT.fetch_add(1, Ordering::SeqCst);
    if count >= THINK_HARDER_LIMIT {
        warn!("think_harder limit reached (count={})", count);
        return "Error: think_harder limit reached".to_string();
    }

    info!("think_harder search #{}: {}", count + 1, query);

    let client = reqwest::Client::new();
    let mut results = Vec::new();

    if let Ok(resp) = client
        .post(crate::knowledge::nautivecs_query_url())
        .json(&serde_json::json!({"query": query, "top_k": 3}))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
    {
        if let Ok(body) = resp.text().await {
            results.push(format!("[nautivecs]: {}", truncate_output(&body, 1500)));
        }
    } else {
        results.push("[nautivecs]: unavailable".to_string());
    }

    if let Ok(resp) = client
        .post(crate::knowledge::wso_url())
        .json(&serde_json::json!({"query": query, "max_results": 3}))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
    {
        if let Ok(body) = resp.text().await {
            results.push(format!("[web search]: {}", truncate_output(&body, 1500)));
        }
    }

    if results.is_empty() {
        "No results from knowledge base or web search.".to_string()
    } else {
        results.join("\n\n")
    }
}

/// Save a lesson to research_log and nautivecs /add.
async fn remember(args: &Value, project_root: &Path) -> String {
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'content' argument required".to_string(),
    };
    let tags = args.get("tags").and_then(|v| v.as_str()).unwrap_or("general");

    let log_path = resolve_path("research_log/lessons_learned.md", project_root);
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let entry = format!("\n## [{}] {}\n{}\n", tags, chrono_now(), content);
    let existing = fs::read_to_string(&log_path).unwrap_or_default();
    let log_ok = fs::write(&log_path, format!("{}{}", existing, entry)).is_ok();

    let client = reqwest::Client::new();
    let nautivecs_status = match client
        .post(crate::knowledge::nautivecs_add_url())
        .json(&serde_json::json!({
            "text": format!("[{}] {}", tags, content),
            "tags": tags,
            "source": "lessons_learned",
            "file_path": "research_log/lessons_learned.md",
        }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => " + indexed in nautivecs".to_string(),
        Ok(r) => format!(" (nautivecs {})", r.status()),
        Err(e) => format!(" (nautivecs unavailable: {})", e),
    };

    if log_ok {
        format!("Remembered (tags: {}){}", tags, nautivecs_status)
    } else {
        format!("Error writing {}", log_path.display())
    }
}

/// Execute a shell command.
async fn run_command(args: &Value, project_root: &Path) -> String {
    let cmd = match args.get("cmd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'cmd' argument required".to_string(),
    };
    
    info!("Executing command: {}", cmd);
    
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(project_root)
        .output()
        .await;
    
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() {
                format!("Command succeeded:\nSTDOUT:{}\nSTDERR:{}", 
                    truncate_output(&stdout, 2000), truncate_output(&stderr, 1000))
            } else {
                format!("Command failed (exit code {:?}):\nSTDOUT:{}\nSTDERR:{}", 
                    out.status.code(), truncate_output(&stdout, 1500), truncate_output(&stderr, 1500))
            }
        },
        Err(e) => format!("Failed to execute command: {}", e),
    }
}

/// Resolve a relative path against the project root.
fn resolve_path(relative: &str, project_root: &Path) -> PathBuf {
    let p = Path::new(relative);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        project_root.join(p)
    }
}

/// Truncate output string to max length with ellipsis.
fn truncate_output(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}... [truncated, {} total bytes]", &s[..max], s.len())
    }
}

/// Get current timestamp in ISO format.
fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}
