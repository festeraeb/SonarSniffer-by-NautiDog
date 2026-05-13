use crate::diagnostics;
use crate::memory;
use crate::prompts;
use crate::tools;
use crate::translator::{self, FailureType, Message};
use crate::{AppState, SendResponse};
use tracing::{info, warn};

const MAX_TOOL_ROUNDS: u32 = u32::MAX; // No limit — only loops/empty stop it
const MAX_DIAGNOSIS_ATTEMPTS: u32 = 2;

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
    let mut temperature = 0.4f32;

    for round in 1..=MAX_TOOL_ROUNDS {
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

            if failure_count >= MAX_DIAGNOSIS_ATTEMPTS {
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

            // Check for repeated tool calls
            if tool_actions.len() >= 3 {
                let last_three = &tool_actions[tool_actions.len() - 3..];
                if last_three[0] == last_three[1] && last_three[1] == last_three[2] {
                    warn!("Repeated tool call detected: {}", tool_call.name);
                    // Force a summary
                    let mut conv = state.conversation.lock().await;
                    conv.push(Message {
                        role: "user".to_string(),
                        content: "[System]: You've called the same tool 3 times. Stop and provide your final answer now.".to_string(),
                    });
                    continue;
                }
            }

            // Execute the tool
            let result = tools::execute(&tool_call.name, &tool_call.arguments, state).await;

            // Format result as user message (QwenChatML format)
            let formatted = translator::format_tool_result_for_qwen(&result, round, MAX_TOOL_ROUNDS);

            // Add to conversation
            let mut conv = state.conversation.lock().await;
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
        .timeout(std::time::Duration::from_secs(30))
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

    let payload = serde_json::json!({
        "prompt": prompt,
        "max_length": 12288,
        "temperature": temperature,
        "top_p": 0.95,
        "rep_pen": 1.1,
        "stop_sequence": ["</tool_call>", "<|im_end|>", "<|endoftext|>"],
    });

    let resp = client
        .post(format!("{}/api/v1/generate", state.config.coder_url))
        .json(&payload)
        .timeout(std::time::Duration::from_secs(120))
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
        .timeout(std::time::Duration::from_secs(30))
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
