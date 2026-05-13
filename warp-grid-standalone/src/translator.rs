//! Universal Translator — normalizes output from ANY model into a standard format.
//!
//! Every model speaks differently:
//! - Qwen: wraps reasoning in <think>...</think>
//! - DeepSeek-R1: uses chain-of-thought before answering
//! - Phi: terse, sometimes forgets closing tags
//! - Llama: verbose, leaks prompt tokens
//!
//! The translator sits between every model handoff and normalizes everything
//! into a NormalizedMessage that any downstream consumer can use.
//!
//! Runs on CPU only. No GPU needed. Pure regex parsing.

use regex::Regex;
use serde::{Serialize, Deserialize};

/// The universal output format. Every model's raw output gets normalized into this.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedMessage {
    /// The model's internal reasoning (stripped from output, saved for logging)
    pub reasoning: String,
    /// The actual actionable content (what the next model/tool should see)
    pub content: String,
    /// Extracted tool calls (if any)
    pub tool_calls: Vec<ToolAction>,
    /// Who produced this and on what hardware
    pub source: MessageSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAction {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSource {
    pub model: String,
    pub device: String,
    pub tokens_used: u32,
}

/// Known reasoning tag patterns across different model families
const REASONING_PATTERNS: &[(&str, &str)] = &[
    ("<think>", "</think>"),           // Qwen 3.x
    ("<reasoning>", "</reasoning>"),   // DeepSeek-R1
    ("<thought>", "</thought>"),       // Some Phi variants
    ("(thinking)", "(done)"),          // Informal chain-of-thought
    ("<|thinking|>", "<|/thinking|>"), // Future models
];

/// Known prompt artifact tokens that leak into output
const PROMPT_ARTIFACTS: &[&str] = &[
    "<|im_start|>",
    "<|im_end|>",
    "<|assistant|>",
    "<|user|>",
    "<|system|>",
    "<|endoftext|>",
    "[INST]",
    "[/INST]",
    "<<SYS>>",
    "<</SYS>>",
];

/// Normalize raw model output into a clean, universal format.
/// This is the Rosetta Stone — call it on EVERY model output before passing downstream.
pub fn normalize(raw: &str, source_model: &str, source_device: &str) -> NormalizedMessage {
    let mut reasoning = String::new();
    let mut content = raw.to_string();
    let mut tool_calls: Vec<ToolAction> = Vec::new();

    // Step 1: Extract reasoning blocks (save them, remove from content)
    for (open_tag, close_tag) in REASONING_PATTERNS {
        let pattern = format!(r"(?s){}(.*?){}", regex::escape(open_tag), regex::escape(close_tag));
        if let Ok(re) = Regex::new(&pattern) {
            for cap in re.captures_iter(&content.clone()) {
                if let Some(thought) = cap.get(1) {
                    reasoning.push_str(thought.as_str().trim());
                    reasoning.push('\n');
                }
            }
            content = re.replace_all(&content, "").to_string();
        }
    }

    // Step 2: Strip prompt artifacts (leaked tokens)
    for artifact in PROMPT_ARTIFACTS {
        content = content.replace(artifact, "");
    }

    // Step 3: Extract tool calls (multiple formats supported)
    // Format 1: <tool_call>{"name": "...", "arguments": {...}}</tool_call>
    if let Ok(re) = Regex::new(r"(?s)<tool_call>\s*(.*?)\s*</tool_call>") {
        for cap in re.captures_iter(&content.clone()) {
            if let Some(json_str) = cap.get(1) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str.as_str()) {
                    let name = val["name"].as_str().unwrap_or("unknown").to_string();
                    let args = val.get("arguments").cloned().unwrap_or_default();
                    tool_calls.push(ToolAction { name, arguments: args });
                }
            }
        }
        content = re.replace_all(&content, "").to_string();
    }

    // Format 2: ```json\n{"tool": "...", ...}\n``` (some models use code blocks)
    if let Ok(re) = Regex::new(r"(?s)```json\s*\n(.*?)\n```") {
        for cap in re.captures_iter(&content.clone()) {
            if let Some(json_str) = cap.get(1) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str.as_str()) {
                    if val.get("name").is_some() || val.get("tool").is_some() {
                        let name = val["name"].as_str()
                            .or_else(|| val["tool"].as_str())
                            .unwrap_or("unknown").to_string();
                        let args = val.get("arguments")
                            .or_else(|| val.get("args"))
                            .cloned()
                            .unwrap_or_default();
                        tool_calls.push(ToolAction { name, arguments: args });
                    }
                }
            }
        }
    }

    // Step 4: Clean up whitespace
    content = content.trim().to_string();
    reasoning = reasoning.trim().to_string();

    // Step 5: If content is empty but we have reasoning, promote reasoning to content
    if content.is_empty() && !reasoning.is_empty() {
        content = reasoning.clone();
    }

    NormalizedMessage {
        reasoning,
        content,
        tool_calls,
        source: MessageSource {
            model: source_model.to_string(),
            device: source_device.to_string(),
            tokens_used: 0, // Filled by caller if known
        },
    }
}

/// Format a NormalizedMessage as input for the NEXT model in the chain.
/// Adapts to the target model's expected prompt format.
pub fn format_for_target(msg: &NormalizedMessage, target_format: PromptFormat) -> String {
    match target_format {
        PromptFormat::ChatML => {
            // Qwen, most modern models
            format!("<|im_start|>assistant\n{}<|im_end|>\n", msg.content)
        }
        PromptFormat::Llama => {
            // Llama 2/3 format
            format!("[INST] {} [/INST]\n", msg.content)
        }
        PromptFormat::Raw => {
            // No formatting — just the content
            msg.content.clone()
        }
        PromptFormat::ToolResult => {
            // Format as a tool result for the next model to consume
            format!("<tool_result>\n{}\n</tool_result>\n", msg.content)
        }
    }
}

/// Compress a message if it's too long for the next model's context window.
/// Keeps the first and last N chars, summarizes the middle.
pub fn compress_for_context(msg: &NormalizedMessage, max_chars: usize) -> NormalizedMessage {
    if msg.content.len() <= max_chars {
        return msg.clone();
    }

    let keep_start = max_chars / 3;
    let keep_end = max_chars / 3;
    let start = &msg.content[..keep_start];
    let end = &msg.content[msg.content.len() - keep_end..];

    let compressed_content = format!(
        "{}...\n[COMPRESSED: {} chars omitted]\n...{}",
        start,
        msg.content.len() - keep_start - keep_end,
        end
    );

    NormalizedMessage {
        reasoning: msg.reasoning.clone(),
        content: compressed_content,
        tool_calls: msg.tool_calls.clone(),
        source: msg.source.clone(),
    }
}

/// Supported prompt formats for different model families
#[derive(Debug, Clone, Copy)]
pub enum PromptFormat {
    ChatML,     // Qwen, most modern models
    Llama,      // Llama 2/3
    Raw,        // No formatting
    ToolResult, // Formatted as tool output
}
