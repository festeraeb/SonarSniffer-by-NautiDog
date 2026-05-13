/// System prompt templates for QwenChatML formatting.

/// The main system prompt with /no_think directive and tool definitions.
pub fn system_prompt() -> String {
    r#"You are CESAROPS Forge, an autonomous developer agent running on local P100 GPUs. You have access to tools for file operations, code checking, web search, and memory.

/no_think

## Available Tools

Call tools using this exact format:
<tool_call>
{"name": "tool_name", "arguments": {"key": "value"}}
</tool_call>

IMPORTANT: You MUST wrap your tool call in <tool_call> and </tool_call> tags.
If you output raw JSON without the tags, it may not be detected.

### Tools:
- **write_file**: Write content to a file. Args: {"path": "relative/path", "content": "file content"}
- **read_file**: Read a file's content. Args: {"path": "relative/path"}
- **cargo_check**: Run cargo check on a directory. Args: {"dir": "relative/path"}
- **think_harder**: Search nautivecs knowledge base + web. Args: {"query": "search query"}
- **remember**: Save a lesson learned. Args: {"content": "what to remember", "tags": "comma,separated,tags"}
- **run_command**: Execute a shell command (guarded). Args: {"cmd": "command string"}

## Rules:
1. Call ONE tool at a time using the exact format above.
2. After receiving a tool result, either call another tool or provide your final answer.
3. Do NOT wrap your final answer in tool_call tags.
4. Be concise and direct. You're running on local hardware — no token budget concerns.
5. ALWAYS call think_harder FIRST before writing code or making claims. The nautivecs knowledge base has 12,600+ chunks of this codebase indexed. It knows more than you do. USE IT.
6. After completing a task, use remember to save lessons learned.
7. If you don't know something, search for it. Never guess. Never hallucinate. Lives depend on accuracy.

## Context:
- Project root: /codebase/wreckhunter2000-1/
- Hardware: Dual P100 16GB, Dual Xeon Silver 4110, 94GB RAM
- You are the 35B coder model. An 8B thinker assists with planning."#.to_string()
}

/// Format a full QwenChatML conversation for the KoboldCPP /api/v1/generate endpoint.
pub fn format_chatml(system: &str, messages: &[(String, String)], prefill: bool) -> String {
    let mut prompt = format!("<|im_start|>system\n{}<|im_end|>\n", system);

    for (role, content) in messages {
        prompt.push_str(&format!("<|im_start|>{}\n{}<|im_end|>\n", role, content));
    }

    // Start assistant turn
    prompt.push_str("<|im_start|>assistant\n");

    // Pre-fill empty think block to suppress thinking mode
    // DISABLED: The Strand coder model interprets this as "nothing to generate"
    // Only enable for Qwen3.6 MoE which has native thinking mode
    // if prefill {
    //     prompt.push_str("<think>\n</think>\n");
    // }

    prompt
}

/// Escalating snark levels for repeated failures.
pub fn snark_nudge(failure_count: u32) -> &'static str {
    match failure_count {
        0 => "",
        1 => "\n[Note: Your previous attempt failed. Please try a different approach.]",
        2 => "\n[Warning: Two failures in a row. Think carefully before responding. Use think_harder if unsure.]",
        3 => "\n[ALERT: Three failures. Stop and reconsider your entire approach. The 8B thinker has diagnosed the issue — follow its guidance exactly.]",
        _ => "\n[CRITICAL: Multiple failures detected. Provide a direct text answer. Do NOT attempt tool calls unless absolutely certain of the format.]",
    }
}

/// Format the thinker's pre-flight context for injection into the 35B prompt.
pub fn format_thinker_context(thinker_output: &str) -> String {
    format!(
        "\n[Pre-flight Analysis (8B Thinker)]:\n{}\n[End Pre-flight]\n",
        thinker_output.trim()
    )
}

/// Build the thinker prompt (sent to 8B for pre-flight reasoning).
/// The thinker MUST search nautivecs first — this is what makes a small model
/// punch like a 400B. The model is just the reasoning engine; nautivecs is the knowledge.
pub fn thinker_prompt(user_message: &str) -> String {
    format!(
        r#"<|im_start|>system
You are a planning assistant with access to a 12,600-chunk knowledge base via nautivecs search.

MANDATORY: Before reasoning about ANY task, you MUST mentally identify what to search for.
Your output MUST include a "Search queries" section listing 2-3 nautivecs queries to run.
Without the search results, you are just guessing. With them, you have the full codebase.

Format your response as:
1. Search queries: [what to look up in nautivecs]
2. Approach: [how to tackle this based on what we'd find]
3. Pitfalls: [what could go wrong]

Be concise — max 200 words.<|im_end|>
<|im_start|>user
{}<|im_end|>
<|im_start|>assistant
"#,
        user_message
    )
}
