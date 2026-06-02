use crate::corrector_preset::{self, CorrectionStrength, Escalation, PresetRegistry};
use crate::loop_tuning;
use crate::diagnostics;
use crate::memory;
use crate::orchestration;
use crate::prompts;
use crate::routing;
use crate::tools;
use crate::translator::{self, FailureType, Message};
use crate::{AppState, SendResponse};
use tracing::{info, warn};

/// Hard ceiling; per-session cap also comes from `[tuning] max_think_rounds`.
const MAX_TOOL_ROUNDS: u32 = 32;

// ── Corrector endpoint cascade ───────────────────────────────────────────────
// Try each in order until one responds. This lets the P1000 TinyLlama act as
// a fallback corrector when Marvin (FortyTwo Rust 14B on 1070) is offline.
//
// LAN IPs — only the M2200 (mobile) uses Tailscale.
const CORRECTOR_CASCADE: &[(&str, &str)] = &[
    ("marvin-rust14b", "http://10.0.0.201:5200"),     // DeepSeek R1 7B on RTX 2060 (cesarops2)
    ("picasso-tiny",   "http://10.0.0.201:5571"),     // Phi-3 draft on GTX 1070 (cesarops2)
    ("t440-fallback",  "http://127.0.0.1:5002"),      // P100 when cesarops2 offline
    ("laptop-tiny",    "http://100.110.214.86:5571"), // M2200 on ThinkPad — Tailscale tertiary (mobile)
];

/// Resolve a model string for a runtime endpoint from `cluster_config.toml`.
/// This lets corrector prompts/tolerances adapt per model family.
fn model_name_for_endpoint(endpoint: &str) -> Option<String> {
    let content = std::fs::read_to_string(orchestration::CFG_PATH).ok()?;
    let table: toml::Table = content.parse().ok()?;
    let agents = table.get("agent")?.as_array()?;
    for a in agents {
        let t = a.as_table()?;
        let ep = t.get("endpoint").and_then(|v| v.as_str()).unwrap_or_default();
        if ep == endpoint {
            let model = t.get("model").and_then(|v| v.as_str()).unwrap_or_default();
            if !model.is_empty() {
                return Some(model.to_string());
            }
        }
    }
    None
}

fn corrector_profile_for_endpoint(
    endpoint: &str,
    fallback_id: &str,
) -> (corrector_preset::CorrectorPreset, String) {
    let registry = PresetRegistry::load(orchestration::CFG_PATH);
    let model_hint = model_name_for_endpoint(endpoint).unwrap_or_else(|| fallback_id.to_string());
    let preset = registry.resolve(&model_hint).clone();
    (preset, model_hint)
}

// ── Auto-WSO trigger ─────────────────────────────────────────────────────────
// After this many consecutive failures, the loop engine fires a think_harder
// (WSO + nautivecs) for the most recent task automatically, even if the model
// hasn't asked for one. Result is injected into the next prompt as context.
const AUTO_WSO_AFTER_FAILURES: u32 = 3;

// ── "Close enough" thresholds ────────────────────────────────────────────────
// If a code block is this close to compilable, finish it ourselves.
const CLOSE_ENOUGH_MISSING_BRACES: i32 = 3;   // ≤3 unmatched braces
const CLOSE_ENOUGH_MISSING_LINES: usize = 15; // ≤15 lines of obvious boilerplate missing

// Light context management (avoid aggressive amnesia on long agent runs).
const CONV_TRIM_MIN_LEN: usize = 40;   // only trim when conversation exceeds this
const CONV_KEEP_LAST: usize = 28;    // keep last N messages (~14 exchanges) + anchor task
const CONV_COMPACT_KEEP_LAST: usize = 18;
const CONV_CAPACITY_SOFT_TOKENS: usize = 2200;
const CONV_CAPACITY_SOFT_CHARS: usize = 9000;
const CONV_SUMMARY_MAX_ITEMS: usize = 10;
const TOOL_RESULT_COMPRESS_AT: usize = 3200;
const TOOL_RESULT_PREVIEW_HEAD: usize = 1200;
const TOOL_RESULT_PREVIEW_TAIL: usize = 500;

/// Large batch/audit dispatches should not run triple-model parallel grading.
fn is_heavy_batch_task(message: &str) -> bool {
    let low = message.to_lowercase();
    low.contains("llm_dispatch_batches.json")
        || (low.contains("blueprint") && low.contains("audit") && low.contains("batch"))
}

/// Byte-safe prefix/suffix (never split a UTF-8 codepoint).
fn safe_byte_prefix(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn safe_byte_suffix(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut start = s.len().saturating_sub(max_bytes);
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

/// Result of the corrector's repetition justification check (when `corrector_llm_loop_judge` is on).
struct RepetitionJudgment {
    allowed: bool,
    reason: String,
}

fn likely_code_or_build_issue(task: &str, reason: &str) -> bool {
    let hay = format!("{} {}", task.to_lowercase(), reason.to_lowercase());
    [
        "rust", "cargo", "compile", "build", "syntax", "type error", "borrow", "lifetime",
        "python", "typescript", "javascript", "shell", "bash", "bad substitution", "path",
        "test", "failing", "stack trace", ".rs", ".py", ".ts", "n8n",
    ]
    .iter()
    .any(|k| hay.contains(k))
}

async fn remember_orchestrator_pattern(
    state: &AppState,
    task: &str,
    tool_name: &str,
    reason: &str,
    demand_try: u32,
) {
    let content = format!(
        "loop-block pattern: tool='{}' demand_try={}/3 reason='{}' task='{}'",
        tool_name,
        demand_try,
        safe_byte_prefix(reason, 220),
        safe_byte_prefix(task, 280)
    );
    let args = serde_json::json!({
        "content": content,
        "tags": "orchestrator,pattern,loop_block,corrector,feedback"
    });
    let _ = tools::execute("remember", &args, state).await;
}

async fn fetch_code_knowledge_hint(state: &AppState, task: &str, reason: &str) -> Option<String> {
    if !likely_code_or_build_issue(task, reason) {
        return None;
    }
    let query = format!(
        "Code issue recovery for task: {} | blocker: {} | find exact file/path fix steps and validation commands",
        safe_byte_prefix(task, 180),
        safe_byte_prefix(reason, 140)
    );
    let args = serde_json::json!({ "query": query });
    let out = tools::execute("think_harder", &args, state).await;
    if out.trim().is_empty() {
        None
    } else {
        Some(safe_byte_prefix(&out, 1200).to_string())
    }
}

/// Inject soft corrector nudges only after many consecutive rounds without a tool call.
async fn maybe_inject_think_escalation(
    state: &AppState,
    _preset: &corrector_preset::CorrectorPreset,
    rounds_since_last_tool: u32,
    think_escalate_after: u32,
    total_round: u32,
    skip_corrector: bool,
    last_escalation: &mut Option<Escalation>,
) {
    if skip_corrector || rounds_since_last_tool < think_escalate_after {
        return;
    }
    let esc = corrector_preset::decide_think_stall_update(
        rounds_since_last_tool,
        think_escalate_after,
        last_escalation.as_ref(),
    );
    if esc == Escalation::Continue {
        return;
    }
    if let Some(msg) = corrector_preset::format_escalation_message(&esc) {
        info!(
            "Think-only corrector escalation after {} rounds without tool: {:?}",
            rounds_since_last_tool,
            std::mem::discriminant(&esc)
        );
        let mut conv = state.conversation.lock().await;
        conv.push(Message {
            role: "user".to_string(),
            content: msg,
        });
        *last_escalation = Some(esc);
    }
}

/// Main orchestration loop: Strategy → Execution → Verification.
pub async fn run(state: &AppState, user_message: &str) -> SendResponse {
    let cfg = state.config.read().await;
    let fast_mode = user_message.starts_with("/fast");
    let message = if fast_mode {
        user_message.trim_start_matches("/fast").trim()
    } else {
        user_message
    };
    let preset_registry = PresetRegistry::load(orchestration::CFG_PATH);
    let preset = preset_registry.resolve(&cfg.chat_model);

    orchestration::pamp_shadow_call(state, message, if fast_mode { "ask" } else { "chat" }).await;

    let heavy_batch = is_heavy_batch_task(message);
    let parallel_dual = cfg.parallel_dual_grade && !fast_mode && !heavy_batch;
    if heavy_batch && cfg.parallel_dual_grade {
        info!("heavy batch task: skipping parallel_dual_grade (single coder path)");
    }
    let parallel_rounds = if cfg.parallel_dual_grade_rounds.is_empty() {
        routing::default_parallel_dual_rounds()
    } else {
        cfg.parallel_dual_grade_rounds.clone()
    };
    if parallel_dual {
        if cfg.parallel_dual_coders {
            info!(
                "parallel_dual_coders: coder_a={} coder_b={} reviewer={} thinker={} rounds={:?}",
                cfg.coder_url, cfg.validator_url, cfg.reviewer_url, cfg.thinker_url, parallel_rounds
            );
        } else {
            info!(
                "parallel_dual_grade: coder={} reviewer={} thinker={} rounds={:?}",
                cfg.coder_url, cfg.reviewer_url, cfg.thinker_url, parallel_rounds
            );
        }
    }

    // --- Layer 1: Planner pre-flight (/fast skips; dual-path gets a two-worker brief) ---
    let thinker_context = if !fast_mode {
        get_thinker_preflight(message, &cfg.thinker_url, parallel_dual).await
    } else {
        String::new()
    };

    let mut user_content = message.to_string();
    if orchestration::worker_inject_vectors_enabled() && preset.vector_inject {
        let snippet = orchestration::fetch_nautivecs_snippet(state, message, 5).await;
        if !snippet.is_empty() {
            user_content = format!(
                "[NAUTIVECS CONTEXT]\n{}\n\n[USER]\n{}",
                &snippet[..snippet.len().min(3000)],
                message
            );
        }
    }
    // Steering notes go in system prompt only (build_prompt), not duplicated in user turn.

    // Add user message to conversation
    {
        let mut conv = state.conversation.lock().await;
        conv.push(Message {
            role: "user".to_string(),
            content: user_content,
        });
    }

    // --- Layer 3: Execute generation loop ---
    let mut tool_actions: Vec<String> = Vec::new();
    let mut output_history: Vec<String> = Vec::new();
    let mut failure_count: u32 = 0;
    let mut diagnosis_info: Option<String> = None;
    let rt = loop_tuning::load_for_loop(preset.think_tolerance);
    let max_diagnosis = rt.max_diagnosis;
    let skip_corrector = rt.skip_corrector;
    let think_escalate_after = rt.think_escalate_after;
    let hard_repeat_cap = rt.hard_repeat_cap;
    let hard_terminate_after = rt.hard_terminate_after;
    let corrector_tool_fix = rt.corrector_tool_fix;
    let corrector_llm_loop_judge = rt.corrector_llm_loop_judge;
    let loop_threshold = preset.loop_threshold.max(3);
    let max_rounds = rt.max_diagnosis.clamp(8, MAX_TOOL_ROUNDS);
    let mut temperature = rt.temperature;
    let mut last_auto_wso_at: u32 = 0; // failure_count when last auto-WSO fired
    let mut rounds_since_last_tool: u32 = 0;
    let mut last_think_escalation: Option<Escalation> = None;
    let mut last_tool_repeat_escalation: Option<Escalation> = None;
    let mut repeat_block_demands: u32 = 0;
    let mut context_trims: u32 = 0;
    let mut total_round: u32 = 0;

    for round in 1..=max_rounds {
        total_round = round;

        if rounds_since_last_tool >= 12 && round > 2 && !tool_actions.is_empty() {
            warn!(
                "Stall: {} rounds since last tool after {:?}",
                rounds_since_last_tool, tool_actions.last()
            );
            return finalize_response(
                format!(
                    "[Forge stalled after round {round}: no new tool calls since {:?}. \
                     Last tools: {:?}. Use read_file/write_file on concrete paths, or /clear and retry. \
                     (nautivecs may be empty — do not loop on think_harder alone.)]",
                    tool_actions.last(),
                    tool_actions
                ),
                &tool_actions,
                diagnosis_info,
                total_round,
                context_trims,
            );
        }

        // Check interrupt flag
        if state.interrupt.load(std::sync::atomic::Ordering::Relaxed) {
            info!("INTERRUPTED at round {}", round);
            state.interrupt.store(false, std::sync::atomic::Ordering::Relaxed);
            return finalize_response(
                "[INTERRUPTED by user]".to_string(),
                &tool_actions,
                diagnosis_info,
                total_round,
                context_trims,
            );
        }

        // ── Auto-WSO when stuck ──────────────────────────────────────────────
        // Fire think_harder automatically if we've hit AUTO_WSO_AFTER_FAILURES
        // consecutive failures since the last auto-fire. The model gets the
        // search results in its next prompt without having to ask.
        if failure_count >= AUTO_WSO_AFTER_FAILURES && failure_count > last_auto_wso_at {
            let last_user = {
                let conv = state.conversation.lock().await;
                conv.iter().rev()
                    .find(|m| m.role == "user")
                    .map(|m| m.content.clone())
                    .unwrap_or_default()
            };
            if !last_user.is_empty() {
                let query = if last_user.len() > 200 { &last_user[..200] } else { &last_user[..] };
                info!("Auto-WSO triggered after {} failures: '{}'", failure_count, query);
                let args = serde_json::json!({"query": query});
                let wso_result = tools::execute("think_harder", &args, state).await;
                let mut conv = state.conversation.lock().await;
                conv.push(Message {
                    role: "user".to_string(),
                    content: format!(
                        "[AUTO-WSO — you appear stuck after {} failures. Here's relevant context from the knowledge base + web. Use it.]\n{}",
                        failure_count, wso_result
                    ),
                });
                last_auto_wso_at = failure_count;
            }
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

        if maybe_compact_conversation_capacity(state, round).await {
            context_trims += 1;
        }


        // Build prompt from conversation state
        let prompt = build_prompt(state, &thinker_context, failure_count, &cfg.chat_template).await;

        crate::stream_sources::log_send(
            state,
            "round",
            &format!("#{round}: generating (prompt {} chars)", prompt.len()),
        )
        .await;

        // Generate: parallel dual + thinker grade on configured rounds (default 1 and 3)
        let (raw_output, grade_note, grade_feedback) = if parallel_dual && parallel_rounds.contains(&round) {
            generate_parallel_graded(
                state,
                &cfg,
                &prompt,
                &thinker_context,
                failure_count,
                message,
                temperature,
                round,
            )
            .await
        } else {
            (
                generate_35b(&prompt, temperature, &cfg.coder_url).await,
                String::new(),
                String::new(),
            )
        };
        if !grade_note.is_empty() {
            tool_actions.push(grade_note);
        }
        if !grade_feedback.is_empty() {
            let mut conv = state.conversation.lock().await;
            conv.push(Message {
                role: "user".to_string(),
                content: format!(
                    "[DUAL-ROUND FEEDBACK]: {}\nApply this feedback in your next action. If confidence is low, call think_harder or use get_file_context/search_symbols first.",
                    grade_feedback
                ),
            });
        }

        let llm_preview: String = raw_output.chars().take(280).collect();
        crate::stream_sources::log_send(
            state,
            "llm",
            &format!("#{}: {}", round, llm_preview.replace('\n', " ")),
        )
        .await;

        // --- Layer 2: Translator normalizes output ---
        let normalized = translator::normalize(&raw_output);

        // Loop detection
        if translator::detect_output_loop(&output_history, &raw_output) {
            warn!("Loop detected at round {}", round);
            // Hard reset recent history
            hard_reset(state).await;
            failure_count += 1;
            rounds_since_last_tool += 1;
            maybe_inject_think_escalation(
                state,
                preset,
                rounds_since_last_tool,
                think_escalate_after,
                round,
                skip_corrector,
                &mut last_think_escalation,
            )
            .await;
            temperature = (temperature + 0.15).min(1.0);
            continue;
        }
        output_history.push(raw_output.clone());

        // Handle failure states
        if let Some(ref failure_type) = normalized.failure {
            warn!("Failure detected: {:?} at round {}", failure_type, round);

            if matches!(failure_type, FailureType::MalformedToolCall) {
                if skip_corrector || !corrector_tool_fix {
                    info!(
                        "Malformed tool call at round {} — static hint (corrector_tool_fix={})",
                        round, corrector_tool_fix
                    );
                    let mut conv = state.conversation.lock().await;
                    conv.push(Message {
                        role: "user".to_string(),
                        content: "[FORMAT FIX]: Your last message had invalid tool JSON. \
                                  Output ONE valid tool call: {\"name\": \"tool_name\", \"arguments\": {...}} \
                                  or give your final answer in plain text.".to_string(),
                    });
                    failure_count += 1;
                    rounds_since_last_tool += 1;
                    temperature = (temperature + 0.05).min(1.0);
                    continue;
                }
                info!("Routing malformed tool call to corrector cascade");
                let corrected =
                    correct_and_execute_cascade(&raw_output, &normalized.content, &cfg.corrector_url, state)
                        .await;
                if let Some((tool_name, result, correction_note, corrector_id)) = corrected {
                    tool_actions.push(format!("{}(corrected-by:{})", tool_name, corrector_id));
                    let feedback = format!(
                        "{}\n\n[CORRECTION by {}]: {}\nPattern: {{\"name\": \"TOOL_NAME\", \"arguments\": {{\"key\": \"value\"}}}}",
                        result, corrector_id, correction_note
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
                    let remember_args = serde_json::json!({
                        "content": format!(
                            "Correct tool call format: {{\"name\": \"{}\", \"arguments\": {{...}}}}. Fixed by {}.",
                            tool_name, corrector_id
                        ),
                        "tags": "tool_call,format_fix,corrector"
                    });
                    let _ = tools::execute("remember", &remember_args, state).await;
                    rounds_since_last_tool = 0;
                    continue;
                }

                // Corrector could not coerce valid JSON after demanded retries.
                // Stop this model's current path and hand back to dispatcher guidance.
                let dispatcher_note = build_dispatcher_handoff(state, &raw_output).await
                    .unwrap_or_else(|| {
                        "Dispatcher handoff: correction failed after 3 demanded retries. Use a different tool path or provide final answer.".to_string()
                    });
                let mut conv = state.conversation.lock().await;
                conv.push(Message {
                    role: "user".to_string(),
                    content: format!(
                        "[CORRECTOR DEMAND FAILED x3]: malformed tool JSON could not be repaired. \
                         Stop current model path and follow dispatcher guidance below.\n\n{}\n\n\
                         Next response MUST be either:\n\
                         1) one valid tool call JSON, or\n\
                         2) final plain-text answer.",
                        dispatcher_note
                    ),
                });
                failure_count += 1;
                rounds_since_last_tool += 1;
                continue;
            }

            if failure_count >= max_diagnosis {
                // Give up — return whatever we have
                let response = if normalized.content.is_empty() {
                    format!("[Model stuck after {} attempts. Last failure: {:?}]", failure_count, failure_type)
                } else {
                    normalized.content
                };

                return finalize_response(
                    response,
                    &tool_actions,
                    diagnosis_info,
                    total_round,
                    context_trims,
                );
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
            rounds_since_last_tool += 1;
            maybe_inject_think_escalation(
                state,
                preset,
                rounds_since_last_tool,
                think_escalate_after,
                round,
                skip_corrector,
                &mut last_think_escalation,
            )
            .await;
            continue;
        }

        // --- Tool call handling ---
        if let Some(ref tool_call) = normalized.tool_call {
            info!("Tool call round {}: {}", round, tool_call.name);
            tool_actions.push(format_tool_action(&tool_call.name, &tool_call.arguments));

            let repeat_count = tool_actions
                .iter()
                .rev()
                .take_while(|a| {
                    a.as_str() == tool_call.name.as_str()
                        || a.starts_with(&format!("{}(", tool_call.name))
                        || a.starts_with(&tool_call.name)
                })
                .count();

            if corrector_llm_loop_judge && tool_actions.len() >= 3 {
                let last_three = &tool_actions[tool_actions.len().saturating_sub(3)..];
                if last_three.iter().all(|a| {
                    a.trim_end_matches(|c: char| !c.is_alphabetic()) == tool_call.name.as_str()
                        || a == &tool_call.name
                        || a.starts_with(&tool_call.name)
                }) && repeat_count >= loop_threshold as usize
                {
                    let judgment = check_repetition_justification_cascade(
                        &tool_call.name,
                        &tool_actions,
                        &output_history,
                        &cfg.corrector_url,
                        state,
                    )
                    .await;
                    if !judgment.allowed {
                        repeat_block_demands = repeat_block_demands.saturating_add(1);
                        remember_orchestrator_pattern(
                            state,
                            message,
                            &tool_call.name,
                            &judgment.reason,
                            repeat_block_demands,
                        )
                        .await;

                        warn!(
                            "Corrector BLOCKED repeated '{}': {}",
                            tool_call.name, judgment.reason
                        );

                        let demand_header = format!(
                            "[LOOP BLOCKED DEMAND {}/3]: '{}' called {} times. Reason: {}.",
                            repeat_block_demands,
                            tool_call.name,
                            repeat_count,
                            judgment.reason
                        );

                        if repeat_block_demands < 3 {
                            let mut conv = state.conversation.lock().await;
                            conv.push(Message {
                                role: "user".to_string(),
                                content: format!(
                                    "{} Use a DIFFERENT tool now.\n\
                                     If this is code/build related, call think_harder with a code-fix query, \
                                     then follow with one concrete tool call (read_file/search_symbols/get_file_context/write_file/run_command).\n\
                                     Do not repeat '{}'.",
                                    demand_header,
                                    tool_call.name
                                ),
                            });
                            rounds_since_last_tool += 1;
                            failure_count += 1;
                            continue;
                        }

                        let dispatcher_note = build_dispatcher_handoff(state, &raw_output)
                            .await
                            .unwrap_or_else(|| {
                                "Dispatcher handoff: loop block demand exhausted. Choose a new approach and tool sequence.".to_string()
                            });
                        let code_hint = fetch_code_knowledge_hint(state, message, &judgment.reason).await;
                        let mut conv = state.conversation.lock().await;
                        conv.push(Message {
                            role: "user".to_string(),
                            content: format!(
                                "[LOOP BLOCKED x3 - RETURN TO DISPATCHER]: stop repeating '{}'.\n\n\
                                 Dispatcher guidance:\n{}\n\n\
                                 {}\n\n\
                                 Next response MUST be either:\n\
                                 1) one different valid tool call JSON, or\n\
                                 2) final plain-text answer.",
                                tool_call.name,
                                dispatcher_note,
                                code_hint
                                    .map(|s| format!("Injected knowledge hint:\n{}", s))
                                    .unwrap_or_else(|| "No extra code knowledge hint available.".to_string())
                            ),
                        });
                        repeat_block_demands = 0;
                        rounds_since_last_tool += 1;
                        failure_count += 1;
                        continue;
                    }
                }
            }

            // Preset ladder (model-specific) before global repeat-demand cap.
            if !skip_corrector && preset.loop_threshold > 0 && repeat_count >= preset.loop_threshold as usize
            {
                let esc = corrector_preset::decide_escalation(
                    preset,
                    repeat_count as u32,
                    0,
                    total_round,
                    last_tool_repeat_escalation.as_ref(),
                    &tool_call.name,
                );
                if esc != Escalation::Continue && repeat_count < hard_repeat_cap {
                    if let Some(msg) = corrector_preset::format_escalation_message(&esc) {
                        info!(
                            "Tool-repeat preset escalation ({}x '{}'): {:?}",
                            repeat_count,
                            tool_call.name,
                            std::mem::discriminant(&esc)
                        );
                        let mut conv = state.conversation.lock().await;
                        conv.push(Message {
                            role: "user".to_string(),
                            content: msg,
                        });
                        last_tool_repeat_escalation = Some(esc);
                    }
                    rounds_since_last_tool += 1;
                    failure_count += 1;
                    continue;
                }
            }

            if repeat_count >= hard_repeat_cap {
                let esc = corrector_preset::decide_tool_repeat_stall(
                    repeat_count,
                    hard_repeat_cap,
                    hard_terminate_after,
                    &tool_call.name,
                    last_tool_repeat_escalation.as_ref(),
                );
                if esc != Escalation::Continue {
                    if let Some(msg) = corrector_preset::format_escalation_message(&esc) {
                        warn!(
                            "Tool-repeat demand ({}x '{}', demand_at={}, terminate_at={}): {:?}",
                            repeat_count,
                            tool_call.name,
                            hard_repeat_cap,
                            hard_terminate_after,
                            std::mem::discriminant(&esc)
                        );
                        let mut conv = state.conversation.lock().await;
                        conv.push(Message {
                            role: "user".to_string(),
                            content: msg,
                        });
                        last_tool_repeat_escalation = Some(esc);
                    }
                    rounds_since_last_tool += 1;
                    failure_count += 1;
                    repeat_block_demands = 0;
                    continue;
                }
            }

            last_tool_repeat_escalation = None;

            // Execute the tool
            let result = tools::execute(&tool_call.name, &tool_call.arguments, state).await;
            repeat_block_demands = 0;

            if tool_call.name == "think_harder" {
                let empty = result.trim().is_empty()
                    || result.trim() == "[]"
                    || (result.contains("[nautivecs]") && result.contains("\"results\":[]")
                        && !result.contains("[web search]"));
                if empty {
                    let mut conv = state.conversation.lock().await;
                    conv.push(Message {
                        role: "user".to_string(),
                        content: "[think_harder returned little or no data — nautivecs index may be empty. \
                                  Next: read_file on src/ and Cargo.toml under the task path, then write_file. \
                                  Do not call think_harder again unless you need web search.]"
                            .to_string(),
                    });
                }
            }

            // ── "Close enough" detection ─────────────────────────────────────
            // If the model just wrote a code file that's nearly complete (compile
            // errors are minor / missing closing braces), finish it ourselves and
            // queue a WIP commit rather than burning another full round.
            if tool_call.name == "write_file" {
                if let Some(content) = tool_call.arguments.get("content").and_then(|v| v.as_str()) {
                    if let Some(path) = tool_call.arguments.get("path").and_then(|v| v.as_str()) {
                        if path.ends_with(".rs") || path.ends_with(".wgsl") || path.ends_with(".toml") {
                            if let Some(fix) = assess_close_enough(content, path) {
                                info!("Close-enough detected for '{}': {}", path, fix.description);
                                // Apply the fix ourselves
                                if let Some(fixed_content) = &fix.fixed_content {
                                    let write_args = serde_json::json!({
                                        "path": path,
                                        "content": fixed_content
                                    });
                                    let _ = tools::execute("write_file", &write_args, state).await;
                                    tool_actions.push(format!("write_file(close-enough-fix:{})", path));
                                    info!("Applied close-enough fix to '{}': {}", path, fix.description);
                                }
                                // Queue next round with targeted context
                                let next_round_msg = format!(
                                    "[CLOSE-ENOUGH AUTO-FIX]: '{}' was nearly complete. Applied fix: {}. \
                                     Next step: run cargo_check on the project to verify it compiles, \
                                     then commit with a WIP message. If cargo_check passes, \
                                     call run_command with: git add {} && git commit -m 'WIP: {}'",
                                    path, fix.description, path,
                                    path.split('/').last().unwrap_or(path)
                                );
                                let mut conv = state.conversation.lock().await;
                                conv.push(Message { role: "assistant".to_string(), content: normalized.content.clone() });
                                conv.push(Message { role: "user".to_string(), content: next_round_msg });
                                continue;
                            }
                        }
                    }
                }
            }

            // --- Context Window Management ---
            // If the result is large, compress it for conversation and store full in nautivecs
            let (conv_result, stored_full) = compress_tool_result(&tool_call.name, &tool_call.arguments, &result, state).await;

            // Format result as user message (QwenChatML format)
            let formatted = translator::format_tool_result_for_qwen(&conv_result, round, MAX_TOOL_ROUNDS);

            let mut conv = state.conversation.lock().await;
            if light_trim_conversation(&mut conv) {
                context_trims += 1;
            }

            let assistant_turn = assistant_turn_summary(&tool_call.name, &normalized.content);
            conv.push(Message {
                role: "assistant".to_string(),
                content: assistant_turn,
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
                last_auto_wso_at = 0; // re-arm auto-WSO for next failure burst
            }
            rounds_since_last_tool = 0;
            last_think_escalation = None;

            continue;
        }

        // --- Plain text response (no tool call, no failure) ---
        let user_reply = sanitize_user_response(&normalized.content);

        if is_unusable_assistant_reply(&user_reply) && !tool_actions.is_empty() {
            warn!(
                "Model returned unusable final text after tools; nudging (round {})",
                round
            );
            let mut conv = state.conversation.lock().await;
            conv.push(Message {
                role: "user".to_string(),
                content: format!(
                    "[Forge]: You already ran tools ({}) but did not give a final answer. \
                     Summarize findings in plain English for the user. \
                     Do not output [tool: ...] prefixes, XML, or channel tags.",
                    tool_actions.join(", ")
                ),
            });
            failure_count += 1;
            rounds_since_last_tool += 1;
            continue;
        }

        {
            let mut conv = state.conversation.lock().await;
            conv.push(Message {
                role: "assistant".to_string(),
                content: user_reply.clone(),
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

        return finalize_response(
            user_reply,
            &tool_actions,
            diagnosis_info,
            total_round,
            context_trims,
        );
    }

    // Exhausted all rounds
    finalize_response(
        format!(
            "[Exhausted {} tool rounds. Last actions: {:?}]",
            MAX_TOOL_ROUNDS, tool_actions
        ),
        &tool_actions,
        diagnosis_info,
        total_round,
        context_trims,
    )
}

/// Parallel generate + thinker grade. When `parallel_dual_coders`, two coders run then reviewer
/// hands fix areas to the opposite lane for round 2.
async fn generate_parallel_graded(
    state: &AppState,
    cfg: &crate::ForgeConfig,
    coder_prompt: &str,
    thinker_context: &str,
    failure_count: u32,
    task: &str,
    temperature: f32,
    round: u32,
) -> (String, String, String) {
    if cfg.parallel_dual_coders {
        return generate_parallel_dual_coders(
            cfg, coder_prompt, task, temperature, round,
        )
        .await;
    }

    let cluster = routing::load_cluster_routing();
    let reviewer_tpl = routing::template_for_endpoint(&cluster, &cfg.reviewer_url);
    let reviewer_base = if reviewer_tpl == cfg.chat_template {
        coder_prompt.to_string()
    } else {
        build_prompt(state, thinker_context, failure_count, &reviewer_tpl).await
    };
    let reviewer_prompt = format!(
        "{}\n{}",
        reviewer_base,
        prompts::reviewer_evidence_suffix(task)
    );

    let coder_url = cfg.coder_url.clone();
    let reviewer_url = cfg.reviewer_url.clone();
    let thinker_url = cfg.thinker_url.clone();

    let (draft_a, draft_b) = tokio::join!(
        generate_35b(coder_prompt, temperature, &coder_url),
        generate_35b(&reviewer_prompt, temperature, &reviewer_url),
    );

    if draft_a.is_empty() && draft_b.is_empty() {
        return (String::new(), "parallel_grade:both_empty".to_string(), String::new());
    }
    if draft_a.is_empty() {
        return (draft_b, "parallel_grade:B(fallback_a_empty)".to_string(), String::new());
    }
    if draft_b.is_empty() {
        return (draft_a, "parallel_grade:A(fallback_b_empty)".to_string(), String::new());
    }

    let grade_prompt = prompts::thinker_grade_prompt(task, &draft_a, &draft_b, round);
    let client = reqwest::Client::new();
    let grade_raw = crate::inference_client::complete_prompt(
        &client,
        &thinker_url,
        &grade_prompt,
        384,
        0.2,
        vec!["<|im_end|>".to_string()],
        None,
    )
    .await
    .unwrap_or_default();

    let pick_b = grade_raw.to_uppercase().contains("WINNER: B")
        || grade_raw.to_uppercase().contains("WINNER:B");
    let winner = if pick_b { "B" } else { "A" };
    let output = if pick_b { draft_b } else { draft_a };
    let grade_feedback = grade_raw
        .lines()
        .nth(1)
        .or_else(|| grade_raw.lines().next())
        .unwrap_or("")
        .trim()
        .to_string();
    info!(
        "parallel_dual_grade: winner={} (thinker: {})",
        winner,
        grade_raw.lines().next().unwrap_or("?")
    );
    (
        output,
        format!("parallel_grade:r{round}_{winner}(coder=A,reviewer=B)"),
        grade_feedback,
    )
}

/// Two coders in parallel (A=coder_url, B=validator/draft URL), ZAYA thinker grades, :5002 reviewer
/// sends FIX_AREAS to the opposite coder for round 2.
async fn generate_parallel_dual_coders(
    cfg: &crate::ForgeConfig,
    coder_prompt: &str,
    task: &str,
    temperature: f32,
    round: u32,
) -> (String, String, String) {
    let coder_a_url = cfg.coder_url.clone();
    let coder_b_url = cfg.validator_url.clone();
    let reviewer_url = cfg.reviewer_url.clone();
    let thinker_url = cfg.thinker_url.clone();

    // c2 MoE :5200 runs with n_ctx=3072 — keep parallel coder prompts bounded.
    let coder_prompt_b = if coder_prompt.len() > 2400 {
        format!("{}…\n[truncated for 5200 ctx]", &coder_prompt[..2400])
    } else {
        coder_prompt.to_string()
    };

    let (draft_a, draft_b) = tokio::join!(
        generate_35b(coder_prompt, temperature, &coder_a_url),
        generate_35b(&coder_prompt_b, temperature, &coder_b_url),
    );

    if draft_a.is_empty() && draft_b.is_empty() {
        return (
            String::new(),
            "parallel_dual_coders:both_empty".to_string(),
            String::new(),
        );
    }
    if draft_a.is_empty() {
        return (
            draft_b.clone(),
            "parallel_dual_coders:B_only".to_string(),
            String::new(),
        );
    }
    if draft_b.is_empty() {
        return (
            draft_a.clone(),
            "parallel_dual_coders:A_only".to_string(),
            String::new(),
        );
    }

    let client = reqwest::Client::new();
    let grade_prompt = prompts::thinker_grade_prompt(task, &draft_a, &draft_b, round);
    let grade_raw = crate::inference_client::complete_prompt(
        &client,
        &thinker_url,
        &grade_prompt,
        384,
        0.2,
        vec!["<|im_end|>".to_string()],
        None,
    )
    .await
    .unwrap_or_default();

    let thinker_pick_b = grade_raw.to_uppercase().contains("WINNER: B")
        || grade_raw.to_uppercase().contains("WINNER:B");
    let winner = if thinker_pick_b { "B" } else { "A" };
    let (winner_draft, loser_draft) = if thinker_pick_b {
        (&draft_b, &draft_a)
    } else {
        (&draft_a, &draft_b)
    };

    let handoff_prompt = prompts::reviewer_dual_coder_handoff_prompt(task, &draft_a, &draft_b);
    let reviewer_raw = crate::inference_client::complete_prompt(
        &client,
        &reviewer_url,
        &handoff_prompt,
        512,
        0.15,
        vec!["<|im_end|>".to_string()],
        None,
    )
    .await
    .unwrap_or_default();

    let handoff_to_b = reviewer_raw.to_uppercase().contains("HANDOFF_TO: B")
        || reviewer_raw.to_uppercase().contains("HANDOFF_TO:B");
    // Opposite coder from thinker winner: if A won, round-2 runs on B URL (and vice versa).
    let round2_on_b = winner == "A";
    let round2_url = if round2_on_b {
        coder_b_url.clone()
    } else {
        coder_a_url.clone()
    };
    let handoff_lane = if round2_on_b { "B" } else { "A" };
    if handoff_to_b != round2_on_b {
        info!(
            "parallel_dual_coders: reviewer HANDOFF_TO={} thinker_winner={} — using opposite lane {}",
            if handoff_to_b { "B" } else { "A" },
            winner,
            handoff_lane
        );
    }

    let round2_prompt =
        prompts::opposite_coder_round2_prompt(task, winner_draft, loser_draft, &reviewer_raw);
    let round2_out = generate_35b(&round2_prompt, (temperature * 0.85).max(0.1), &round2_url).await;
    let output = if round2_out.is_empty() {
        winner_draft.clone()
    } else {
        round2_out
    };

    let grade_feedback = reviewer_raw
        .lines()
        .filter(|l| l.contains("FIX") || l.starts_with('-'))
        .take(12)
        .collect::<Vec<_>>()
        .join("\n");

    info!(
        "parallel_dual_coders: thinker_winner={} round2_lane={} url={}",
        winner, handoff_lane, round2_url
    );

    (
        output,
        format!(
            "parallel_dual_coders:r{round}_thinker={winner}_round2={handoff_lane}"
        ),
        grade_feedback,
    )
}

/// Call the 8B planner for pre-flight reasoning (dual-path uses a two-worker brief).
async fn get_thinker_preflight(message: &str, thinker_url: &str, dual_path: bool) -> String {
    let prompt = if dual_path {
        prompts::thinker_dual_prompt(message)
    } else {
        prompts::thinker_prompt(message)
    };
    let max_tokens = if dual_path { 384 } else { 256 };
    let client = reqwest::Client::new();

    match crate::inference_client::complete_prompt(
        &client,
        thinker_url,
        &prompt,
        max_tokens,
        0.5,
        vec!["<|im_end|>".to_string()],
        None,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            warn!("Thinker preflight failed: {}", e);
            String::new()
        }
    }
}

/// Build the full prompt from conversation state.
async fn build_prompt(state: &AppState, thinker_context: &str, failure_count: u32, chat_template: &str) -> String {
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

    prompts::format_for_template(chat_template, &system, &messages, true)
}

/// Generate from the main coder (llama-server OpenAI API by default).
async fn generate_35b(prompt: &str, temperature: f32, coder_url: &str) -> String {
    let client = reqwest::Client::new();

    match crate::inference_client::complete_prompt(
        &client,
        coder_url,
        prompt,
        16384,
        temperature,
        vec!["</tool_call>".to_string(), "<|endoftext|>".to_string()],
        None,
    )
    .await
    {
        Ok(text) => {
            if !text.contains("</tool_call>") && text.contains("<tool_call>") {
                format!("{}</tool_call>", text)
            } else {
                text
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

/// Hard reset after loop detection: keep the original task anchor, not the last tool result.
async fn hard_reset(state: &AppState) {
    let mut conv = state.conversation.lock().await;
    if conv.len() <= 3 {
        return;
    }
    let anchor = conv.first().cloned();
    let latest_real_user = conv
        .iter()
        .rev()
        .find(|m| m.role == "user" && !is_injected_context_note(&m.content))
        .cloned();
    conv.clear();
    if let Some(a) = anchor {
        conv.push(a);
    }
    if let Some(u) = latest_real_user {
        let duplicate = conv
            .last()
            .map(|m| m.content == u.content)
            .unwrap_or(false);
        if !duplicate {
            conv.push(u);
        }
    }
    conv.push(Message {
        role: "user".to_string(),
        content: "[LOOP RESET] Output was repeating. Continue the original task above. \
                  Summarize what you already did, then take the next step."
            .to_string(),
    });
    info!("Hard reset: kept task anchor + latest real user turn");
}

/// Short assistant turn for conversation history (avoid empty "assistant" bubbles in UI).
fn assistant_turn_summary(tool_name: &str, normalized_content: &str) -> String {
    let body = normalized_content.trim();
    if body.is_empty() {
        return format!("[called tool: {tool_name}]");
    }
    let short = if body.len() > 400 {
        format!("{}...", &body[..400])
    } else {
        body.to_string()
    };
    format!("[tool: {tool_name}] {short}")
}

fn format_tool_action(name: &str, args: &serde_json::Value) -> String {
    let key = match name {
        "write_file" | "read_file" | "cargo_check" => args.get("path").and_then(|v| v.as_str()),
        "think_harder" => args.get("query").and_then(|v| v.as_str()),
        "run_command" => args.get("cmd").and_then(|v| v.as_str()),
        _ => None,
    };
    if let Some(k) = key {
        let short = if k.len() > 80 { format!("{}...", &k[..80]) } else { k.to_string() };
        format!("{name}({short})")
    } else {
        name.to_string()
    }
}

fn is_injected_context_note(content: &str) -> bool {
    let c = content.trim_start();
    c.starts_with("[Tool Result]")
        || c.starts_with("[CONTEXT NOTE")
        || c.starts_with("[Forge —")
        || c.starts_with("[STEERING")
        || c.starts_with("[FORMAT FIX]")
        || c.starts_with("[AUTO-WSO")
        || c.starts_with("[CLOSE-ENOUGH")
        || c.starts_with("[COMPRESSED —")
        || c.starts_with("[LOOP")
        || c.starts_with("[TERMINATED]")
        || c.starts_with("[PLAN FROM THINKER]")
        || c.starts_with("[NAUTIVECS CONTEXT]")
}

fn build_session_recap(tool_actions: &[String], rounds: u32, context_trims: u32) -> String {
    if tool_actions.is_empty() {
        return String::new();
    }
    let mut files: Vec<String> = Vec::new();
    let mut counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for action in tool_actions {
        let base = action.split('(').next().unwrap_or(action).trim();
        *counts.entry(base.to_string()).or_insert(0) += 1;
        if let Some(path) = action.strip_prefix("write_file(close-enough-fix:") {
            files.push(path.trim_end_matches(')').to_string());
        } else if let Some(rest) = action.strip_prefix("write_file(") {
            files.push(rest.trim_end_matches(')').to_string());
        } else if let Some(rest) = action.strip_prefix("read_file(") {
            files.push(rest.trim_end_matches(')').to_string());
        }
    }
    files.sort();
    files.dedup();

    let top_tools: Vec<String> = {
        let mut v: Vec<_> = counts.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.into_iter()
            .take(8)
            .map(|(k, n)| format!("{k}×{n}"))
            .collect()
    };

    let trim_note = if context_trims > 0 {
        format!(" (light trim ×{context_trims})")
    } else {
        String::new()
    };

    let files_line = if files.is_empty() {
        "none detected".to_string()
    } else {
        files.join(", ")
    };

    format!(
        "\n\n---\n**Session recap** — {steps} tool steps in {rounds} rounds{trim_note}:\n\
         - Tools: {tools}\n\
         - Files touched: {files}\n",
        steps = tool_actions.len(),
        rounds = rounds,
        trim_note = trim_note,
        tools = top_tools.join(", "),
        files = files_line,
    )
}

fn rough_token_estimate(text: &str) -> usize {
    text.len().div_ceil(4)
}

fn conversation_size(conv: &[Message]) -> (usize, usize) {
    let chars = conv.iter().map(|m| m.content.len()).sum();
    let tokens = conv
        .iter()
        .map(|m| rough_token_estimate(&m.content) + 6)
        .sum();
    (chars, tokens)
}

fn first_real_user_task(conv: &[Message]) -> String {
    conv.iter()
        .find(|m| m.role == "user" && !is_injected_context_note(&m.content))
        .map(|m| m.content.clone())
        .unwrap_or_else(|| "Continue the current Forge task with accurate, concise state only.".to_string())
}

fn summarize_trimmed_messages(messages: &[Message]) -> String {
    let mut items: Vec<String> = Vec::new();
    for msg in messages {
        if is_injected_context_note(&msg.content) {
            continue;
        }
        let body = msg.content.split_whitespace().collect::<Vec<_>>().join(" ");
        let body = body.trim();
        if body.is_empty() {
            continue;
        }
        let prefix = if msg.role == "user" { "Task" } else { "State" };
        let short = safe_byte_prefix(body, 180).trim();
        let item = format!("- {}: {}", prefix, short);
        if !items.contains(&item) {
            items.push(item);
        }
        if items.len() >= CONV_SUMMARY_MAX_ITEMS {
            break;
        }
    }
    if items.is_empty() {
        "- Preserve the current task and the most recent concrete results only.".to_string()
    } else {
        items.join("\n")
    }
}

async fn maybe_compact_conversation_capacity(state: &AppState, round: u32) -> bool {
    let snapshot = { state.conversation.lock().await.clone() };
    let (chars, tokens) = conversation_size(&snapshot);
    let already_compacted = snapshot
        .iter()
        .rev()
        .take(6)
        .any(|m| m.content.starts_with("[CONTEXT CAPACITY CHECK"));

    if already_compacted || (snapshot.len() <= CONV_TRIM_MIN_LEN && chars <= CONV_CAPACITY_SOFT_CHARS && tokens <= CONV_CAPACITY_SOFT_TOKENS) {
        return false;
    }

    let anchor = snapshot.first().cloned();
    let tail: Vec<Message> = snapshot
        .iter()
        .rev()
        .take(CONV_COMPACT_KEEP_LAST)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    let start = usize::from(anchor.is_some());
    let end = snapshot.len().saturating_sub(tail.len());
    let trimmed_slice = if end > start {
        &snapshot[start..end]
    } else {
        &[]
    };
    let recap = summarize_trimmed_messages(trimmed_slice);
    let task = first_real_user_task(&snapshot);
    let trim_request = format!(
        "Round {round} is nearing context capacity (~{tokens} tokens / {chars} chars). Remove fluff, thought traces, and handoff chatter. Keep only accurate project state and next actions.\n\nProject brief:\n{recap}"
    );
    let tuned = orchestration::request_prompt_trim(&task, &trim_request).await;

    let mut compact_note = format!(
        "[CONTEXT CAPACITY CHECK — round {round}] Approaching context limit (~{tokens} tokens / {chars} chars). Keep the broad project overview, but strip fluff, internal thought traces, and redundant handoffs.\n\n[PROJECT BRIEF]\n{recap}"
    );
    if let Some(tuned_prompt) = tuned {
        compact_note.push_str("\n\n[n8n trim guide]\n");
        compact_note.push_str(tuned_prompt.trim());
    }

    let mut conv = state.conversation.lock().await;
    let current_tail: Vec<Message> = conv
        .iter()
        .rev()
        .take(CONV_COMPACT_KEEP_LAST)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    conv.clear();
    if let Some(a) = anchor {
        conv.push(a);
    }
    conv.push(Message {
        role: "user".to_string(),
        content: compact_note,
    });
    conv.extend(current_tail);
    true
}

fn sanitize_user_response(raw: &str) -> String {
    translator::sanitize_for_user(raw)
}

/// True when the model exited without a real user-facing answer.
fn is_unusable_assistant_reply(content: &str) -> bool {
    let t = content.trim();
    if t.is_empty() {
        return true;
    }
    if t.starts_with("[tool:") || t.starts_with("[called tool:") {
        return true;
    }
    if t.eq_ignore_ascii_case("<thought>") || t.starts_with("<thought>") && t.len() < 120 {
        return true;
    }
    if looks_like_token_spam(t) {
        return true;
    }
    false
}

/// Detect runaway repetition (e.g. "term-term-term" from regurgitated nautivecs JSON).
fn looks_like_token_spam(s: &str) -> bool {
    if s.len() < 200 {
        return false;
    }
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() < 30 {
        return false;
    }
    let repeated = words
        .windows(2)
        .filter(|w| w[0] == w[1] && w[0].len() > 3)
        .count();
    repeated > words.len() / 4
        || s.matches("term-term").count() > 5
        || s.matches("mission_id_id").count() > 3
}

fn finalize_response(
    mut content: String,
    tool_actions: &[String],
    diagnosis: Option<String>,
    rounds: u32,
    context_trims: u32,
) -> SendResponse {
    content = sanitize_user_response(&content);
    if is_unusable_assistant_reply(&content) && !tool_actions.is_empty() {
        content = format!(
            "I ran {} tool step(s) ({}) but the model did not produce a readable summary. \
             Clear the chat and retry, or use `/fast` for a direct answer. \
             Check nautivecs (`curl -s http://127.0.0.1:5003/health`) and the reference doc \
             `docs/nautinferer-production-phase2.md`.",
            tool_actions.len(),
            tool_actions.join(", ")
        );
    }
    if !tool_actions.is_empty() {
        content.push_str(&build_session_recap(tool_actions, rounds, context_trims));
    }
    SendResponse {
        response: content,
        tool_actions: tool_actions.to_vec(),
        diagnosis,
        accepted: false,
    }
}

/// Route a malformed tool call through the corrector cascade.
/// Tries each corrector in order until one fixes it.
/// Returns (tool_name, result, correction_note, corrector_id) or None.
async fn correct_and_execute_cascade(
    raw_output: &str,
    stripped_content: &str,
    corrector_url: &str,
    state: &AppState,
) -> Option<(String, String, String, String)> {
    // Build the corrector URL list: config corrector first, then cascade fallbacks
    let mut endpoints: Vec<(String, String)> = Vec::new();

    // Primary from config (may be disabled/empty)
    let primary = corrector_url;
    if !primary.is_empty() && !primary.contains("0.0.0.0") {
        endpoints.push(("config-corrector".to_string(), primary.to_string()));
    }

    // Cascade fallbacks
    for (id, url) in CORRECTOR_CASCADE {
        if !endpoints.iter().any(|(_, u)| u == url) {
            endpoints.push((id.to_string(), url.to_string()));
        }
    }

    for (corrector_id, corrector_url) in &endpoints {
        info!("Trying corrector '{}' at {}", corrector_id, corrector_url);
        if let Some(result) = try_corrector(raw_output, stripped_content, corrector_url, corrector_id, state).await {
            return Some(result);
        }
        warn!("Corrector '{}' failed or unreachable, trying next", corrector_id);
    }

    warn!("All correctors exhausted — cannot fix malformed tool call");
    None
}

/// If correction fails repeatedly, hand control back to the dispatcher role
/// (typically thinker) with a compact recovery plan for the next round.
async fn build_dispatcher_handoff(
    state: &AppState,
    malformed_output: &str,
) -> Option<String> {
    let thinker_url = state.config.read().await.thinker_url.clone();
    if thinker_url.is_empty() || thinker_url.contains("0.0.0.0") {
        return None;
    }

    let prompt = format!(
        "You are the dispatcher recovery role. The worker failed tool-call JSON correction after 3 demanded retries.\n\
         Return 2-3 bullet points with: (1) why likely failed, (2) exact next action, (3) one valid next tool name to try.\n\
         Keep it concise and operational.\n\
         Failed output sample:\n{}",
        safe_byte_prefix(malformed_output, 500)
    );

    let client = reqwest::Client::new();
    crate::inference_client::complete_prompt(
        &client,
        &thinker_url,
        &prompt,
        180,
        0.1,
        vec!["\n\n\n".to_string(), "<|im_end|>".to_string()],
        None,
    )
    .await
    .ok()
}

/// Try a single corrector endpoint. Returns None if unreachable or can't fix.
async fn try_corrector(
    raw_output: &str,
    stripped_content: &str,
    corrector_url: &str,
    corrector_id: &str,
    state: &AppState,
) -> Option<(String, String, String, String)> {
    let client = reqwest::Client::new();

    // Per-model tuning: small models need strict, short prompts; larger models
    // get fuller instructions but lower coercion.
    let (preset, model_hint) = corrector_profile_for_endpoint(corrector_url, corrector_id);
    let tiny_hint = corrector_id.contains("tiny") || corrector_id.contains("picasso") || corrector_id.contains("laptop");
    let strict_tool_contract = tiny_hint || matches!(preset.correction_strength, CorrectionStrength::Heavy);
    let max_length = match preset.correction_strength {
        CorrectionStrength::None => 700,
        CorrectionStrength::Light => 560,
        CorrectionStrength::Medium => 420,
        CorrectionStrength::Heavy => 260,
    };
    let fix_temperature = if strict_tool_contract { 0.02 } else { 0.08 };
    info!(
        "Corrector profile '{}' model='{}' strict={} strength={:?}",
        corrector_id,
        model_hint,
        strict_tool_contract,
        preset.correction_strength
    );

    let mut last_bad_reply = String::new();
    for demand_try in 1..=3 {
        let demand_msg = if demand_try > 1 {
            let esc = Escalation::Demand(format!(
                "Attempt {}/3 failed. You MUST output one valid JSON tool call now.",
                demand_try
            ));
            corrector_preset::format_escalation_message(&esc).unwrap_or_else(|| {
                "Demand: output one valid JSON tool call only. No explanation.".to_string()
            })
        } else {
            String::new()
        };

        let fix_prompt = if strict_tool_contract {
            // TinyLlama / small model — ultra-minimal prompt
            format!(
                "Fix this JSON tool call. Output ONLY valid JSON, nothing else.\n\
                 Format: {{\"name\": \"tool_name\", \"arguments\": {{\"key\": \"value\"}}}}\n\
                 Tools: write_file, read_file, cargo_check, think_harder, remember, run_command, speed_check, list_fleet_agents, call_sub_agent, delegate_sub_agent, scan_region, magnetic_dipole_detect, download_satellite_window, weather_window, detection_health, detection_scan, detection_poll, sat_mission, sat_read_mission_report, search_symbols, get_symbol, get_file_context, search_text, replace_symbol_body, edit_within_symbol, insert_symbol, delete_symbol, batch_edit, batch_rename\n\
                 Input: {}\n\
                 Previous bad reply: {}\n\
                 {}\n\
                 Do not explain. Do not apologize. Return JSON only.\n\
                 Fixed JSON:",
                &stripped_content[..stripped_content.len().min(300)],
                safe_byte_prefix(&last_bad_reply, 200),
                demand_msg
            )
        } else {
            // Full corrector prompt for 14B+
            format!(
                r#"<|im_start|>system
You are a tool-call JSON fixer. Output ONLY the corrected JSON. Nothing else.
Format: {{"name": "tool_name", "arguments": {{"key": "value"}}}}
Tools: write_file, read_file, cargo_check, think_harder, remember, run_command, speed_check, list_fleet_agents, call_sub_agent, delegate_sub_agent, scan_region, magnetic_dipole_detect, download_satellite_window, weather_window, detection_health, detection_scan, detection_poll, sat_mission, sat_read_mission_report, search_symbols, get_symbol, get_file_context, search_text, replace_symbol_body, edit_within_symbol, insert_symbol, delete_symbol, batch_edit, batch_rename
{demand_msg}
<|im_end|>
<|im_start|>user
Fix: {stripped_content}
Original model output sample: {raw_sample}
Previous bad reply: {last_bad}
<|im_end|>
<|im_start|>assistant
"#,
                demand_msg = demand_msg,
                stripped_content = stripped_content,
                raw_sample = safe_byte_prefix(raw_output, 250),
                last_bad = safe_byte_prefix(&last_bad_reply, 220)
            )
        };

        let stops = if strict_tool_contract {
            vec!["\n\n".to_string(), "```".to_string()]
        } else {
            vec!["<|im_end|>".to_string(), "\n\n".to_string()]
        };

        let fixed_text = crate::inference_client::complete_prompt(
            &client,
            corrector_url,
            &fix_prompt,
            max_length,
            fix_temperature,
            stops,
            None,
        )
        .await
        .ok()?;

        // Extract JSON from the response (may have surrounding text for small models)
        let json_text = extract_json_from_text(&fixed_text);
        let normalized = translator::normalize(&json_text);
        if let Some(ref tool_call) = normalized.tool_call {
            info!(
                "Corrector '{}' fixed tool call on demand attempt {}/3: {}",
                corrector_id,
                demand_try,
                tool_call.name
            );
            let result = tools::execute(&tool_call.name, &tool_call.arguments, state).await;
            let correction_note = format!(
                "Your tool call JSON was malformed. Fixed by {} after {}/3 demand attempts. \
                 Correct format: {{\"name\": \"{}\", \"arguments\": {{...}}}}",
                corrector_id,
                demand_try,
                tool_call.name
            );
            return Some((
                tool_call.name.clone(),
                result,
                correction_note,
                corrector_id.to_string(),
            ));
        }

        last_bad_reply = fixed_text;
        warn!(
            "Corrector '{}' did not return valid tool JSON (attempt {}/3)",
            corrector_id,
            demand_try
        );
    }

    warn!(
        "Corrector '{}' exhausted 3 demand attempts; stopping this model and returning control",
        corrector_id
    );
    None
}

/// Extract the first JSON object from a string (handles small models that add surrounding text).
fn extract_json_from_text(text: &str) -> String {
    // Try to find { ... } pattern
    if let Some(start) = text.find('{') {
        let substr = &text[start..];
        let mut depth = 0i32;
        let mut end = 0;
        for (i, c) in substr.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if end > 0 {
            return substr[..end].to_string();
        }
    }
    text.to_string()
}

/// Ask the corrector cascade whether a repeated tool call is justified.
async fn check_repetition_justification_cascade(
    tool_name: &str,
    tool_history: &[String],
    output_history: &[String],
    corrector_url: &str,
    state: &AppState,
) -> RepetitionJudgment {
    let recent_outputs = output_history.iter().rev().take(2)
        .map(|s| s.chars().take(150).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n---\n");
    let recent_tools = tool_history.iter().rev().take(5).cloned().collect::<Vec<_>>().join(", ");

    // Build endpoint list
    let mut endpoints: Vec<(String, String)> = Vec::new();
    let primary = corrector_url;
    if !primary.is_empty() && !primary.contains("0.0.0.0") {
        endpoints.push(("config-corrector".to_string(), primary.to_string()));
    }
    for (id, url) in CORRECTOR_CASCADE {
        if !endpoints.iter().any(|(_, u)| u == url) {
            endpoints.push((id.to_string(), url.to_string()));
        }
    }

    for (corrector_id, corr_url) in &endpoints {
        let (preset, model_hint) = corrector_profile_for_endpoint(corr_url, corrector_id);
        let tiny_hint = corrector_id.contains("tiny") || corrector_id.contains("picasso") || corrector_id.contains("laptop");
        let strict_tool_contract = tiny_hint || matches!(preset.correction_strength, CorrectionStrength::Heavy);

        let prompt = if strict_tool_contract {
            format!(
                "Is calling '{}' again justified? Recent calls: [{}]. Recent output: {}...\n\
                 Answer ALLOW or BLOCK with one reason.",
                tool_name, recent_tools, &recent_outputs[..recent_outputs.len().min(100)]
            )
        } else {
            format!(
                r#"<|im_start|>system
Loop detection judge. Respond ONLY: ALLOW: <reason> or BLOCK: <reason>
<|im_end|>
<|im_start|>user
Tool repeated: "{}" | Recent: [{}]
Output: {}
Justified?<|im_end|>
<|im_start|>assistant
"#,
                tool_name, recent_tools, &recent_outputs[..recent_outputs.len().min(150)]
            )
        };

        let client = reqwest::Client::new();
        if let Ok(text) = crate::inference_client::complete_prompt(
            &client,
            corr_url,
            &prompt,
            if strict_tool_contract { 48 } else { 72 },
            if strict_tool_contract { 0.0 } else { 0.1 },
            vec!["\n\n".to_string(), "<|im_end|>".to_string()],
            None,
        )
        .await
        {
            let text = text.trim().to_string();
            info!(
                "Repetition judgment from '{}' model='{}': {}",
                corrector_id,
                model_hint,
                &text[..text.len().min(80)]
            );

            if text.to_uppercase().starts_with("BLOCK") {
                let reason = text.splitn(2, ':').nth(1).unwrap_or(&text).trim().to_string();
                return RepetitionJudgment { allowed: false, reason };
            } else {
                let reason = text.splitn(2, ':').nth(1).unwrap_or(&text).trim().to_string();
                return RepetitionJudgment { allowed: true, reason };
            }
        }
        // This corrector failed, try next
    }

    // All correctors unreachable — allow by default
    RepetitionJudgment { allowed: true, reason: "all correctors unreachable".to_string() }
}

/// Assessment of whether a code file is "close enough" to complete.
struct CloseEnoughFix {
    description: String,
    fixed_content: Option<String>,
}

/// Check if a code file is close enough to complete that we should fix it ourselves.
/// Returns Some(fix) if we can patch it, None if it needs a full round.
fn assess_close_enough(content: &str, path: &str) -> Option<CloseEnoughFix> {
    if path.ends_with(".rs") {
        return assess_rust_close_enough(content);
    }
    if path.ends_with(".wgsl") {
        return assess_wgsl_close_enough(content);
    }
    None
}

fn assess_rust_close_enough(content: &str) -> Option<CloseEnoughFix> {
    // Count unmatched braces
    let open = content.chars().filter(|&c| c == '{').count() as i32;
    let close = content.chars().filter(|&c| c == '}').count() as i32;
    let brace_diff = open - close;

    // Check for truncated function (ends mid-function without closing)
    let ends_cleanly = content.trim_end().ends_with('}')
        || content.trim_end().ends_with("};")
        || content.trim_end().ends_with(")\n}");

    if brace_diff > 0 && brace_diff <= CLOSE_ENOUGH_MISSING_BRACES && !ends_cleanly {
        // Missing closing braces — append them
        let mut fixed = content.to_string();
        // Add a newline if needed
        if !fixed.ends_with('\n') { fixed.push('\n'); }
        for _ in 0..brace_diff {
            fixed.push_str("}\n");
        }
        return Some(CloseEnoughFix {
            description: format!("appended {} missing closing braces", brace_diff),
            fixed_content: Some(fixed),
        });
    }

    // Check for file that ends with a comment or doc string (truncated mid-write)
    let trimmed = content.trim_end();
    if trimmed.ends_with("//") || trimmed.ends_with("///") || trimmed.ends_with("/*") {
        // Truncated comment — remove it and close
        let without_comment = trimmed.trim_end_matches('/').trim_end_matches('*').trim_end();
        let mut fixed = without_comment.to_string();
        if !fixed.ends_with('\n') { fixed.push('\n'); }
        // Close any open braces
        let open2 = fixed.chars().filter(|&c| c == '{').count() as i32;
        let close2 = fixed.chars().filter(|&c| c == '}').count() as i32;
        for _ in 0..(open2 - close2).max(0) {
            fixed.push_str("}\n");
        }
        return Some(CloseEnoughFix {
            description: "removed truncated comment and closed open braces".to_string(),
            fixed_content: Some(fixed),
        });
    }

    None
}

fn assess_wgsl_close_enough(content: &str) -> Option<CloseEnoughFix> {
    let open = content.chars().filter(|&c| c == '{').count() as i32;
    let close = content.chars().filter(|&c| c == '}').count() as i32;
    let diff = open - close;

    if diff > 0 && diff <= CLOSE_ENOUGH_MISSING_BRACES {
        let mut fixed = content.to_string();
        if !fixed.ends_with('\n') { fixed.push('\n'); }
        for _ in 0..diff {
            fixed.push_str("}\n");
        }
        return Some(CloseEnoughFix {
            description: format!("appended {} missing WGSL closing braces", diff),
            fixed_content: Some(fixed),
        });
    }
    None
}

/// Keep the old single-corrector function for backward compat (now delegates to cascade).
async fn correct_and_execute(
    raw_output: &str,
    stripped_content: &str,
    state: &AppState,
) -> Option<(String, String, String)> {
    let corrector_url = state.config.read().await.corrector_url.clone();
    correct_and_execute_cascade(raw_output, stripped_content, &corrector_url, state)
        .await
        .map(|(name, result, note, _id)| (name, result, note))
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
    let compress_at = if tool_name == "think_harder" {
        900
    } else {
        TOOL_RESULT_COMPRESS_AT
    };
    if full_result.len() <= compress_at {
        return (full_result.to_string(), false);
    }

    let first = safe_byte_prefix(full_result, TOOL_RESULT_PREVIEW_HEAD);
    let last = safe_byte_suffix(full_result, TOOL_RESULT_PREVIEW_TAIL);
    let lines = full_result.lines().count();

    let summary = format!(
        "[LARGE TOOL OUTPUT — full text stored in memory; use think_harder if you need details]\n\
         Tool: {}({})\n\
         Size: {} chars, {} lines\n\
         Head: {}...\n\
         Tail: ...{}",
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
        safe_byte_prefix(full_result, 3000)
    );
    let remember_args = serde_json::json!({
        "content": remember_content,
        "tags": format!("tool_result,{},context_overflow", tool_name)
    });
    let _ = tools::execute("remember", &remember_args, state).await;

    (summary, true)
}

/// Light trim: only when very long; always keep the first user task (anchor).
/// Returns true if messages were removed.
fn light_trim_conversation(conv: &mut Vec<Message>) -> bool {
    if conv.len() <= CONV_TRIM_MIN_LEN {
        return false;
    }

    let before = conv.len();
    let anchor = conv.first().cloned();
    let tail: Vec<Message> = conv
        .iter()
        .rev()
        .take(CONV_KEEP_LAST)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    conv.clear();
    if let Some(a) = anchor {
        conv.push(a);
    }
    conv.push(Message {
        role: "user".to_string(),
        content: "[CONTEXT NOTE: lightly trimmed older messages; original task kept. \
                  Large tool output may be in memory — think_harder to recall. Continue the task.]"
            .to_string(),
    });
    conv.extend(tail);
    let dropped = before.saturating_sub(conv.len());
    if dropped > 0 {
        if let Some(note) = conv.get_mut(1) {
            note.content = format!(
                "[CONTEXT NOTE: lightly trimmed {dropped} older messages; original task kept. \
                 Large tool output may be in memory — think_harder to recall. Continue the task.]"
            );
        }
    }
    dropped > 0
}
