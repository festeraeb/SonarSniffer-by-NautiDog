use crate::diagnostics;
use crate::memory;
use crate::prompts;
use crate::tools;
use crate::translator::{self, FailureType, Message};
use crate::{AppState, SendResponse};
use tracing::{info, warn};

const MAX_TOOL_ROUNDS: u32 = u32::MAX; // No limit — only loops/empty stop it

/// Read tuning params from cluster_config.toml
fn read_tuning() -> (u32, u64, u64, u32, f32, bool) {
    let config_path = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";
    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    
    let tuning = table.get("tuning");
    let max_think = tuning.and_then(|t| t.get("max_think_rounds")).and_then(|v| v.as_integer()).unwrap_or(20) as u32;
    let gen_timeout = tuning.and_then(|t| t.get("generation_timeout_secs")).and_then(|v| v.as_integer()).unwrap_or(600) as u64;
    let thinker_timeout = tuning.and_then(|t| t.get("thinker_timeout_secs")).and_then(|v| v.as_integer()).unwrap_or(60) as u64;
    let max_tokens = tuning.and_then(|t| t.get("max_generation_tokens")).and_then(|v| v.as_integer()).unwrap_or(12288) as u32;
    let temperature = tuning.and_then(|t| t.get("temperature")).and_then(|v| v.as_float()).unwrap_or(0.4) as f32;
    let skip_corrector = tuning.and_then(|t| t.get("skip_corrector")).and_then(|v| v.as_bool()).unwrap_or(false);
    
    (max_think, gen_timeout, thinker_timeout, max_tokens, temperature, skip_corrector)
}

/// Main orchestration loop: Strategy → Execution → Verification.
pub async fn run(state: &AppState, user_message: &str) -> SendResponse {
    let fast_mode = user_message.starts_with("/fast");
    let message = if fast_mode {
        user_message.trim_start_matches("/fast").trim()
    } else {
        user_message
    };

    // --- Layer 1: Thinker pre-flight (skip with /fast) ---
    let thinker_context = if !fast_mode {
        get_thinker_preflight(message, state).await
    } else {
        String::new()
    };

    // Add user message to conversation
    {
        let mut conv = state.conversation.lock().await;
        conv.push(Message {
            role: "user".to_string(),
            content: message.to_string(),
        });
    }

    // --- Layer 3: Execute generation loop ---
    let mut tool_actions: Vec<String> = Vec::new();
    let mut output_history: Vec<String> = Vec::new();
    let mut failure_count: u32 = 0;
    let mut diagnosis_info: Option<String> = None;
    let (max_diagnosis, _gen_timeout, _thinker_timeout, _max_tokens, start_temp, skip_corrector) = read_tuning();
    let mut temperature = start_temp;

    for round in 1..=MAX_TOOL_ROUNDS {
        // Check interrupt flag
        if state.interrupt.load(std::sync::atomic::Ordering::Relaxed) {
            info!("INTERRUPTED at round {}", round);
            state.interrupt.store(false, std::sync::atomic::Ordering::Relaxed);
            return SendResponse {
                response: "[INTERRUPTED by user]".to_string(),
                tool_actions,
                diagnosis: diagnosis_info,
            };
        }

        // Check for steering messages injected between rounds
        {
            let mut steering = state.steering.lock().await;
            if !steering.is_empty() {
                let mut conv = state.conversation.lock().await;
                for msg in steering.drain(..) {
                    info!("Injecting steering: {}", &msg[..msg.len().min(60)]);
                    conv.push(Message {
                        role: "user".to_string(),
                        content: format!("[STEERING FROM OPERATOR]: {}", msg),
                    });
                }
            }
        }

        // Build prompt from conversation state
        let prompt = build_prompt(state, &thinker_context, failure_count).await;

        // Generate from 35B
        let raw_output = generate_35b(&prompt, temperature, state).await;

        // --- Layer 2: Translator normalizes output ---
        let normalized = translator::normalize(&raw_output);

        // Loop detection
        if translator::detect_output_loop(&output_history, &raw_output) {
            warn!("Loop detected at round {}", round);
            // Hard reset recent history
            hard_reset(state).await;
            failure_count += 1;
            temperature = (temperature + 0.15).min(1.0);
            continue;
        }
        output_history.push(raw_output.clone());

        // Handle failure states
        if let Some(ref failure_type) = normalized.failure {
            warn!("Failure detected: {:?} at round {}", failure_type, round);

            // --- MalformedToolCall: Route to 14B Corrector (Marvin) ---
            if matches!(failure_type, FailureType::MalformedToolCall) {
                if skip_corrector {
                    info!("Corrector disabled (skip_corrector=true). Treating as parse failure.");
                    failure_count += 1;
                    temperature = (temperature + 0.1).min(1.0);
                    continue;
                }
                info!("Routing malformed tool call to 14B Corrector (Marvin)");
                let corrected = correct_and_execute(&raw_output, &normalized.content, state).await;
                
                if let Some((tool_name, result, correction_note)) = corrected {
                    tool_actions.push(format!("{}(corrected)", tool_name));
                    
                    // Feed result back to 35B WITH the correction
                    let feedback = format!(
                        "{}\n\n[CORRECTION]: {}\nPattern: {{\"name\": \"TOOL_NAME\", \"arguments\": {{\"key\": \"value\"}}}}",
                        result, correction_note
                    );
                    let formatted = translator::format_tool_result_for_qwen(&feedback, round, MAX_TOOL_ROUNDS);
                    
                    let mut conv = state.conversation.lock().await;
                    conv.push(Message {
                        role: "assistant".to_string(),
                        content: normalized.content.clone(),
                    });
                    conv.push(Message {
                        role: "user".to_string(),
                        content: formatted,
                    });
                    
                    // Save correct format to nautivecs for future injection
                    let remember_args = serde_json::json!({
                        "content": format!("Correct tool call format: {{\"name\": \"{}\", \"arguments\": {{...}}}}. The 35B produced malformed JSON. Fix applied by corrector.", tool_name),
                        "tags": "tool_call,format_fix,corrector"
                    });
                    let _ = tools::execute("remember", &remember_args, state).await;
                    
                    continue;
                }
                // If corrector couldn't fix it either, fall through to diagnostics
            }

            if failure_count >= max_diagnosis {
                // Give up — return whatever we have
                let response = if normalized.content.is_empty() {
                    format!("[Model stuck after {} attempts. Last failure: {:?}]", failure_count, failure_type)
                } else {
                    normalized.content
                };

                return SendResponse {
                    response,
                    tool_actions,
                    diagnosis: diagnosis_info,
                };
            }

            // --- Layer 4: Diagnostic ---
            let prompt_tail = get_prompt_tail(state).await;
            let diag = diagnostics::diagnose(
                &prompt_tail,
                &raw_output,
                failure_type,
                state,
            )
            .await;

            if let Some(ref diagnosis) = diag {
                diagnosis_info = Some(diagnosis.explanation.clone());
                info!("Diagnosis: {} (confidence: {:.2})", diagnosis.explanation, diagnosis.confidence);

                // Apply prompt override if provided
                if let Some(ref override_text) = diagnosis.prompt_override {
                    let mut conv = state.conversation.lock().await;
                    conv.push(Message {
                        role: "user".to_string(),
                        content: format!("[System correction]: {}", override_text),
                    });
                }

                // Temperature spike on failure
                temperature = (temperature + 0.15).min(0.9);
            }

            failure_count += 1;
            continue;
        }

        // --- Tool call handling ---
        if let Some(ref tool_call) = normalized.tool_call {
            info!("Tool call round {}: {}", round, tool_call.name);
            tool_actions.push(tool_call.name.clone());

            // Check for repeated tool calls — demand justification, terminate after 4
            if tool_actions.len() >= 3 {
                let last_three = &tool_actions[tool_actions.len() - 3..];
                if last_three.iter().all(|a| a == &last_three[0]) {
                    let repeat_count = tool_actions.iter().rev()
                        .take_while(|a| *a == &tool_call.name)
                        .count();

                    let msg = match repeat_count {
                        3 => format!("[System]: You've called '{}' 3 times in a row. State WHY you need it again in your next response, then call it. If you cannot justify it, use a different tool.", tool_call.name),
                        4 => format!("[System — FINAL WARNING]: '{}' called 4 times. You MUST either use a DIFFERENT tool or provide your final answer NOW. Next repetition = termination.", tool_call.name),
                        _ => {
                            // 5+ = terminate
                            warn!("Terminated: {} called {}x with no progress", tool_call.name, repeat_count);
                            let mut conv = state.conversation.lock().await;
                            conv.push(Message {
                                role: "user".to_string(),
                                content: format!("[TERMINATED]: '{}' repeated {} times. Provide your answer in plain text NOW. No more tool calls.", tool_call.name, repeat_count),
                            });
                            continue;
                        }
                    };

                    let mut conv = state.conversation.lock().await;
                    conv.push(Message {
                        role: "user".to_string(),
                        content: msg,
                    });
                }
            }

            // Execute the tool
            let result = tools::execute(&tool_call.name, &tool_call.arguments, state).await;

            // --- Context Window Management ---
            // If the result is large, compress it for conversation and store full in nautivecs
            let (conv_result, stored_full) = compress_tool_result(&tool_call.name, &tool_call.arguments, &result, state).await;

            // Format result as user message (QwenChatML format)
            let formatted = translator::format_tool_result_for_qwen(&conv_result, round, MAX_TOOL_ROUNDS);

            // Trim old conversation to keep context lean (keep system + last 6 exchanges)
            let mut conv = state.conversation.lock().await;
            trim_conversation(&mut conv, 12); // keep last 12 messages (6 exchanges)

            conv.push(Message {
                role: "assistant".to_string(),
                content: normalized.content.clone(),
            });
            conv.push(Message {
                role: "user".to_string(),
                content: formatted,
            });

            // If diagnosis was used and this succeeded, auto-remember
            if failure_count > 0 {
                if let Some(ref diag) = diagnosis_info {
                    let diag_result = diagnostics::DiagnosisResult {
                        explanation: diag.clone(),
                        prompt_override: None,
                        confidence: 0.8,
                    };
                    let ft = FailureType::ThinkOnly; // approximate
                    memory::auto_remember_success(&diag_result, &ft, "qwen3.6-35b", state).await;
                }
                failure_count = 0; // Reset on success
            }

            continue;
        }

        // --- Plain text response (no tool call, no failure) ---
        // Add to conversation and return
        {
            let mut conv = state.conversation.lock().await;
            conv.push(Message {
                role: "assistant".to_string(),
                content: normalized.content.clone(),
            });
        }

        // Auto-remember success after diagnosis recovery
        if failure_count > 0 {
            if let Some(ref diag) = diagnosis_info {
                let diag_result = diagnostics::DiagnosisResult {
                    explanation: diag.clone(),
                    prompt_override: None,
                    confidence: 0.8,
                };
                let ft = FailureType::ThinkOnly;
                memory::auto_remember_success(&diag_result, &ft, "qwen3.6-35b", state).await;
            }
        }

        return SendResponse {
            response: normalized.content,
            tool_actions,
            diagnosis: diagnosis_info,
        };
    }

    // Exhausted all rounds
    SendResponse {
        response: format!("[Exhausted {} tool rounds. Last actions: {:?}]", MAX_TOOL_ROUNDS, tool_actions),
        tool_actions,
        diagnosis: diagnosis_info,
    }
}

/// Call the 8B thinker for pre-flight reasoning.
async fn get_thinker_preflight(message: &str, state: &AppState) -> String {
    let prompt = prompts::thinker_prompt(message);
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "prompt": prompt,
        "max_length": 900,
        "temperature": 0.5,
        "top_p": 0.9,
        "stop_sequence": ["<|im_end|>"],
    });

    let resp = client
        .post(format!("{}/api/v1/generate", state.config.thinker_url))
        .json(&payload)
        .timeout(std::time::Duration::from_secs(90))
        .send()
        .await;

    match resp {
        Ok(r) => {
            if let Ok(body) = r.json::<serde_json::Value>().await {
                body.get("results")
                    .and_then(|r| r.as_array())
                    .and_then(|a| a.first())
                    .and_then(|r| r.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            }
        }
        Err(e) => {
            warn!("Thinker preflight failed: {}", e);
            String::new()
        }
    }
}

/// Build the full prompt from conversation state.
async fn build_prompt(state: &AppState, thinker_context: &str, failure_count: u32) -> String {
    let conv = state.conversation.lock().await;

    let system = format!(
        "{}{}{}",
        prompts::system_prompt(),
        if !thinker_context.is_empty() {
            prompts::format_thinker_context(thinker_context)
        } else {
            String::new()
        },
        prompts::snark_nudge(failure_count),
    );

    let messages: Vec<(String, String)> = conv
        .iter()
        .map(|m| (m.role.clone(), m.content.clone()))
        .collect();

    prompts::format_chatml(&system, &messages, true)
}

/// Generate from the 35B via KoboldCPP.
async fn generate_35b(prompt: &str, temperature: f32, state: &AppState) -> String {
    let client = reqwest::Client::new();

    // Fresh token budget every call — context is trimmed between rounds
    // so we always have room for a full generation
    let payload = serde_json::json!({
        "prompt": prompt,
        "max_length": 16384,
        "temperature": temperature,
        "top_p": 0.95,
        "rep_pen": 1.1,
        "stop_sequence": ["</tool_call>", "<|endoftext|>"],
    });

    let resp = client
        .post(format!("{}/api/v1/generate", state.config.coder_url))
        .json(&payload)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await;

    match resp {
        Ok(r) => {
            if let Ok(body) = r.json::<serde_json::Value>().await {
                let text = body
                    .get("results")
                    .and_then(|r| r.as_array())
                    .and_then(|a| a.first())
                    .and_then(|r| r.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");

                // If stopped at </tool_call>, re-append it so the parser can find it
                if !text.contains("</tool_call>") && text.contains("<tool_call>") {
                    format!("{}</tool_call>", text)
                } else {
                    text.to_string()
                }
            } else {
                String::new()
            }
        }
        Err(e) => {
            warn!("35B generation failed: {}", e);
            String::new()
        }
    }
}

/// Get the tail of the prompt for diagnostic context.
async fn get_prompt_tail(state: &AppState) -> String {
    let conv = state.conversation.lock().await;
    let tail: Vec<String> = conv
        .iter()
        .rev()
        .take(3)
        .map(|m| format!("[{}]: {}", m.role, &m.content[..m.content.len().min(200)]))
        .collect();
    tail.join("\n")
}

/// Hard reset: clear recent conversation history (keep system + first user message).
async fn hard_reset(state: &AppState) {
    let mut conv = state.conversation.lock().await;
    if conv.len() > 2 {
        // Keep only the last user message
        let last_user = conv
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .cloned();
        conv.clear();
        if let Some(msg) = last_user {
            conv.push(msg);
        }
    }
    info!("Hard reset: conversation cleared to last user message");
}

/// Route a malformed tool call to the 14B Corrector (Marvin) on the 1070.
/// The corrector fixes the JSON, we execute the tool, and return the result + correction note.
async fn correct_and_execute(
    raw_output: &str,
    stripped_content: &str,
    state: &AppState,
) -> Option<(String, String, String)> {
    let client = reqwest::Client::new();

    // Ask the 14B to fix the malformed tool call
    let fix_prompt = format!(
        r#"<|im_start|>system
You are a tool-call JSON fixer. The main model produced malformed JSON for a tool call.
Fix it and output ONLY the corrected JSON. Nothing else.

Available tools: write_file, read_file, cargo_check, think_harder, remember, run_command

Correct format: {{"name": "tool_name", "arguments": {{"key": "value"}}}}
<|im_end|>
<|im_start|>user
Fix this malformed tool call:
{}
<|im_end|>
<|im_start|>assistant
"#,
        stripped_content
    );

    let payload = serde_json::json!({
        "prompt": fix_prompt,
        "max_length": 600,
        "temperature": 0.1,
        "top_p": 0.9,
        "stop_sequence": ["<|im_end|>", "\n\n"],
    });

    let resp = client
        .post(format!("{}/api/v1/generate", state.config.corrector_url))
        .json(&payload)
        .timeout(std::time::Duration::from_secs(90))
        .send()
        .await
        .ok()?;

    let body: serde_json::Value = resp.json().await.ok()?;
    let fixed_text = body
        .get("results")
        .and_then(|r| r.as_array())
        .and_then(|a| a.first())
        .and_then(|r| r.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    // Try to parse the corrector's output as a valid tool call
    let normalized = translator::normalize(fixed_text);
    if let Some(ref tool_call) = normalized.tool_call {
        info!("Corrector fixed tool call: {} (Marvin grudgingly approves)", tool_call.name);
        
        // Execute the corrected tool call
        let result = tools::execute(&tool_call.name, &tool_call.arguments, state).await;
        
        let correction_note = format!(
            "Your tool call JSON was malformed. The corrector fixed it. You wrote something like: {}... Correct format: {{\"name\": \"{}\", \"arguments\": {{...}}}}. Do it right next time.",
            &raw_output[..raw_output.len().min(100)],
            tool_call.name
        );
        
        Some((tool_call.name.clone(), result, correction_note))
    } else {
        warn!("Corrector couldn't fix the tool call either");
        None
    }
}

/// Result of the corrector's repetition justification check.
struct RepetitionJudgment {
    allowed: bool,
    reason: String,
}

/// Ask the 14B corrector whether a repeated tool call is justified.
/// The corrector sees the tool history and recent outputs, then decides
/// if the model is legitimately exploring (different args) or stuck in a loop.
async fn check_repetition_justification(
    tool_name: &str,
    tool_history: &[String],
    output_history: &[String],
    state: &AppState,
) -> RepetitionJudgment {
    let client = reqwest::Client::new();

    let recent_outputs = output_history.iter().rev().take(3)
        .map(|s| s.chars().take(200).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n---\n");

    let recent_tools = tool_history.iter().rev().take(5)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");

    let prompt = format!(
        r#"<|im_start|>system
You are a loop detection judge. A coding model has called the same tool multiple times in a row.
Decide if this is legitimate exploration (different arguments, making progress) or a stuck loop (same thing repeatedly, no progress).

Respond with ONLY one of:
ALLOW: <reason>
BLOCK: <reason>
<|im_end|>
<|im_start|>user
Tool being repeated: "{}"
Recent tool calls: [{}]
Recent outputs (truncated):
{}

Is this repetition justified?<|im_end|>
<|im_start|>assistant
"#,
        tool_name, recent_tools, recent_outputs
    );

    let payload = serde_json::json!({
        "prompt": prompt,
        "max_length": 100,
        "temperature": 0.1,
        "stop_sequence": ["<|im_end|>", "\n\n"],
    });

    let resp = client
        .post(format!("{}/api/v1/generate", state.config.corrector_url))
        .json(&payload)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    match resp {
        Ok(r) => {
            if let Ok(body) = r.json::<serde_json::Value>().await {
                let text = body.get("results")
                    .and_then(|r| r.as_array())
                    .and_then(|a| a.first())
                    .and_then(|r| r.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("ALLOW: corrector unavailable");

                let trimmed = text.trim();
                if trimmed.starts_with("BLOCK") {
                    let reason = trimmed.strip_prefix("BLOCK:").unwrap_or(trimmed).trim().to_string();
                    RepetitionJudgment { allowed: false, reason }
                } else {
                    let reason = trimmed.strip_prefix("ALLOW:").unwrap_or(trimmed).trim().to_string();
                    RepetitionJudgment { allowed: true, reason }
                }
            } else {
                // Can't parse response — allow by default
                RepetitionJudgment { allowed: true, reason: "corrector response unparseable".to_string() }
            }
        }
        Err(_) => {
            // Corrector unreachable — allow by default (don't block work if corrector is down)
            RepetitionJudgment { allowed: true, reason: "corrector unreachable, allowing".to_string() }
        }
    }
}

/// Compress a tool result for conversation context.
/// If the result is large (>800 chars), store the full version in nautivecs
/// and return a compressed summary for the conversation.
/// This keeps the context window lean across many tool rounds.
async fn compress_tool_result(
    tool_name: &str,
    args: &serde_json::Value,
    full_result: &str,
    state: &AppState,
) -> (String, bool) {
    const COMPRESS_THRESHOLD: usize = 800;

    if full_result.len() <= COMPRESS_THRESHOLD {
        return (full_result.to_string(), false);
    }

    // Build a compressed summary: first 400 chars + last 200 chars + metadata
    let first = &full_result[..400.min(full_result.len())];
    let last_start = full_result.len().saturating_sub(200);
    let last = &full_result[last_start..];
    let lines = full_result.lines().count();

    let summary = format!(
        "[COMPRESSED — full result stored in memory, use think_harder to recall]\n\
         Tool: {}({})\n\
         Size: {} chars, {} lines\n\
         Preview: {}...\n\
         ...tail: {}",
        tool_name,
        args.get("path").or(args.get("query")).or(args.get("cmd"))
            .and_then(|v| v.as_str()).unwrap_or(""),
        full_result.len(),
        lines,
        first.trim(),
        last.trim(),
    );

    // Store full result in nautivecs for later retrieval via think_harder
    let remember_content = format!(
        "Tool result from {}({}): {}", 
        tool_name,
        args.get("path").or(args.get("query")).or(args.get("cmd"))
            .and_then(|v| v.as_str()).unwrap_or(""),
        &full_result[..full_result.len().min(3000)]
    );
    let remember_args = serde_json::json!({
        "content": remember_content,
        "tags": format!("tool_result,{},context_overflow", tool_name)
    });
    let _ = tools::execute("remember", &remember_args, state).await;

    (summary, true)
}

/// Trim conversation to keep only the first message (system/task) and the last N messages.
/// This prevents unbounded context growth across many tool rounds.
fn trim_conversation(conv: &mut Vec<Message>, keep_last: usize) {
    if conv.len() <= keep_last + 1 {
        return; // Nothing to trim
    }

    // Keep first message (the original user task) + last N messages
    let first = conv[0].clone();
    let tail: Vec<Message> = conv.iter().rev().take(keep_last).cloned().collect::<Vec<_>>().into_iter().rev().collect();

    conv.clear();
    conv.push(first);

    // Add a context note so the model knows history was trimmed
    conv.push(Message {
        role: "user".to_string(),
        content: format!(
            "[CONTEXT NOTE: {} earlier messages were compressed and stored in memory. \
             Use think_harder to recall previous tool results if needed. \
             Continue working on the original task.]",
            tail.len()
        ),
    });

    conv.extend(tail);
}
