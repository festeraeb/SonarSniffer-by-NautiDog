# White Paper Steering Context — Human-in-the-Loop Corrections

## CORRECTIONS (Your previous draft had these errors)

1. **HALLUCINATION: constants.rs does NOT exist.** The drift thresholds (0.05/0.15) are defined in `src/scm/drift.rs` in the `SteeringMode` enum's `threshold()` method. The unsteered threshold (0.02) is a constant in `src/scm/pipeline.rs`.

2. **WRONG: P100 VRAM is 16GB each, not 12GB.** The T440 has dual Tesla P100-PCIE-16GB = 32GB total HBM2. Check `nvidia-smi` output: "Tesla P100-PCIE-16GB, 14792 MiB used, 16384 MiB total".

3. **MISSING: The novel contributions.** Your draft described WHAT the system does but not WHY it's novel. The key innovations are:
   - Step B (1-token LLM-as-Judge) that overrides false-positive keyword drift
   - The Momentum Rule (keep retrying as long as accuracy improves, halt on plateau)
   - Self-Critique mode (force the model to explain WHY it failed before retrying)
   - Nautivecs grounding search (query local vector store to produce concrete constraints)

## FILES YOU MUST READ

Read these files to ground your claims in actual project history:

1. `.kiro/specs/segmented-context-manager/requirements.md` — The 12 formal requirements
2. `.kiro/specs/segmented-context-manager/design.md` — The full architecture with Mermaid diagrams
3. `.kiro/specs/segmented-context-manager/tasks.md` — The 42 tasks executed across 11 waves
4. `SCM_RUN7_ANALYSIS.txt` — Analysis of the first successful pipeline run
5. `src/scm/drift.rs` — The actual drift thresholds and DriftMonitor trait
6. `src/scm/pipeline.rs` — The PipelineCoordinator with Step B, Momentum Rule, Self-Critique
7. `src/scm/steering_ctrl.rs` — The SteeringController with judge_drift() method
8. `src/scm/segmenter.rs` — The grounding_search() function
9. `src/scm/guardrail.rs` — Key Term Weighting and prefix/stem matching

## ACTUAL METRICS FROM THE SUCCESSFUL RUN

These are REAL numbers from the production run on T440:

- **Total segments processed:** 7
- **Average drift score:** 0.013 (target was < 0.05)
- **Total retries:** 0 (after segment_001's initial calibration)
- **Budget exhaustion count:** 0
- **Segments by mode:** 5 precision, 2 exploratory
- **Total elapsed time:** 1,208,727ms (20.1 minutes)
- **Hardware:** Dual P100 16GB (32GB HBM2), Dual Xeon E5-2630v3, 94GB DDR4
- **Model:** Qwen2.5-Coder-14B-Instruct Q4_K_M at ~18 T/s
- **nautivecs chunks indexed:** 383 (from src/scm/)
- **Test suite:** 150 tests, 0 failures across 12 modules
- **Build tasks:** 42 tasks across 11 parallel waves, all completed

## THE BUILD JOURNEY (Include this narrative)

The SCM was built iteratively through these phases:
1. **42 tasks executed** in 11 waves without context collapse
2. **First test run:** InputGuardrail rejected segment_001 (0% keyword overlap)
3. **Fix 1:** Key Term Weighting + prefix/stem matching in guardrail.rs
4. **Second test run:** AccuracyCheck kept saying FAIL (keyword drift 1.0)
5. **Fix 2:** Step B (1-token LLM-as-Judge) — judge_score=5/5 overrode false positive
6. **Third test run:** CrossSegmentValidator blocked (same keyword issue)
7. **Fix 3:** Step B applied to CrossValidator — segment_001 finally committed
8. **Fix 4:** Momentum Rule — keep retrying as long as scores improve
9. **Fix 5:** Self-Critique — force model to explain failure before retrying
10. **Fix 6:** Nautivecs grounding search — inject real code entities into constraints
11. **Final run:** 7 segments completed, average drift 0.013, zero retries after calibration

## HARDWARE TOPOLOGY (Use these exact specs)

| Node | Role | Hardware | IP (Tailscale) |
|------|------|----------|----------------|
| T440 | Orchestrator + Generator | Dual Xeon E5-2630v3, 94GB DDR4, Dual P100 16GB | 100.72.182.77 |
| cesarops2 | Auditor | GTX 1070 8GB + Quadro P1000 4GB | 100.102.158.111 |
| cesarops3 | Reserve | (available) | 100.105.77.74 |
| Pi | Sentinel | Raspberry Pi 4 | 100.127.66.32 |

## WHAT MAKES THIS PAPER NOVEL

1. **Zero cloud tokens at inference** — all validation (keyword overlap + Step B judge) runs locally
2. **$2000 hardware outperforms cloud** — formal accuracy guarantees via closed-loop validation
3. **Self-correcting pipeline** — Momentum Rule + Self-Critique = model improves without human intervention
4. **Hardware-aware design** — Context TTL eviction specifically for Pascal-era VRAM constraints
5. **The system wrote its own white paper** — this is proof of the architecture working

## TONE AND DEPTH

- Write like an academic paper, not a summary
- Include code snippets from the actual source
- Show the evolution: problem → failed attempt → fix → success
- Every claim must cite a specific file and line/function
- Include the Mermaid architecture diagram from design.md
- Discuss limitations honestly (the keyword-only scorer's weakness, the 18 T/s speed constraint)
