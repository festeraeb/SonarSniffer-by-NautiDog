use axum::{
    extract::{Json, State},
    response::Html,
    routing::{get, post},
    Router,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::path::Path;
use std::process::Command;
use tokio::sync::Mutex;
use tracing::{info, warn};

// ═══════════════════════════════════════════════════════════════════════════════
// State
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Clone)]
struct AppState {
    conversation: Arc<Mutex<String>>,
    kobold_url: String,
    thinker_url: String,
    nautivecs_url: String,
    wso_url: String,
    project_root: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// System Prompt
// ═══════════════════════════════════════════════════════════════════════════════

const SYSTEM_PROMPT: &str = r#"<|im_start|>system
You are the CESARops Forge — an autonomous developer agent on dual P100 GPUs.

You do NOT just talk. You ACT using tools.

TOOL FORMAT: To execute a command, output EXACTLY:
<tool_call>
{"name": "TOOL_NAME", "arguments": {"key": "value"}}
</tool_call>

After outputting </tool_call>, you MUST stop generating and wait for the result.

AVAILABLE TOOLS:
- write_file(path, content): Write code to the project at /codebase/wreckhunter2000-1/
- read_file(path): Read existing code
- cargo_check(project_dir): Run cargo check to verify code compiles
- think_harder(query): Search codebase + internet when uncertain (3-8 word query)
- remember(content, tags): Save a lesson to the vector DB for future reference
- run_command(command): Execute a shell command (nvidia-smi, ls, cat, etc.)

WORKFLOW:
1. When asked to implement: FIRST call think_harder to find patterns
2. Then write_file to create code
3. Then cargo_check to verify
4. If it fails, fix and write again
5. After solving something hard, call remember

RULES:
- ALWAYS use tools to modify files. Never just output code in text.
- ALWAYS cargo_check after writing.
- If uncertain, think_harder BEFORE guessing.
- Keep think_harder queries short (3-8 words).
- NEVER hallucinate file paths or claim success without cargo_check proof.
- One tool call per response. Wait for the result before the next action.
<|im_end|>
"#;

// ═══════════════════════════════════════════════════════════════════════════════
// Synthetic Tool Loop
// ═══════════════════════════════════════════════════════════════════════════════

async fn generate_step(kobold_url: &str, prompt: &str) -> String {
    generate_step_with_temp(kobold_url, prompt, 0.3).await
}

async fn generate_step_with_temp(kobold_url: &str, prompt: &str, temperature: f32) -> String {
    let client = reqwest::Client::new();
    // Widen top_p when temperature is spiked (allows long-tail options like read_file)
    let top_p = if temperature > 0.5 { 0.95 } else { 0.9 };
    let resp = client
        .post(format!("{}/api/v1/generate", kobold_url))
        .json(&serde_json::json!({
            "prompt": prompt,
            "max_length": 4096,
            "temperature": temperature,
            "top_p": top_p,
            "stop_sequence": ["</tool_call>", "<|im_end|>", "\n\n\n"],
            "rep_pen": 1.2
        }))
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await;

    match resp {
        Ok(r) => {
            let body = r.text().await.unwrap_or_default();
            let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let mut text = parsed["results"][0]["text"]
                .as_str()
                .unwrap_or("")
                .to_string();

            // If model stopped at </tool_call> (stop sequence), re-append for parser
            if text.contains("<tool_call>") && !text.contains("</tool_call>") {
                text.push_str("</tool_call>");
            }
            text
        }
        Err(e) => format!("[LLM Error: {}]", e),
    }
}

fn parse_tool_call(output: &str) -> (String, Option<serde_json::Value>) {
    let re = Regex::new(r"(?s)(.*?)<tool_call>\s*(.*?)\s*</tool_call>").unwrap();
    if let Some(cap) = re.captures(output) {
        let thinking = cap.get(1).map_or("", |m| m.as_str()).trim().to_string();
        let json_str = cap.get(2).map_or("", |m| m.as_str());
        let action = serde_json::from_str(json_str).ok();
        (thinking, action)
    } else {
        (output.to_string(), None)
    }
}

/// Strip <think>...</think> blocks from model output — show only the final answer.
fn strip_think_blocks(text: &str) -> String {
    let re = Regex::new(r"(?s)<think>.*?</think>\s*").unwrap();
    let cleaned = re.replace_all(text, "").to_string();
    if cleaned.trim().is_empty() {
        // If stripping think blocks leaves nothing, return the think content instead
        let think_re = Regex::new(r"(?s)<think>(.*?)</think>").unwrap();
        if let Some(cap) = think_re.captures(text) {
            return format!("[Thinking]: {}", cap.get(1).map_or("", |m| m.as_str()).trim());
        }
        text.to_string()
    } else {
        cleaned.trim().to_string()
    }
}

async fn execute_tool(name: &str, args: &serde_json::Value, state: &AppState) -> String {
    match name {
        "write_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            let full_path = format!("{}/{}", state.project_root, path);

            if let Some(parent) = Path::new(&full_path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&full_path, content) {
                Ok(_) => format!("OK: wrote {} bytes to {}", content.len(), path),
                Err(e) => format!("ERROR: {}", e),
            }
        }
        "read_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let full_path = format!("{}/{}", state.project_root, path);
            match std::fs::read_to_string(&full_path) {
                Ok(content) => {
                    let len = content.len();
                    if len > 3000 {
                        format!("{}...\n[truncated, {} bytes total]", &content[..3000], len)
                    } else {
                        content
                    }
                }
                Err(e) => format!("ERROR: {}", e),
            }
        }
        "cargo_check" => {
            let dir = args["project_dir"].as_str().unwrap_or(&state.project_root);
            let output = Command::new("cargo")
                .args(["check", "--message-format=json"])
                .current_dir(dir)
                .output();

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let mut errors: Vec<String> = Vec::new();
                    for line in stdout.lines() {
                        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                            if msg.get("reason").and_then(|r| r.as_str()) == Some("compiler-message") {
                                if let Some(m) = msg.get("message") {
                                    let level = m.get("level").and_then(|l| l.as_str()).unwrap_or("");
                                    let text = m.get("message").and_then(|t| t.as_str()).unwrap_or("");
                                    if level == "error" {
                                        errors.push(text.to_string());
                                    }
                                }
                            }
                        }
                    }
                    if errors.is_empty() {
                        "OK: cargo check passed — no errors".to_string()
                    } else {
                        let error_list = errors.iter().take(5).cloned().collect::<Vec<_>>().join("\n");
                        format!("FAILED ({} errors):\n{}", errors.len(), error_list)
                    }
                }
                Err(e) => format!("ERROR: {}", e),
            }
        }
        "think_harder" => {
            let query = args["query"].as_str().unwrap_or("");
            let client = reqwest::Client::new();
            let mut results = String::new();

            // nautivecs
            if let Ok(resp) = client
                .post(&state.nautivecs_url)
                .json(&serde_json::json!({"query": query, "top_k": 3}))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                if let Ok(body) = resp.text().await {
                    results.push_str("=== Codebase ===\n");
                    results.push_str(&body[..body.len().min(1500)]);
                    results.push('\n');
                }
            }

            // WSO
            if let Ok(resp) = client
                .post(&state.wso_url)
                .json(&serde_json::json!({"query": query, "max_results": 3}))
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
            {
                if let Ok(body) = resp.text().await {
                    results.push_str("=== Web ===\n");
                    results.push_str(&body[..body.len().min(1500)]);
                }
            }

            if results.is_empty() { "No results found.".to_string() } else { results }
        }
        "remember" => {
            let content = args["content"].as_str().unwrap_or("");
            let tags = args["tags"].as_str().unwrap_or("general");
            let lessons_path = format!("{}/research_log/lessons_learned.md", state.project_root);
            let _ = std::fs::create_dir_all(format!("{}/research_log", state.project_root));

            let entry = format!("\n## [{}]\n{}\n", tags, content);
            match std::fs::OpenOptions::new().create(true).append(true).open(&lessons_path) {
                Ok(mut f) => {
                    use std::io::Write;
                    let _ = f.write_all(entry.as_bytes());
                    format!("Remembered with tags [{}]", tags)
                }
                Err(e) => format!("ERROR: {}", e),
            }
        }
        "run_command" => {
            let cmd = args["command"].as_str().unwrap_or("");
            if cmd.contains("rm -rf") || cmd.contains("dd if=") {
                return "BLOCKED: dangerous command".to_string();
            }
            match Command::new("bash").args(["-c", cmd]).current_dir(&state.project_root).output() {
                Ok(out) => {
                    let combined = format!("{}{}", 
                        String::from_utf8_lossy(&out.stdout),
                        String::from_utf8_lossy(&out.stderr));
                    if combined.len() > 2000 {
                        format!("{}...[truncated]", &combined[..2000])
                    } else {
                        combined
                    }
                }
                Err(e) => format!("ERROR: {}", e),
            }
        }
        _ => format!("Unknown tool: {}", name),
    }
}

/// The main synthetic tool-calling loop.
/// DEFAULT: Hits the 8B Thinker (DeepSeek-R1) first for reasoning spark,
/// then feeds that into the 35B Coder for execution.
/// Use /fast prefix to skip the thinker and go direct to 35B.
async fn run_tool_loop(state: &AppState, user_message: &str) -> (String, Vec<String>) {
    let mut prompt = state.conversation.lock().await.clone();

    // Check for /fast flag — skip thinker if present
    let (skip_thinker, clean_message) = if user_message.starts_with("/fast ") {
        (true, user_message.strip_prefix("/fast ").unwrap_or(user_message))
    } else {
        (false, user_message)
    };

    // Step 0: Hit the 8B Thinker first (unless /fast)
    let thinker_context = if !skip_thinker {
        info!("Thinker pass (8B DeepSeek-R1 on 1070)...");
        get_thinker_spark(&state.thinker_url, clean_message).await
    } else {
        info!("Skipping thinker (/fast mode)");
        String::new()
    };

    // Add user message in ChatML format, with thinker context if available
    if !thinker_context.is_empty() {
        prompt.push_str(&format!(
            "<|im_start|>user\n[Thinker Analysis (8B R1)]: {}\n\nUser Request: {}<|im_end|>\n<|im_start|>assistant\n<think>\n</think>\n",
            thinker_context, clean_message
        ));
    } else {
        prompt.push_str(&format!("<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n<think>\n</think>\n", clean_message));
    }

    let mut tool_actions: Vec<String> = Vec::new();
    let mut final_response = String::new();
    let max_rounds = 12;
    let mut repeat_count: u32 = 0;
    let mut diagnosis_count: u32 = 0;
    let max_diagnoses: u32 = 2;
    let mut last_output: String = String::new();

    for round in 0..max_rounds {
        info!("Tool loop round {}", round + 1);

        // Temperature spike if we detected a repeat last round
        let temp = if repeat_count > 0 { 0.7 } else { 0.3 };
        let output = generate_step_with_temp(&state.kobold_url, &prompt, temp).await;

        if output.starts_with("[LLM Error") {
            final_response = output;
            break;
        }

        // N-gram detection: if the raw output is the same as last round, hard reset
        if !last_output.is_empty() && output == last_output {
            info!("N-gram loop detected — identical output twice. Hard resetting context.");
            // Strip everything after the system prompt and start fresh
            let system_end = prompt.find("<|im_end|>").unwrap_or(0) + 10;
            prompt.truncate(system_end);
            prompt.push_str(&format!(
                "\n<|im_start|>user\n[CONTEXT RESET - Previous attempt looped]\nOriginal request: {}<|im_end|>\n<|im_start|>assistant\n<think>\n</think>\n",
                clean_message
            ));
            last_output.clear();
            continue;
        }
        last_output = output.clone();

        let (thinking, action) = parse_tool_call(&output);

        if let Some(action_val) = action {
            let tool_name = action_val["name"].as_str().unwrap_or("unknown");
            let tool_args = action_val.get("arguments").cloned().unwrap_or_default();
            let call_sig = format!("{}:{}", tool_name, tool_args.to_string());

            // Detect repeated tool calls — break the loop
            if tool_actions.last().map(|s| s.as_str()) == Some(&call_sig) {
                repeat_count += 1;
                if repeat_count >= 2 {
                    info!("Detected repeated tool call x{} — forcing summary", repeat_count);
                    prompt.push_str(&thinking);
                    prompt.push_str(&format!(
                        "\n\n[SYSTEM CONSTRAINT - Round {}/{}]: You have called the SAME tool {} times. This is FORBIDDEN. \
                        You MUST NOT call '{}' again. You have enough information. \
                        Synthesize what you've learned and provide your complete assessment NOW. \
                        Use read_file if you need to examine specific source files, or just summarize.\n",
                        round + 1, max_rounds, repeat_count + 1, tool_name
                    ));
                    let summary = generate_step_with_temp(&state.kobold_url, &prompt, 0.5).await;
                    let (summary_text, _) = parse_tool_call(&summary);
                    final_response = strip_think_blocks(&summary_text);
                    break;
                }
            } else {
                repeat_count = 0;
            }

            info!("Tool call: {}({:?})", tool_name, &tool_args.to_string()[..tool_args.to_string().len().min(80)]);
            tool_actions.push(call_sig);

            let result = execute_tool(tool_name, &tool_args, state).await;
            info!("Result: {}...", &result[..result.len().min(100)]);

            // Append thinking + tool call + result with round counter
            prompt.push_str(&thinking);
            prompt.push_str(&format!("\n<tool_call>\n{}\n</tool_call>\n", serde_json::to_string(&action_val).unwrap_or_default()));
            prompt.push_str(&format!(
                "<tool_result>[Round {}/{}] {}\n</tool_result>\n",
                round + 1, max_rounds, result
            ));
        } else {
            // No tool call found — check if it's a think-only response
            let stripped = strip_think_blocks(&thinking);
            if stripped.is_empty() || stripped.len() < 30 {
                diagnosis_count += 1;

                // Max diagnosis cap — after 2 failed attempts, hard reset
                if diagnosis_count > max_diagnoses {
                    info!("Max diagnoses ({}) reached — hard resetting", max_diagnoses);
                    let system_end = prompt.find("<|im_end|>").unwrap_or(0) + 10;
                    prompt.truncate(system_end);
                    prompt.push_str(&format!(
                        "\n<|im_start|>user\n[HARD RESET after {} failed diagnoses]\n\
                        The model kept producing only internal reasoning. Respond DIRECTLY to: {}\n\
                        Do NOT think. Just answer or call a tool.<|im_end|>\n<|im_start|>assistant\n",
                        max_diagnoses, clean_message
                    ));
                    let reset_output = generate_step_with_temp(&state.kobold_url, &prompt, 0.7).await;
                    let (reset_text, reset_action) = parse_tool_call(&reset_output);
                    if let Some(action_val) = reset_action {
                        let tool_name = action_val["name"].as_str().unwrap_or("unknown");
                        let tool_args = action_val.get("arguments").cloned().unwrap_or_default();
                        tool_actions.push(format!("{}:{}", tool_name, tool_args.to_string()));
                        let result = execute_tool(tool_name, &tool_args, state).await;
                        prompt.push_str(&reset_output);
                        prompt.push_str(&format!("<tool_result>\n{}\n</tool_result>\n", result));
                        continue;
                    }
                    final_response = strip_think_blocks(&reset_text);
                    if final_response.is_empty() {
                        final_response = "[FAILED: Model cannot exit thinking mode after hard reset. This requires Burn inference to fix at the logit level.]".to_string();
                    }
                    break;
                }

                // DIAGNOSTIC: Send the failed exchange to the 8B thinker
                info!("Think-only response (diagnosis {}/{}) — sending to 8B", diagnosis_count, max_diagnoses);
                
                let diagnostic_prompt = format!(
                    "The 35B model was given a prompt with tool results but only produced think tags with no actual response or tool call. \
                    Last part of what was sent:\n{}\n\n\
                    What came back (raw):\n{}\n\n\
                    Why did the model fail to produce content? Is the tool_result format confusing it? \
                    What EXACT format should tool results be in so the model responds properly? Keep answer under 100 words.",
                    &prompt[prompt.len().saturating_sub(800)..],
                    &output[..output.len().min(400)]
                );
                
                let diagnosis = get_thinker_spark(&state.thinker_url, &diagnostic_prompt).await;
                
                if !diagnosis.is_empty() {
                    info!("8B diagnosis: {}...", &diagnosis[..diagnosis.len().min(150)]);
                    prompt.push_str(&output);
                    prompt.push_str(&format!(
                        "\n[DIAGNOSTIC from 8B reviewer]: {}\n\
                        Based on this feedback, provide your response NOW. Either call a tool or give your answer.\n",
                        &diagnosis[..diagnosis.len().min(300)]
                    ));
                } else {
                    prompt.push_str(&output);
                    prompt.push_str("\n[SYSTEM]: Respond with a <tool_call> or plain text answer NOW.\n");
                }
                
                let nudged = generate_step_with_temp(&state.kobold_url, &prompt, 0.6).await;
                let (nudged_thinking, nudged_action) = parse_tool_call(&nudged);
                
                if let Some(action_val) = nudged_action {
                    let tool_name = action_val["name"].as_str().unwrap_or("unknown");
                    let tool_args = action_val.get("arguments").cloned().unwrap_or_default();
                    info!("Diagnosed + nudged into tool call: {}", tool_name);
                    tool_actions.push(format!("{}:{}", tool_name, tool_args.to_string()));
                    let result = execute_tool(tool_name, &tool_args, state).await;
                    prompt.push_str(&nudged);
                    prompt.push_str(&format!("<tool_result>\n{}\n</tool_result>\n", result));

                    // AUTO-REMEMBER: The diagnosis worked! Save the lesson.
                    if !diagnosis.is_empty() {
                        info!("Diagnosis succeeded — auto-remembering the fix");
                        let lesson = format!(
                            "When 35B produces only <think> blocks after tool results, the 8B diagnosed: '{}'. \
                            This nudge successfully produced a {} tool call.",
                            &diagnosis[..diagnosis.len().min(200)], tool_name
                        );
                        let remember_args = serde_json::json!({"content": lesson, "tags": "35b-fix,think-loop,diagnosis"});
                        let _ = execute_tool("remember", &remember_args, state).await;
                    }
                    continue;
                } else {
                    final_response = strip_think_blocks(&nudged_thinking);
                    if final_response.is_empty() {
                        final_response = thinking.replace("<think>", "").replace("</think>", "").trim().to_string();
                        if !final_response.is_empty() {
                            final_response = format!("[Model reasoning]: {}", &final_response[..final_response.len().min(2000)]);
                        } else {
                            final_response = "[No response — model stuck in thinking mode. Moving to Burn inference will fix this permanently.]".to_string();
                        }
                    }
                    break;
                }
            } else {
                // Got actual content — use it
                final_response = stripped;
                prompt.push_str(&final_response);
                break;
            }
        }
    }

    // Close the assistant turn
    prompt.push_str("<|im_end|>\n");

    // Save conversation state
    let mut conv = state.conversation.lock().await;
    *conv = prompt;

    // Truncate conversation if too long (keep system prompt + last exchanges)
    if conv.len() > 50000 {
        let system_end = conv.find("<|im_end|>").unwrap_or(0) + 10;
        let keep_from = conv.len() - 30000;
        let new_conv = format!("{}{}", &conv[..system_end], &conv[keep_from..]);
        *conv = new_conv;
    }

    let tool_actions_display: Vec<String> = tool_actions.iter()
        .map(|s| {
            let name = s.split(':').next().unwrap_or(s);
            format!("{}()", name)
        })
        .collect();

    (final_response, tool_actions_display)
}

/// Hit the 8B DeepSeek-R1 on the 1070 for a reasoning "spark" before the 35B executes.
/// The thinker approaches problems from different angles than the MoE coder.
async fn get_thinker_spark(thinker_url: &str, message: &str) -> String {
    let client = reqwest::Client::new();
    let prompt = format!(
        "<|im_start|>system\nYou are a senior architect. Think step-by-step about this request. \
        What should the coder search for? What patterns apply? What could go wrong? \
        Keep your analysis under 200 words. Be specific and actionable.<|im_end|>\n\
        <|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
        message
    );

    let resp = client
        .post(format!("{}/api/v1/generate", thinker_url))
        .json(&serde_json::json!({
            "prompt": prompt,
            "max_length": 512,
            "temperature": 0.4,
            "stop_sequence": ["<|im_end|>", "\n\n\n"]
        }))
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await;

    match resp {
        Ok(r) => {
            let body = r.text().await.unwrap_or_default();
            let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let text = parsed["results"][0]["text"].as_str().unwrap_or("").to_string();
            let cleaned = strip_think_blocks(&text);
            info!("Thinker spark: {}...", &cleaned[..cleaned.len().min(100)]);
            cleaned
        }
        Err(e) => {
            warn!("Thinker unavailable ({}), proceeding without spark", e);
            String::new()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HTTP Handlers
// ═══════════════════════════════════════════════════════════════════════════════

async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

async fn health() -> &'static str {
    r#"{"status":"ok","service":"cesarops-forge","mode":"synthetic-tool-calling"}"#
}

#[derive(Deserialize)]
struct SendRequest {
    message: String,
}

#[derive(Serialize)]
struct SendResponse {
    response: String,
    tool_actions: Vec<String>,
    thinking: String,
}

async fn send_message(
    State(state): State<AppState>,
    Json(req): Json<SendRequest>,
) -> Json<SendResponse> {
    info!("User: {}", &req.message[..req.message.len().min(100)]);

    let (response, tool_actions) = run_tool_loop(&state, &req.message).await;

    info!("Response: {}... (tools: {:?})", &response[..response.len().min(80)], tool_actions);

    Json(SendResponse {
        response: response.clone(),
        tool_actions,
        thinking: String::new(),
    })
}

async fn clear(State(state): State<AppState>) -> &'static str {
    let mut conv = state.conversation.lock().await;
    *conv = SYSTEM_PROMPT.to_string();
    "Conversation cleared"
}

// ═══════════════════════════════════════════════════════════════════════════════
// Main
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("cesarops_forge_web=info")
        .init();

    let state = AppState {
        conversation: Arc::new(Mutex::new(SYSTEM_PROMPT.to_string())),
        kobold_url: "http://127.0.0.1:5001".to_string(),
        thinker_url: "http://100.102.158.111:5555".to_string(),
        nautivecs_url: "http://127.0.0.1:5003/query".to_string(),
        wso_url: "http://127.0.0.1:5010/search".to_string(),
        project_root: "/codebase/wreckhunter2000-1".to_string(),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/send", post(send_message))
        .route("/clear", post(clear))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 9100));
    info!("cesarops-forge (synthetic tool-calling) on {}", addr);
    info!("Tools: write_file, read_file, cargo_check, think_harder, remember, run_command");
    info!("Stop sequences: </tool_call>, <|im_end|>");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
