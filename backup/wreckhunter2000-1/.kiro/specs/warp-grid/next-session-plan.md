# Next Session Plan — Complete Self-Healing Translator Build

## Decisions Made (all three agreed):

1. **Rebuild from scratch** — 7-file structure, not refactor
2. **8B has instructional authority** — provides PromptOverride, translator injects it
3. **Tool results as QwenChatML user messages** — `<|im_start|>user\n<tool_result>{data}</tool_result><|im_end|>\n<|im_start|>assistant`
4. **/no_think in prompt** — disables Qwen3.6 thinking mode at the prompt level
5. **Translator is self-healing** — has its own think_harder (web search) to find fixes
6. **Auto-remember on success** — every successful correction saved to nautivecs
7. **Burn comes AFTER** — the translator/diagnostic layer must work first (backend-agnostic)

## Immediate Fix to Test First (before full rebuild):

Add `/no_think` to the system prompt and switch tool_result format to user messages.
This might fix the think-only issue WITHOUT a full rebuild. Test it first.

## The 7-File Structure:

```
cesarops-forge-web/src/
├── main.rs           — HTTP handlers, startup, state
├── translator.rs     — Format in/out, normalization, format learning, self-healing search
├── thinker.rs        — Pre-flight reasoning (8B on 1070)
├── diagnostic.rs     — Failure analysis (8B), prompt rewriting
├── memory.rs         — Auto-remember on success, nautivecs integration
├── executor.rs       — Generate + tool execution (35B on P100s)
├── tools.rs          — Tool implementations (write_file, cargo_check, etc.)
└── loop.rs           — Main orchestration loop tying all layers together
```

## Key Architectural Decisions:

- Translator is the SINGLE SOURCE OF TRUTH for conversation state
- 8B suggests, translator decides (Director vs Script Supervisor)
- Max 2 diagnostic attempts, then hard reset
- N-gram loop detection: same output twice = immediate format switch
- Format learning: starts with QwenChatML, adapts based on 8B diagnosis
- All lessons saved to research_log/lessons_learned.md (nautivecs indexes it)

## What's Already Working (don't break these):

- KoboldCPP on P100s (35B Qwen3.6 MoE)
- DeepSeek-R1 8B on 1070 (thinker/diagnostic)
- nautivecs (12,606 chunks, lessons_learned.md ready)
- WSO (web search)
- All services on systemd with auto-restart
- Repo on RAID at /codebase/wreckhunter2000-1
- warp-grid compiles (types, numa, mem, metrics, translator, pool)
- Shaders deployed (pascal/matmul_half2.wgsl, generic/matmul_f32.wgsl)
- nauticuvs f64 precision compiled

## Test Case: Sonar Sniffer Audit

The first task after rebuild:
"Audit the Sonar Sniffer at /codebase/projects/cesarops/rust/sonarsniffer/"

Success = actual assessment with file contents and prioritized fix list.
Failure = think-only or empty response.

## Gemini's Key Findings:

- Qwen3.6 supports `/no_think` to disable thinking mode
- Tool results should be in user turn (not custom XML tags)
- `enable_thinking: false` parameter may work in some KoboldCPP builds
- Logit bias: set -100 on <think> token ID as nuclear option
- The translator should prioritize web-crawled fixes over internal hallucinations
