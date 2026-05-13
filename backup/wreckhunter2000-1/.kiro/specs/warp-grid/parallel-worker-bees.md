# Parallel Worker Bee Architecture — Fan-Out / Merge Pattern

## Concept

Instead of sequential (8B thinks → 35B executes → 8B reviews), fire ALL models simultaneously on different aspects of the same task, merge their outputs, then execute once with full context.

## The Pattern

```
User sends message
    │
    ├── Worker 1 (8B R1 on 1070): "Think about this problem"
    │   → Returns: approach strategy, what to search, pitfalls
    │
    ├── Worker 2 (35B MoE on P100s): "Search nautivecs for relevant code"
    │   → Returns: code snippets, existing patterns
    │
    ├── Worker 3 (4B Qwen on P106): "Search web for latest docs/issues"
    │   → Returns: crate docs, GitHub issues, SO answers
    │
    └── [All three run in parallel via tokio::join!]
         │
         ▼
    MERGE: Combine all three results into one rich context
         │
         ▼
    EXECUTE: 35B generates the final response/code with ALL context
         │
         ▼
    VALIDATE: cargo_check or 8B review (if code was written)
```

## Why This Is Faster

- Sequential: 8B (30s) → 35B search (5s) → 35B generate (60s) → 8B review (30s) = **125s**
- Parallel: max(8B: 30s, 35B search: 5s, 4B web: 10s) → 35B generate (60s) = **90s**
- Savings: ~30% faster per task, compounds over overnight runs

## Implementation (Rust)

```rust
async fn parallel_fan_out(
    message: &str,
    thinker_url: &str,   // 8B on 1070
    coder_url: &str,     // 35B on P100s
    scout_url: &str,     // 4B on P106
    nautivecs_url: &str,
    wso_url: &str,
) -> MergedContext {
    // Fire all three simultaneously
    let (thinker_result, code_context, web_context) = tokio::join!(
        // Worker 1: 8B thinks about the problem
        get_thinker_spark(thinker_url, message),
        // Worker 2: Search codebase for patterns
        search_nautivecs(nautivecs_url, &extract_keywords(message)),
        // Worker 3: Search web for docs/issues
        search_web(wso_url, &extract_keywords(message)),
    );

    MergedContext {
        thinker_analysis: thinker_result,
        code_patterns: code_context,
        web_findings: web_context,
    }
}

struct MergedContext {
    thinker_analysis: String,  // 8B's reasoning spark
    code_patterns: String,     // nautivecs results
    web_findings: String,      // WSO/web results
}

// Then the 35B gets ONE prompt with everything:
fn build_enriched_prompt(msg: &str, ctx: &MergedContext) -> String {
    format!(
        "[Senior Architect Analysis]: {}\n\n\
         [Relevant Code Patterns]: {}\n\n\
         [Latest Documentation]: {}\n\n\
         User Request: {}\n\n\
         Now execute using tools.",
        ctx.thinker_analysis,
        ctx.code_patterns,
        ctx.web_findings,
        msg
    )
}
```

## Worker Bee Roles

| Worker | Hardware | Model | Role | Max Tokens | Timeout |
|--------|----------|-------|------|-----------|---------|
| Thinker | 1070 8GB | DeepSeek-R1-8B | Strategy, pitfalls, approach | 512 | 30s |
| Coder | P100s 32GB | Qwen3.6-35B MoE | Code generation + tool execution | 4096 | 300s |
| Scout | P106 6GB | Qwen-4B | Web search, doc lookup, pattern matching | 256 | 15s |
| Validator | 1070 8GB | DeepSeek-R1-8B | Code review (post-execution) | 1024 | 60s |

## Hardware Utilization

With parallel fan-out:
- 1070: busy during fan-out (thinking) AND during validation (reviewing)
- P100s: busy during execution (the heavy lift)
- P106: busy during fan-out (web search) — otherwise idle
- Xeons: handle nautivecs search (CPU-bound, no GPU needed)

All hardware working simultaneously. No idle GPUs during the thinking phase.

## Scaling

When you add the M10 (4x 8GB GPUs):
- Worker 1-4 on M10: four parallel thinkers with different prompts
  - "Think about architecture"
  - "Think about edge cases"
  - "Think about performance"
  - "Think about security"
- Merge all four perspectives → 35B executes with 4x the insight

When you add V100s:
- V100 becomes the new Coder (Tensor Cores, faster generation)
- P100s become additional Thinkers or Validators
- The fan-out pattern stays the same — just more workers

## Prerequisites

- P106 needs KoboldCPP running with a 4B model (already has the GGUF on shared models)
- All three endpoints must be reachable from T440
- The merge function needs to truncate combined context to fit 35B's 32K window
- Extract keywords function: take the 5 longest non-stopwords from the user message

## When to Implement

After the sequential thinker-first version is proven stable:
1. Start KoboldCPP on cesarops3 with 4B Qwen
2. Add `scout_url` to AppState
3. Replace the sequential `get_thinker_spark` call with `parallel_fan_out`
4. Test with the sonar sniffer audit task
5. Compare quality and speed vs sequential
