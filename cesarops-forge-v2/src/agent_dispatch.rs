//! Agent Dispatch — wraps any LLM endpoint with tool-calling capability.
//! 
//! Any GPU worker (1060, 1070, P100, laptop) can become an agent by routing
//! through this dispatcher. It injects the system prompt with tool definitions,
//! parses <tool_call> blocks from the model output, executes them, and feeds
//! results back until the model produces a final answer.
//!
//! Usage: call `run_agent_loop(endpoint_url, user_message, project_root)` and
//! it handles the full multi-turn tool-calling loop.

use async_recursion::async_recursion;
use tracing::{info, warn};
use std::path::PathBuf;
use tokio::process::Command;

const MAX_ROUNDS: usize = 20;
const TOOL_CALL_OPEN: &str = "<tool_call>";
const TOOL_CALL_CLOSE: &str = "</tool_call>";

#[derive(Clone)]
pub struct AgentConfig {
    pub endpoint_url: String,
    pub project_root: String,
    pub nautivecs_url: String,
    pub wso_url: String,
    /// Optional remote MCP worker for tool offload (e.g. http://10.0.0.201:8090).
    pub mcp_worker_url: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
    /// Safe mode: write_file creates duplicates instead of overwriting.
    /// Blocks deletion and modification of existing files.
    pub safe_mode: bool,
    /// qwen2.5 | gemma | deepseek-r1 | llama3
    pub chat_template: String,
    /// Inference engine: None/llama-server → OpenAI API; "koboldcpp" → Kobold generate API.
    pub engine: Option<String>,
    /// Nesting depth for fleet sub-agent delegation (0 = primary agent).
    pub fleet_delegate_depth: u32,
}

/// The system prompt injected for agent mode. Same tools as forge-v2.
fn agent_system_prompt(project_root: &str) -> String {
    format!(r#"You are CESAROPS Agent, an autonomous developer running on local GPUs. You have access to tools for file operations, code checking, web search, memory, and satellite mission execution.

## RULES
1. You MUST use your tools. NEVER ask the user to do things you can do yourself.
2. ALWAYS save your work using write_file.
3. Use think_harder when blocked, after a failed attempt, or when facts are missing.
4. After completing a task, use remember to save lessons learned.

## Tools

Call tools using this exact format:
<tool_call>
{{"name": "tool_name", "arguments": {{"key": "value"}}}}
</tool_call>

### Available Tools:
- **think_harder**: Search knowledge base + web. Args: {{"query": "search query"}}
- **read_file**: Read a file. Args: {{"path": "relative/path"}}
- **write_file**: Write content to a file. Args: {{"path": "relative/path", "content": "file content"}}
- **cargo_check**: Run cargo check. Args: {{"dir": "relative/path"}}
- **run_command**: Execute a shell command. Args: {{"cmd": "command string"}}
- **remember**: Save a lesson. Args: {{"content": "what to remember", "tags": "comma,separated,tags"}}
- **list_fleet_agents**: List loaded LLMs on T440 + cesarops2 (online/offline, roles). Args: {{}}
- **call_sub_agent**: Ask another fleet model one question. Args: {{"target": "thinker|reviewer|coder|draft|gemma|marvin|picasso|http://...", "message": "your question"}}
- **delegate_sub_agent**: Run a full tool-enabled sub-agent on another GPU. Args: {{"target": "thinker", "task": "detailed sub-task"}}
- **download_satellite_window**: Run universal_downloader window job. Args: {{"bbox":"lat_min,lon_min,lat_max,lon_max","provider":"aws|all|sentinel2|landsat|...","days":14,"max_results":20}}
- **sat_mission**: Run mission orchestrator from JSON spec. Args: {{"spec_path":"path/to/spec.json","dry_run":false}}
- **sat_read_mission_report**: Read mission/validation reports. Args: {{"output_dir":"...","which":"mission|validation|both"}}
- **weather_window**: Get weather-conditioned scan windows. Args: {{"bbox":"lat_min,lon_min,lat_max,lon_max","check":"post_storm|calm","days":14}}
- **detection_health**: Probe triple-lock detection service. Args: {{}}
- **detection_scan**: Submit tile scan job. Args: {{"region":"label","tiles":[{{"lat":..,"lon":..,"image_b64":"..."}}]}}
- **detection_poll**: Poll a detection job id. Args: {{"job_id":"..."}}
- **search_symbols**: Symbol-aware code search (SymForge-compatible). Args: {{"query":"...","limit":20}}
- **get_symbol**: Read a symbol definition and context. Args: {{"name":"symbol_name","path":"optional/file"}}
- **get_file_context**: Summarize file symbols/imports/dependencies. Args: {{"path":"relative/path"}}
- **search_text**: Project text/regex search. Args: {{"query":"...","glob":"*.rs","max_results":50}}
- **replace_symbol_body**: Replace symbol implementation body. Args: {{"name":"...","new_body":"..."}}
- **edit_within_symbol**: Scoped find/replace in symbol range. Args: {{"name":"...","find":"...","replace":"..."}}
- **insert_symbol**: Insert symbol before/after target symbol. Args: {{"target":"...","position":"before|after","code":"..."}}
- **delete_symbol**: Delete symbol by name. Args: {{"name":"..."}}
- **batch_edit**: Multi-file structural edits. Args: {{"edits":[...]}}
- **batch_rename**: Rename symbol and references. Args: {{"old_name":"...","new_name":"..."}}

## Context:
- Project root: {}
- You have FULL filesystem access. Use it."#, project_root)
}

/// Sub-agent entry (non-recursive async path for fleet delegation).
pub async fn run_sub_agent_loop(config: &AgentConfig, user_message: &str) -> String {
    run_agent_loop_inner(config, user_message).await
}

/// Run the full agent loop: send message, parse tool calls, execute, repeat.
/// Returns the final text response from the model.
pub async fn run_agent_loop(config: &AgentConfig, user_message: &str) -> String {
    run_agent_loop_inner(config, user_message).await
}

#[async_recursion]
async fn run_agent_loop_inner(config: &AgentConfig, user_message: &str) -> String {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .unwrap();

    let system = agent_system_prompt(&config.project_root);
    let messages = vec![("user".to_string(), user_message.to_string())];
    let mut conversation = crate::prompts::format_for_template(
        &config.chat_template,
        &system,
        &messages,
        true,
    );

    for round in 0..MAX_ROUNDS {
        info!("Agent round {}/{}", round + 1, MAX_ROUNDS);

        let engine = config.engine.as_deref();
        let text = match crate::inference_client::complete_prompt(
            &client,
            &config.endpoint_url,
            &conversation,
            config.max_tokens,
            config.temperature,
            vec![TOOL_CALL_CLOSE.to_string(), "<|im_end|>".to_string()],
            engine,
        )
        .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!("Agent request failed: {}", e);
                return format!("[Agent error: {}]", e);
            }
        };

        // Check if model produced a tool call
        if let Some(tool_json) = extract_tool_call(&text) {
            let tool_name = tool_json.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = tool_json.get("arguments").cloned().unwrap_or_default();

            info!("Agent tool call: {} args={}", tool_name, arguments);

            let result = if config.fleet_delegate_depth >= 1 {
                execute_tool_sub_agent_only(tool_name, &arguments, config).await
            } else {
                execute_tool(tool_name, &arguments, config).await
            };

            // Append the tool call and result to conversation
            conversation.push_str(&text);
            conversation.push_str(TOOL_CALL_CLOSE);
            let tool_result_msg = format!(
                "[Tool Result - Round {}/{}]: {}\nNow continue. Either call another tool or provide your final answer.",
                round + 1, MAX_ROUNDS, result
            );
            if config.chat_template == "gemma" {
                conversation.push_str(&format!(
                    "<end_of_turn>\n<start_of_turn>user\n{}\n<end_of_turn>\n<start_of_turn>model\n",
                    tool_result_msg
                ));
            } else {
                conversation.push_str(&format!(
                    "<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                    tool_result_msg
                ));
            }
        } else {
            // No tool call — this is the final answer
            return text.trim().to_string();
        }
    }

    "[Agent reached max rounds without final answer]".to_string()
}

/// Extract a tool call JSON from model output.
fn extract_tool_call(text: &str) -> Option<serde_json::Value> {
    // Look for <tool_call> ... or just raw JSON with "name" and "arguments"
    let search = if let Some(start) = text.find(TOOL_CALL_OPEN) {
        &text[start + TOOL_CALL_OPEN.len()..]
    } else if text.contains("\"name\"") && text.contains("\"arguments\"") {
        text
    } else {
        return None;
    };

    // Try to parse JSON from the remaining text
    let trimmed = search.trim();
    // Find the JSON object boundaries
    if let Some(start) = trimmed.find('{') {
        let mut depth = 0;
        let mut end = start;
        for (i, ch) in trimmed[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&trimmed[start..end]) {
            if val.get("name").is_some() {
                return Some(val);
            }
        }
    }
    None
}

async fn fleet_delegate_impl(args: &serde_json::Value, config: &AgentConfig) -> String {
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
    let Some(ep) = crate::model_command::resolve_fleet_target(target).await else {
        return format!("Error: fleet target '{}' not found", target);
    };
    if !ep.online {
        return format!("Error: {} is offline", ep.label);
    }
    let cluster = crate::routing::load_cluster_routing();
    let mut sub = config.clone();
    sub.endpoint_url = ep.url;
    sub.fleet_delegate_depth = 1;
    sub.chat_template = crate::routing::template_for_endpoint(&cluster, &sub.endpoint_url);
    let out = run_agent_loop_inner(&sub, task).await;
    format!("[sub-agent {}]\n{}", ep.label, out)
}

/// Tools available inside a delegated sub-agent (no nested delegate).
async fn execute_tool_sub_agent_only(name: &str, args: &serde_json::Value, config: &AgentConfig) -> String {
    if name == "delegate_sub_agent" {
        return "Error: nested delegate_sub_agent blocked (max depth 1)".to_string();
    }
    execute_tool_core(name, args, config).await
}

/// Execute a tool and return the result string.
async fn execute_tool(name: &str, args: &serde_json::Value, config: &AgentConfig) -> String {
    if name == "delegate_sub_agent" {
        return fleet_delegate_impl(args, config).await;
    }
    execute_tool_core(name, args, config).await
}

async fn execute_tool_core(name: &str, args: &serde_json::Value, config: &AgentConfig) -> String {
    if let Some(ref mcp_base) = config.mcp_worker_url {
        if !mcp_base.is_empty() && crate::mcp_delegate::is_delegatable_tool(name) {
            if let Ok(r) = crate::mcp_delegate::execute_on_mcp_at(mcp_base, name, args).await {
                return format!("[mcp] {}", r);
            }
        }
    }

    match name {
        "write_file" => tool_write_file(args, &config.project_root, config.safe_mode).await,
        "read_file" => tool_read_file(args, &config.project_root).await,
        "cargo_check" => tool_cargo_check(args, &config.project_root).await,
        "think_harder" => tool_think_harder(args, config).await,
        "remember" => tool_remember(args, &config.project_root, &config.nautivecs_url).await,
        "run_command" => tool_run_command(args, &config.project_root, config.safe_mode).await,
        "list_fleet_agents" => crate::model_command::tool_list_fleet_agents().await,
        "call_sub_agent" => crate::model_command::tool_call_sub_agent(args).await,
        _ => format!("Unknown tool: '{}'", name),
    }
}

async fn tool_write_file(args: &serde_json::Value, root: &str, safe_mode: bool) -> String {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() || content.is_empty() {
        return "Error: path and content required".to_string();
    }
    let full = PathBuf::from(root).join(path);

    if safe_mode && full.exists() {
        // File exists — write to .draft/ shadow instead of overwriting
        let draft_dir = PathBuf::from(root).join(".draft");
        let draft_path = draft_dir.join(path);
        if let Some(parent) = draft_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        match tokio::fs::write(&draft_path, content).await {
            Ok(_) => format!("[SAFE MODE] Original preserved. Draft written to .draft/{} ({} bytes)", path, content.len()),
            Err(e) => format!("Error writing draft: {}", e),
        }
    } else {
        // New file or safe_mode off — write normally
        if let Some(parent) = full.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        match tokio::fs::write(&full, content).await {
            Ok(_) => format!("Written {} bytes to {}", content.len(), path),
            Err(e) => format!("Error: {}", e),
        }
    }
}

async fn tool_read_file(args: &serde_json::Value, root: &str) -> String {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() { return "Error: path required".to_string(); }
    let full = PathBuf::from(root).join(path);
    match tokio::fs::read_to_string(&full).await {
        Ok(c) => {
            if c.len() > 3000 { format!("{}...\n[truncated at 3000 chars, total {} bytes]", &c[..3000], c.len()) }
            else { c }
        }
        Err(e) => format!("Error: {}", e),
    }
}

async fn tool_cargo_check(args: &serde_json::Value, root: &str) -> String {
    let dir = args.get("dir").and_then(|v| v.as_str()).unwrap_or(".");
    let full = PathBuf::from(root).join(dir);
    let output = match Command::new("cargo").args(["check"]).current_dir(&full).output().await {
        Ok(o) => o,
        Err(e) => return format!("Error: {}", e),
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() { "cargo check: OK".to_string() }
    else { format!("cargo check FAILED:\n{}", &stderr[..stderr.len().min(2000)]) }
}

async fn tool_think_harder(args: &serde_json::Value, config: &AgentConfig) -> String {
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
    if query.is_empty() { return "Error: query required".to_string(); }

    let client = reqwest::Client::new();
    let mut results = Vec::new();

    // Nautivecs
    if let Ok(resp) = client.post(&config.nautivecs_url)
        .json(&serde_json::json!({"query": query, "top_k": 3}))
        .send().await {
        if let Ok(body) = resp.text().await {
            let t = if body.len() > 1500 { &body[..1500] } else { &body };
            results.push(format!("[nautivecs]: {}", t));
        }
    }

    // WSO
    if let Ok(resp) = client.post(&config.wso_url)
        .json(&serde_json::json!({"query": query, "max_results": 3}))
        .send().await {
        if let Ok(body) = resp.text().await {
            let t = if body.len() > 1500 { &body[..1500] } else { &body };
            results.push(format!("[web search]: {}", t));
        }
    }

    if results.is_empty() { "No results.".to_string() } else { results.join("\n\n") }
}

async fn tool_remember(args: &serde_json::Value, root: &str, nautivecs_url: &str) -> String {
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let tags = args.get("tags").and_then(|v| v.as_str()).unwrap_or("general");
    if content.is_empty() { return "Error: content required".to_string(); }

    let log_path = PathBuf::from(root).join("research_log/lessons_learned.md");
    if let Some(parent) = log_path.parent() { let _ = tokio::fs::create_dir_all(parent).await; }

    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let entry = format!("\n## [{}] {}\n{}\n", tags, ts, content);
    let existing = tokio::fs::read_to_string(&log_path).await.unwrap_or_default();
    let log_result = tokio::fs::write(&log_path, format!("{}{}", existing, entry)).await;

    let base = nautivecs_url.trim_end_matches("/query").trim_end_matches("/search");
    let add_url = format!("{}/add", base);
    let client = reqwest::Client::new();
    let nautivecs_status = match client
        .post(&add_url)
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

    match log_result {
        Ok(_) => format!("Remembered (tags: {}){}", tags, nautivecs_status),
        Err(e) => format!("Error: {}", e),
    }
}

async fn tool_run_command(args: &serde_json::Value, root: &str, safe_mode: bool) -> String {
    let cmd = args.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
    if cmd.is_empty() { return "Error: cmd required".to_string(); }

    // Always blocked
    let blocked = ["rm -rf /", "rm -rf /*", "dd if=", "mkfs", "> /dev/sd"];
    for p in &blocked {
        if cmd.contains(p) { return format!("BLOCKED: {}", cmd); }
    }

    // Safe mode: block file modification/deletion commands
    if safe_mode {
        let destructive = ["rm ", "rm\t", "mv ", "mv\t", "> ", ">> ", "truncate", "shred"];
        for p in &destructive {
            if cmd.contains(p) {
                return format!("[SAFE MODE] BLOCKED: '{}' — cannot modify/delete existing files in safe mode", cmd);
            }
        }
    }

    let output = match Command::new("bash").args(["-c", cmd]).current_dir(root).output().await {
        Ok(o) => o,
        Err(e) => return format!("Error: {}", e),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut r = String::new();
    if !stdout.is_empty() { r.push_str(&stdout[..stdout.len().min(2000)]); }
    if !stderr.is_empty() { r.push_str(&format!("\n[stderr]: {}", &stderr[..stderr.len().min(500)])); }
    if r.is_empty() { format!("Command completed (exit code: {})", output.status.code().unwrap_or(-1)) }
    else { r }
}
