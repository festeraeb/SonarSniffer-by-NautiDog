# Self-Healing Translator — Complete Spec

## The Problem

Models get stuck. They produce only `<think>` blocks, repeat tool calls, or output nothing useful after receiving tool results. This happens regardless of backend (KoboldCPP, Burn, Ollama, any future engine). We need a universal recovery system.

## The Solution

A three-layer system that sits between every model interaction:

```
User Message
    ↓
[Layer 1: THINKER] — 8B reasons about approach (pre-flight)
    ↓
[Layer 2: TRANSLATOR] — formats prompt for target model's dialect
    ↓
[Layer 3: EXECUTOR] — 35B generates with tools
    ↓
[Layer 2: TRANSLATOR] — normalizes output, detects failures
    ↓
    If failure detected:
    ↓
[Layer 4: DIAGNOSTIC] — 8B analyzes WHY it failed
    ↓
[Layer 2: TRANSLATOR] — rewrites prompt based on diagnosis
    ↓
[Layer 3: EXECUTOR] — 35B retries with corrected format
    ↓
    If success:
    ↓
[Layer 5: MEMORY] — saves the correction to nautivecs
    ↓
Response to User
```

## Layer 1: Thinker (Pre-flight)

- Fires the 8B BEFORE the 35B
- Produces: approach strategy, what to search, pitfalls to avoid
- Output injected into the 35B's prompt as context
- Skip with `/fast` prefix

## Layer 2: Translator (The Rosetta Stone)

### Input Formatting (prompt → model)

Different models need different formats for tool results:

```rust
pub enum ToolResultFormat {
    /// Qwen3.6: wrap as a user message with clear "now respond" signal
    QwenChatML,
    /// DeepSeek: wrap as observation in ReAct format
    DeepSeekReAct,
    /// Llama: wrap as system context
    LlamaSystem,
    /// Generic: XML tags (current approach)
    GenericXML,
}

impl Translator {
    /// Format a tool result for the target model
    fn format_tool_result(&self, result: &str, round: u32, max_rounds: u32) -> String {
        match self.active_format {
            ToolResultFormat::QwenChatML => {
                // Qwen responds better when tool results come as user messages
                format!(
                    "<|im_end|>\n<|im_start|>user\n[Tool Output - Round {}/{}]: {}\n\
                    Now continue. Either call another tool or provide your final answer.<|im_end|>\n\
                    <|im_start|>assistant\n",
                    round, max_rounds, result
                )
            }
            ToolResultFormat::GenericXML => {
                format!("<tool_result>[Round {}/{}] {}</tool_result>\n", round, max_rounds, result)
            }
            // ... other formats
        }
    }
}
```

### Output Normalization (model → system)

Strips ALL model-specific artifacts and produces a clean `NormalizedMessage`:
- Strips `<think>`, `<reasoning>`, prompt leaks
- Extracts tool calls from any format (XML, JSON, markdown)
- Detects failure states (empty content, think-only, repeated output)

### Format Learning

The translator starts with `GenericXML`. If it fails:
1. 8B diagnoses the issue
2. If diagnosis says "use user message format" → translator switches to `QwenChatML`
3. Saves the successful format to nautivecs: `remember("Qwen3.6 needs tool results as user messages, not XML tags", "qwen,format,tool_result")`
4. Next session: `think_harder("qwen tool result format")` finds this lesson immediately

## Layer 3: Executor (The 35B)

- Receives pre-formatted prompt from translator
- Generates with stop sequences
- Output goes back through translator for normalization
- No changes needed here — it just generates

## Layer 4: Diagnostic (The 8B Psychiatrist)

When the translator detects a failure:

```rust
pub struct DiagnosticRequest {
    /// What was sent to the 35B
    pub prompt_tail: String,
    /// What came back (raw)
    pub raw_output: String,
    /// What the translator detected
    pub failure_type: FailureType,
    /// How many times we've tried to fix this
    pub attempt: u32,
}

pub enum FailureType {
    ThinkOnly,          // Only <think> blocks, no content
    EmptyResponse,      // Nothing generated
    RepeatedToolCall,   // Same tool called 2+ times
    MalformedToolCall,  // Tool call JSON is broken
    PromptLeak,         // Model leaked system tokens
}
```

The 8B receives the diagnostic request and outputs:
1. **Why** it failed (e.g., "the XML tag confused it into thinking it's still in system mode")
2. **What format to try** (e.g., "wrap the result as a user message instead")
3. **A rewritten prompt snippet** that the translator can inject directly

### Max Diagnosis Attempts: 2

If the 8B can't fix it in 2 tries:
- Hard reset: clear recent conversation history
- Switch to a different `ToolResultFormat`
- If ALL formats fail: return `[STUCK]` with the 8B's best diagnosis and let the user decide

## Layer 5: Memory (The Learning Loop)

When a diagnosis SUCCEEDS (35B responds correctly after the fix):

```rust
// Auto-remember the successful correction
execute_tool("remember", &json!({
    "content": format!(
        "Model '{}' failed with {:?}. Fix: {}. New format: {:?}",
        model_name, failure_type, diagnosis, new_format
    ),
    "tags": format!("{},format_fix,{:?}", model_name, failure_type)
}), state).await;
```

This means:
- First time a model gets stuck: takes 2-3 rounds to diagnose and fix
- Second time same pattern: `think_harder` finds the fix instantly, applies it without diagnosis
- Third time: never happens (translator already knows the correct format)

## N-Gram Loop Detection

```rust
/// Detects if the model is producing the same output repeatedly
fn detect_output_loop(history: &[String], current: &str) -> bool {
    // Check if current output matches any of the last 3 outputs
    history.iter().rev().take(3).any(|prev| {
        // Fuzzy match: if 80%+ of lines are identical, it's a loop
        let similarity = calculate_line_similarity(prev, current);
        similarity > 0.8
    })
}
```

If detected: immediate hard reset of recent history + format switch.

## Implementation Structure

```
cesarops-forge-web/src/
├── main.rs           — HTTP handlers, startup
├── translator.rs     — Layer 2: format in/out, normalization, format learning
├── thinker.rs        — Layer 1: pre-flight reasoning (8B)
├── diagnostic.rs     — Layer 4: failure analysis (8B)
├── memory.rs         — Layer 5: auto-remember on success
├── executor.rs       — Layer 3: generate + tool execution
├── tools.rs          — Tool implementations (write_file, cargo_check, etc.)
└── loop.rs           — The main orchestration loop tying all layers together
```

## Success Criteria

1. The sonar sniffer audit completes with actual content (not think-only)
2. If the 35B gets stuck, the 8B diagnoses and fixes it within 2 attempts
3. The fix is saved to nautivecs
4. On the NEXT run of the same type of task, it works first try (lesson learned)
5. Works with KoboldCPP today, Burn tomorrow, any backend ever

## Why This Is Better Than "Just Switch to Burn"

- Burn gives token-level control but doesn't prevent ALL failure modes
- Models can still get confused by context, produce garbage, or loop on patterns
- The diagnostic layer catches failures that token suppression can't prevent
- It's a universal safety net that makes ANY model more reliable
- It learns and improves over time without retraining
