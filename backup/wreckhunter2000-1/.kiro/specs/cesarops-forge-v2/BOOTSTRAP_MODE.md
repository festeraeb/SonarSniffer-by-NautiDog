# Bootstrap Mode — Kiro as Pass-Through Corrector

## Hardware Config for Bootstrap

| Role | Model | Hardware | Node | Port |
|------|-------|----------|------|------|
| Main Worker | Qwen3.6-35B-A3B MoE MXFP4 | Dual P100 32GB (T440) | 100.72.182.77 | 5001 |
| Translator/Corrector | Qwen2.5-Coder-14B Q4_K_M | 1070 8GB (cesarops2) | 100.102.158.111 | 5555 |
| Thinker | DeepSeek-R1-1.5B or small | P1000/P106 (cesarops3) | 100.105.77.74 | 5556 |
| Overseer | Kiro (Claude) | Cloud — ONLY when system fails | — | — |

**The 14B Coder is the TRANSLATOR/CORRECTOR — not just a thinker:**
- Sits BETWEEN the thinker and the 35B main worker
- When 35B produces a malformed tool call → 14B fixes the JSON, executes the tool
- Passes the correct result back to 35B WITH a correction note
- Loads the correct format into nautivecs so 35B has examples to learn from
- This is the "execute anyway, teach after" pattern proven on 4B

**The experiment:**
- If 35B learns from the corrections (starts self-correcting) → process works, model is capable
- If 35B NEVER learns despite correct examples in KV + nautivecs → it's the model, not the process
- From previous work: not all models of the same weight class are equal. Far from it.
- This definitively answers: "Is Qwen3.6 MoE capable of tool calling, or do we need a different model?"

## The "Execute Anyway, Teach After" Pattern

The key insight (proven on 4B coder): DON'T reject malformed tool calls.
Execute them, return the result, AND include the correction inline.
The model sees "I did it wrong → got corrected → here's the right way" in its KV.
After 5-8 corrections, it starts producing correct format unprompted.

### The "Coder Tugboat" Extension

The 35B MoE stalls because it's trying to hold SAR drift math, Rust syntax,
AND tool schema all in active HBM2 at once. Solution: decouple Strategy from Syntax.

- **The Brain (35B MoE on P100s)**: Analyzes, reasons, determines WHAT needs to happen
- **The Specialist (14B Coder on 1070)**: Receives the MoE's "intent" and converts it
  into a flawless, syntactically correct `<tool_call>` JSON
- **The Translator (forge-v2)**: If the MoE stalls or outputs garbage, detects the failure
  and hands the ball to the Coder: "The big guy is stuck. Here's what he wants to do;
  you write the tool call."

This is a **Model Swap Nudge** — instead of text nudges, we route to a specialist.
The Coder doesn't care about the SAR mission; it only cares about Tool Call Schema.

### Why This Definitively Tests the Model

If after all this (14B corrector + nautivecs examples + in-KV corrections):
- 35B learns → process works, Qwen3.6 MoE is capable
- 35B never learns → it's the model, swap it out
- From previous work: not all models of the same weight class are equal. Far from it.

```
Spec Chunk (small, focused)
    ↓
[Kiro] → sends to Thinker for pre-flight planning
    ↓
[Kiro] → sends to 14B Coder (cesarops2:5555) with thinker context
    ↓
[14B responds with tool call]
    ↓
    If tool call is WRONG:
        [Kiro] → fixes the JSON, executes the tool anyway
        [Kiro] → feeds result back WITH correction note:
                  "Tool executed. NOTE: Your JSON was malformed.
                   Correct format: {\"name\": \"X\", \"arguments\": {...}}
                   Do it right next time."
    ↓
    If tool call is RIGHT:
        [Kiro] → executes tool, feeds result back normally
    ↓
[35B continues building]
    ↓
After enough corrections in KV:
    → Model starts self-correcting (proven on 4B coder)
    → Eventually needs zero corrections
```

## Why This Works

1. **KV as Training Data**: Every correction lives in the context window. The model sees "wrong → correction → right" patterns accumulate.
2. **Proven on 4B**: A 4B coder model started self-correcting after ~5-8 corrections in KV. The 35B MoE should learn faster.
3. **No Retraining Needed**: This is pure in-context learning. The model adapts within a single session.
4. **Kiro Never Blocks**: Even if the tool call is malformed, Kiro fixes and executes it. Progress never stops.

## NEXT SESSION: Build Burn/Cake Inference Engine

**NO MORE TESTS. Go straight to building.**

The multi-model setup (thinker + 35B + 14B coder) is training wheels.
Once Burn is live with unified KV and logit-level control:
- No need for multiple models
- Creativity/grounding baked into the sampler (logit filtering)
- Zero-copy between LLM and SAR pipeline
- Millions of tokens of context via tiered KV (HBM2 → GDDR5 → DDR4 → RAID)

### Kiro's Role: Overseer
- Feed spec chunks to the system
- If code breaks → fix it, keep moving
- If flow breaks → reconnect the dots
- The system should build itself. Kiro only intervenes on failure.

### Implementation Order (from design.md):
1. `hardware.rs` — IronProfile audit
2. `loader.rs` — GGUF parser + GridBuffer mapping
3. `bridge.rs` — GridBuffer ↔ Burn Tensor zero-copy
4. `attention.rs` — Multi-head attention with RoPE
5. `transformer.rs` — Forward pass using Burn
6. `moe.rs` — Expert routing + cross-GPU dispatch
7. `kv_cache.rs` — Overflow to DDR4
8. `sampling.rs` — Temperature, top_p, rep_pen + logit filtering (<think> killer)
9. `tokenizer.rs` — Qwen tokenizer
10. `server.rs` — Axum API (drop-in KoboldCPP replacement)

## Correction Templates

When Qwen produces malformed JSON:
```
[CORRECTION]: Your tool call JSON was malformed.
You wrote: {"name": "write_file", {"arguments": {"path": "..."}}}
Correct:   {"name": "write_file", "arguments": {"path": "..."}}
The tool was executed anyway. Fix your format for the next call.
Pattern: {"name": "TOOL_NAME", "arguments": {"key": "value"}}
```

When Qwen calls a non-existent tool:
```
[CORRECTION]: Tool "compile_check" does not exist.
Available tools: write_file, read_file, cargo_check, think_harder, remember, run_command
Did you mean "cargo_check"? Executing cargo_check instead.
```

When Qwen produces think-only:
```
[NUDGE]: You produced only reasoning with no action.
You MUST either:
1. Call a tool: <tool_call>{"name": "...", "arguments": {...}}</tool_call>
2. Provide a final text answer.
Do NOT just think. ACT.
```

## Kiro's Role (Next Session)

1. Read the spec chunk
2. Format it as a task for the 8B thinker
3. Send thinker output + task to 35B via forge-v2 /send endpoint
4. Intercept the response
5. If tool call malformed → fix, execute, feed back with correction
6. If tool call correct → execute, feed back normally
7. If think-only → nudge and retry
8. After each file is written → run cargo check → feed errors back
9. Repeat until the crate compiles clean

## Success Metric

The 35B starts producing correct tool calls without Kiro corrections.
At that point, the SHKT is self-sufficient and Kiro can step back to observer mode.

## The Creativity-Grounding Tradeoff (CRITICAL DESIGN PHILOSOPHY)

Over-grounding kills novel ideas. Under-grounding produces hallucinations.
When someone might prep a dive based on your output, hallucinations are dangerous.
But if you stamp out all creativity, you miss the patterns nobody else would find.

### The Three-Brain Solution:

**Thinker (P1000/P106) — CREATIVE, UNGROUNDED**
- High temperature, no format constraints
- Allowed to explore wild angles: "what if the seiche timing correlates with..."
- Its job is to think of shit we might have missed
- NO tool calling, NO code generation — pure ideation
- This is where novel wreck detection approaches come from

**35B MoE (P100s) — BALANCED, GUIDED**
- Receives thinker's creative spark as context
- Reasons about the problem with both creative input AND grounded corrections
- Attempts to execute — may get syntax wrong, that's OK
- The thinker prevents it from being a boring code monkey
- The coder prevents it from hallucinating coordinates

**14B Coder (1070) — GROUNDED, MECHANICAL**
- Zero creativity needed or wanted
- Pure syntax: take intent → produce correct JSON → execute → verify
- cargo check is the only judge
- Loads correct patterns into nautivecs for the 35B to learn from
- This is the safety net that prevents dangerous hallucinations

### The Rule:
- Creativity flows DOWN (thinker → 35B)
- Grounding flows UP (coder → 35B)
- The 35B is the synthesis point — creative ideas, grounded execution
- NEVER flatten the thinker into a grounding layer
- NEVER let the coder make strategic decisions

### Why This Matters for SAR:
A hallucinated wreck coordinate could send a diver into danger.
The grounding layer (14B + nautivecs + compiler) exists so the creative layer
can run wild WITHOUT the output being dangerous.
Creativity in → Grounded output out.
