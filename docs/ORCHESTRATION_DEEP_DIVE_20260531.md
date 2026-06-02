# Orchestration Deep Dive (2026-05-31)

## Objective

Keep the current 3-model lineup and increase task scores by tightening the closed-loop orchestration.

Current lineup to keep:
- thinker: DeepSeek-R1-Distill-Qwen-14B-IQ4_XS
- coder: qwen2.5-coder-7b-instruct-q4_k_m
- reviewer/corrector: gemma-2-9b-it-IQ4_XS

## What is already strong

1. Multi-layer orchestration exists:
- planner preflight (thinker)
- dual-path generation + thinker winner pick
- corrector presets by model family
- loop blocking and hard caps

2. Score-aware endpoint selection exists in routing:
- routing can pick endpoint by per-task scorecard confidence.

3. Corrector now has demand-and-handoff:
- malformed JSON path: 3 demanded retries, then dispatcher handoff
- repeated-loop path: 3 demanded retries, pattern memory write, optional code knowledge injection, then dispatcher handoff

## Main missing pieces (high impact)

1. Scorecard write path is missing at runtime
- `Scorecard::record` exists but runtime appears to only read scorecard for routing and metrics.
- Result: routing confidence cannot improve materially over time from actual run outcomes.

2. Endpoint separation is not consistently enforced in A/B evidence
- Recent probes often resolved to a single endpoint (`5202`) even in specialized mode.
- Result: route A/B comparisons are noisy; score gains are hard to attribute.

3. Dual-round feedback loop was present but weakly fed back
- Winner decision existed; actionable grader feedback was not consistently injected.
- This has now been improved in `loop_engine`.

4. Retrieval policy was under-specified for code regressions
- Need explicit policy: code/build blockers should force retrieval-first next step (think_harder + symbol/file context), then concrete tool action.
- This has now been partially wired for loop-block demand exhaustion.

## Are we forging our own path?

Yes, but in a good way.

This is a hybrid of:
- Router + scorer selection
- Self-correction with escalating constraints
- Memory-augmented retry guidance

What is custom/novel here:
- Tight local tool-first orchestration with per-model corrector behavior
- Explicit demand ladder with hard role handoff instead of only timeout/fallback
- In-loop pattern memory writes for orchestration failures

## Best place for pattern knowledge

Use this order:

1. Nautivecs first (primary)
- Best for fast local recall inside the same loop.
- Lowest latency and already integrated with think_harder.

2. Thinker guidance second
- Best for strategic rewrite of next-step instructions after failures.
- Should consume pattern notes and emit strict next actions.

3. External vector MCP third
- Use for cross-repo/cross-domain retrieval not covered by local index.
- Keep as fallback, not first-hop, to avoid latency and instruction drift.

## Score increase plan (with same 3 models)

Phase A (already applied today)
1. Corrector demand ladder + dispatcher return on malformed JSON.
2. Same demand ladder on repeated-loop blocking.
3. Inject dual-round grader feedback back into next round.
4. Persist loop-block patterns to memory tags.

Phase B (next patch, highest impact)
1. Add runtime scorecard recording from loop outcomes:
- task type (from role/task)
- pass/fail by round
- whether corrector/vector/translator helped
- failure note category
2. Persist after each run so routing score confidence can converge.

Phase C (validation discipline)
1. Enforce endpoint split in A/B mode before grading.
2. Run fixed benchmark set (P1/P2 + 3 additional code/research/json-repair tasks).
3. Compare confidence uplift and pass-rate deltas per task type.

## Success criteria

1. Scorecard history grows every run (not static).
2. Endpoint_used differs between same/specialized when expected.
3. P1 and P2 median score increases by >=10 points over 3 controlled batches.
4. Loop-blocked retries decrease over time for repeated task families.
