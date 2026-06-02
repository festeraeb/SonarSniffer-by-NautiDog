# Thinker plan test — GTX 1070 / Gemma-4-E4B Q4

**When:** 2026-06-01  
**Endpoint:** `http://127.0.0.1:5202` (only load on 1070; Phi/triple cleared)  
**Model:** `gemma-4-E4B-it-Q4_K_M.gguf` (~4.7G, `-ngl 99`, ctx 8192)  
**VRAM:** ~3680 MiB on GTX 1070  
**Latency:** 578 words in 28.5s  
**Prompt:** `var/role_bench/manual_round1_prompt.txt` (script cleanup / `forge-verdict.json`)

## Grade: **84 / 100**

| Dimension | Score | Notes |
|-----------|-------|-------|
| Structure (5 sections) | 10/10 | Goal, Constraints, Architecture, Handoff, Risks — all present |
| Repo specificity | 7/10 | Uses `var/script-inventory/latest/manifest.json`, staging path; no `fleet/` / `forge-health/` path callouts like P100 winner |
| Actionable handoff | 9/10 | 3 numbered tasks + per-task acceptance criteria; logging + `reason` field |
| Classification logic | 7/10 | Defers rules to “design spec separately”; P100 Gemma gave explicit tier mapping (`review→archive`) |
| Safety / NFS | 9/10 | No-delete, no Nomad breakage, defensive parsing called out |
| Length / discipline | 10/10 | 578 words, no full code, under 700 |

**Heuristic rubric (`score_thinker`):** 100.0 — structure/length/handoff all pass automated checks.

## vs P100 Gemma-4-26B-MoE (thinker winner, ~88)

| | P100 Gemma MoE | 1070 Gemma E4B |
|--|----------------|----------------|
| Tier mapping | Explicit `review→archive`, path heuristics | Generic “rules exist” — weaker |
| Schema | `scripts[]` array + 185 count | Path-keyed map + `verdict`/`reason` — valid but different |
| NFS ops | Race/lock contention | Safety-focused, less concurrency detail |
| Speed | (not measured this run) | Fast on 1070 full GPU |

## Verdict

**Gemma E4B on the 1070 is a solid thinker for this plan task** — production-viable for Phase 2 handoff if you normalize JSON schema in the prompt. Not quite the P100 MoE winner on specificity, but **much cheaper VRAM** (~3.7 GB vs 16 GB) and **no fit/spill**. Good fit for “thinker on 1070” in your fleet split while RTX holds MoE/coder.

**Output:** `manual_1070-GemmaE4B.md`
