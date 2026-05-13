---
inclusion: auto
---

# Accuracy Steering: Segmented Thinking Guardrails

## Principles

1. **Atomic Validation**: Every segment must be validated before moving to the next. Do not proceed to synthesis until all sub-queries have returned findings.
2. **Context Preservation**: Cross-reference the output of the current segment against the primary prompt. If the segment drifts from the original intent, flag it with `[DRIFT_DETECTED]`.
3. **No Shortcuts**: If a segment lacks data, flag it with `[MISSING_CONTEXT: what is needed]` rather than hallucinating. Never invent function names, struct names, or parameter values.
4. **Citation Required**: Every code reference must include the source file path and line range. No anonymous code blocks.
5. **Correction Priority**: Any text marked `[CRITICAL: PREVIOUS HUMAN CORRECTION]` overrides all other data including the codebase context.

## Implementation Patterns

- **Decompose First**: Complex queries get broken into 3-5 sub-queries before any code is written.
- **Parallel Fetch**: Sub-queries to nautivecs (local code) and external oracles (web/API) run simultaneously.
- **Sectional Synthesis**: Build the answer one finding at a time. Each finding is validated before the next section begins.
- **Confidence Gate**: If the nautivecs match score is below 0.7, do not proceed — log to n8n and wait for human input.
- **Stop on Conflict**: If web research contradicts local code context, output `[CONFLICT: describe the disagreement]` and ask the human.

## Anti-Drift Rules for Code Generation

- Use exact types from the codebase. If you see `f32`, do not write `f64`.
- If a function is not in the nautivecs context, output `[MISSING_CONTEXT: function_name]`.
- Parameter values must reference ranges from actual code shown in context.
- The LLM is a **Librarian**, not an **Author**. It finds and assembles — it does not invent.

## Hardware Awareness

- This workspace uses wgpu 29.x with WGSL shaders on Vulkan.
- Storage: nautivecs v0.1.0 serverless JSON store. No LanceDB or Arrow.
- Dual NVIDIA P100 GPUs for heavy compute (when T440 is online).
- Pascal-era GPUs (1060/1070/P1000) for LLM inference and worker tasks.
- Coral TPU for fast glint/jitter prepass (when available).
