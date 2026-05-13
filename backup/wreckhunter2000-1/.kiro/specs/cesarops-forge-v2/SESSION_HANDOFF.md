# Session Handoff — Build cesarops-forge-v2

## TASK: Build the complete Self-Healing Knowledge Translator

You are continuing from a previous session. The spec is fully designed. Your job is to IMPLEMENT it as a complete 7-file Rust crate, deploy to T440, test, and run the sonar sniffer analysis.

## READ THESE FILES FIRST (on disk):
- `.kiro/specs/warp-grid/self-healing-translator.md` — the full architecture spec
- `.kiro/specs/cesarops-inference/requirements.md` — Burn engine spec (deploy if SHKT works)
- `.kiro/specs/cesarops-inference/design.md` — Burn architecture
- `research_log/lessons_learned.md` — indexed knowledge (Qwen quirks, NUMA rules, etc.)
- `cesarops-forge-v2/Cargo.toml` — already written
- `cesarops-forge-v2/src/main.rs` — already written (orchestrator shell)
- `cesarops-forge-web/src/index.html` — reuse this HTML frontend

## WHAT EXISTS ON T440 (RAID at /codebase/wreckhunter2000-1/):
- KoboldCPP running on :5001 (Qwen3.6-35B-A3B MoE on dual P100s)
- DeepSeek-R1-8B on cesarops2:5555 (1070, the "thinker")
- nautivecs on :5003 (12,606 chunks indexed)
- cesarops-wso on :5010 (web search)
- Old forge-web on :9100 (systemd, will be replaced)
- warp-grid-standalone/ (compiles clean — types, numa, mem, metrics, translator, pool)

## THE 7 FILES TO BUILD:

### 1. `src/main.rs` (DONE — just needs the HTML include path fixed)
Axum server, routes, state management.

### 2. `src/translator.rs` — The Rosetta Stone
- `normalize(raw_output, model_name)` → `NormalizedMessage`
- Strips: `<think>`, `<reasoning>`, prompt leaks (`<|im_start|>` etc.)
- Extracts tool calls from `<tool_call>JSON</tool_call>`
- Detects failure states: ThinkOnly, Empty, RepeatedOutput
- `format_tool_result_for_qwen(result, round, max)` → wraps as user message:
  `<|im_end|>\n<|im_start|>user\n[Tool Result - Round N/M]: {result}\nNow continue.<|im_end|>\n<|im_start|>assistant\n`
- N-gram loop detection (same output 80%+ similar to previous = loop)

### 3. `src/diagnostics.rs` — The 8B Psychiatrist
- `diagnose(failed_prompt_tail, raw_output, failure_type)` → hits 8B on cesarops2:5555
- Categorizes failure: Incomplete / Hallucination / ToolMisuse
- Searches nautivecs for historical fixes before diagnosing
- Returns: `DiagnosisResult { explanation, prompt_override, confidence }`
- Max 2 diagnosis attempts per failure, then hard reset
- If confidence < 0.8, uses `think_harder` (web search) for external help

### 4. `src/tools.rs` — The Hands
- `write_file(path, content)` — writes to project_root, creates dirs
- `read_file(path)` — reads, truncates at 3000 chars
- `cargo_check(dir)` — runs with --message-format=json, parses nested errors
- `think_harder(query)` — searches nautivecs + WSO, combines results
- `remember(content, tags)` — appends to research_log/lessons_learned.md
- `run_command(cmd)` — K-line guard (blocks rm -rf, dd), truncates output
- All return String (the tool result)

### 5. `src/memory.rs` — The Learning Loop
- `auto_remember_success(diagnosis, fix_applied, model_name)` — saves successful corrections
- `search_prior_fixes(failure_type)` → queries nautivecs for tagged fixes
- `prioritize_web_over_hallucination(results)` — web-crawled fixes rank higher
- Background indexing via tokio::spawn (doesn't block main loop)

### 6. `src/hardware.rs` — Health Monitoring
- `query_gpu_metrics()` — nvidia-smi CSV parsing (compatible with driver 580.x)
- `estimate_register_pressure(wgsl_source)` — count var/let lines
- AVX-512 throttle awareness (informational, for future use)

### 7. `src/loop_engine.rs` — The Main Loop (Strategy-Execution-Verification)
```
1. Thinker fires (8B, 30s timeout) — unless /fast
2. Build prompt with QwenChatML + /no_think + thinker context
3. Generate (35B) with stop_sequence: ["</tool_call>", "<|im_end|>"]
4. Translator normalizes output
5. If tool_call found:
   a. Execute tool
   b. Format result as USER message (QwenChatML format)
   c. Continue loop (max 12 rounds)
6. If think-only detected:
   a. Translator searches nautivecs for prior fix
   b. If found → apply fix, retry
   c. If not found → send to 8B diagnostics
   d. 8B diagnoses, provides prompt_override
   e. Translator injects override, retries with temp spike (0.7)
   f. If success → auto_remember the fix
   g. If fail after 2 diagnoses → hard reset (clear recent history)
   h. If still fails → return [Model reasoning] content
7. If plain text response → return to user
8. Repeated tool call detection → force summary after 2 repeats
9. Temperature spike + top_p widening on repeats
```

### 8. `src/prompts.rs` — System Prompt Templates
- SYSTEM_PROMPT with /no_think directive and tool definitions
- The "Snarky Clippy" escalation (low snark → high snark based on failure count)
- QwenChatML formatting helpers

## KEY TECHNICAL DECISIONS:
- Tool results go as `<|im_start|>user` messages (NOT custom XML tags)
- `/no_think` added to system prompt to disable Qwen3.6 thinking mode
- Pre-fill `<think>\n</think>\n` before assistant turn as backup
- 8B has Instructional Authority (rewrites prompts) but NOT System Authority
- Translator is the single source of truth for conversation state
- Every successful diagnosis → auto-remember to nautivecs
- Max 12 tool rounds, max 2 diagnosis attempts, then hard reset

## AFTER BUILD — TEST SEQUENCE:
1. `cargo check` in cesarops-forge-v2/ (must compile clean)
2. `cargo build --release`
3. scp to T440, kill old forge-web, start new one on :9100
4. Send simple test: "What tools do you have?"
5. If response comes back (not think-only) → SUCCESS
6. Send sonar sniffer audit prompt
7. If assessment comes back → deploy Burn/Cake spec as next task

## IF IT DOESN'T WORK:
- Drop KoboldCPP entirely
- Build cesarops-inference with Burn (spec at .kiro/specs/cesarops-inference/)
- The self-healing translator still applies — it's backend-agnostic
- Use pmetal-gguf for mmap weight loading
- burn-wgpu with spirv feature for fused kernels on P100
- PagedAttention for distributed KV cache

## CLUSTER ACCESS:
- T440: ssh cesarops@100.72.182.77 (password: cesarops)
- cesarops2: ssh cesarops@100.102.158.111
- cesarops3: ssh cesarops@100.105.77.74
- All on Tailscale
- RAID: /codebase/wreckhunter2000-1/
- sudo password: cesarops
