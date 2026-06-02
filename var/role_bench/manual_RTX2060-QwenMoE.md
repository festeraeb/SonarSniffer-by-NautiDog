<!-- meta: {"endpoint": "http://10.0.0.201:5200", "finish_reason": "stop", "usage": {"completion_tokens": 2769, "prompt_tokens": 260, "total_tokens": 3029, "prompt_tokens_details": {"cached_tokens": 0}}, "elapsed_s": 677.7, "content_words": 515, "reasoning_words": 900} -->

<!-- final content -->
## Goal
Classify the ~185 shell scripts inventoried in `var/script-inventory/latest/` into `keep`, `archive`, or `ephemeral` tiers. Produce a deterministic `forge-verdict.json` mapping each script path to its verdict, rationale, and confidence score. Phase 2 is strictly read-only classification; no deletions, moves, or renames occur.

## Constraints
- **Environment:** Shared NFS mounted across T440 and cesarops2 nodes. All reads must be NFS-safe (no heavy polling, respect mount latency).
- **Safety:** Zero destructive operations. Must not break or interfere with active fleet, forge-health, or nomad orchestration scripts.
- **Path Discipline:** Inventory reads from `var/script-inventory/latest/manifest.json` and `report.md`. Verdict output written to `forge-verdict.json` at the repo root.
- **Idempotency:** Rerunning the pipeline with unchanged inputs must yield byte-identical `forge-verdict.json`.
- **Analysis Mode:** Static analysis only. No live execution, no runtime state assumptions.

## Architecture sketch (how thinker → coder → reviewer would execute Phase 2)
1. **Thinker (Spec & Routing):** Defines the classification taxonomy, `forge-verdict.json` schema, and dependency-mapping rules. Routes the inventory baseline from `var/script-inventory/latest/manifest.json` to the coder agent.
2. **Coder (Analysis & Verdict Generation):** Parses manifest tiers and cross-references `report.md` for usage frequency. Runs static scans for `source`, `exec`, cron/systemd hooks, and nomad/fleet job references. Applies Thinker’s rules to assign `keep`/`archive`/`ephemeral` verdicts. Writes structured output to `forge-verdict.json`.
3. **Reviewer (Validation & Gate):** Validates JSON schema conformance, verifies that no fleet/forge-health/nomad scripts are misclassified, checks idempotency via diff, and confirms read-only execution. If rejected, returns a structured diff to the coder for correction. Final artifact is committed.

## Handoff to coders (numbered tasks + acceptance criteria for forge-verdict.json)
1. **Parse Inventory Baseline:** Read `var/script-inventory/latest/manifest.json` and `report.md`. Extract absolute paths, current tier, and last-run timestamps.
2. **Static Dependency & Usage Analysis:** For each script, scan for `source`, `exec`, cron/systemd references, and nomad/fleet job bindings. Classify as `keep` if actively referenced or part of fleet/forge-health/nomad pipelines; `archive` if legacy but still in use; `ephemeral` if orphaned or one-off.
3. **Generate `forge-verdict.json`:** Write a JSON array mapping each script path to `{ "path": "...", "verdict": "keep|archive|ephemeral", "rationale": "...", "confidence": 0.0-1.0 }`. Ensure deterministic ordering and output.
4. **Validation & Idempotency Check:** Run diff against previous run (if exists). Verify no fleet/forge-health/nomad scripts are misclassified. Confirm file is valid JSON under repo root.

**Acceptance Criteria for `forge-verdict.json`:**
- Valid JSON matching the defined schema (path, verdict, rationale, confidence).
- Covers all ~185 scripts from `var/script-inventory/latest/manifest.json`.
- Zero `keep` verdicts for scripts explicitly flagged as fleet/forge-health/nomad critical.
- Idempotent: rerun with same inputs produces byte-identical output.
- No destructive side effects; execution trace confirms read-only NFS access.

## Risks & open questions
- **NFS Latency/Locking:** Concurrent static scans may stall; consider batched reads or local staging before classification.
- **Dynamic Sourcing Blind Spots:** `eval`, `source $VAR`, or runtime-generated includes may evade static analysis. Open question: should we allow manual override flags in the manifest?
- **Merge Conflicts:** If multiple agents write `forge-verdict.json` concurrently, atomic write patterns or file locking are required.
- **Phase 3 Ambiguity:** Archival/ephemeral pruning criteria are undefined. Need explicit retention policies before deletion/moves.
- **Hybrid Scripts:** How do we classify scripts that are fleet-critical but functionally ephemeral (e.g., health checks, one-off probes)?

<!-- reasoning_content -->
Here's a thinking process:

1.  **Analyze User Input:**
   - **Role:** THINKER in a multi-agent pipeline
   - **Task:** MIGRATION_STATUS + SCRIPT_CLEANUP_FORGE_MISSION
   - **Context:** ~185 shell scripts on shared NFS. Phase 1 inventory done at `var/script-inventory/latest/` (manifest.json, report.md, keep/review/ephemeral tiers). Phase 2 needs Forge to classify each script into keep/archive/ephemeral and write `forge-verdict.json` — no deletes yet.
   - **Required Sections:**
     - ## Goal
     - ## Constraints (NFS shared T440+cesarops2, do not break fleet/forge-health/nomad scripts)
     - ## Architecture sketch (how thinker → coder → reviewer would execute Phase 2)
     - ## Handoff to coders (numbered tasks + acceptance criteria for forge-verdict.json)
     - ## Risks & open questions
   - **Constraints:** Under 700 words. Be specific to repo paths. Do not write full code. Output complete sections in main reply.

2.  **Deconstruct Requirements & Brainstorming:**
   - *Goal:* Clear, concise statement of Phase 2 objective. Classify ~185 scripts, output `forge-verdict.json`, preserve existing functionality, prepare for archival/ephemeral handling.
   - *Constraints:* NFS shared across T440+cesarops2 nodes. Must not break fleet/forge-health/nomad scripts. Read-only operations for classification. Idempotent. Path-specific: `var/script-inventory/latest/`, `forge-verdict.json`.
   - *Architecture Sketch:* Thinker (me) defines spec → Coder implements classification logic (static analysis, dependency mapping, usage tracking via logs/manifest) → Reviewer validates against constraints & outputs `forge-verdict.json`. Pipeline: manifest.json → analysis → verdict → validation.
   - *Handoff to Coders:* Numbered tasks. Each with acceptance criteria. Focus on `forge-verdict.json` schema, tier classification, path references, idempotency, no destructive ops.
   - *Risks & Open Questions:* NFS latency/locking, false positives in static analysis, dynamic execution context missing, merge conflicts with concurrent fleet updates, schema validation, archival process definition.

3.  **Draft - Section by Section (Mental Refinement & Word Count Check):**
   *Goal:* Classify the ~185 shell scripts inventoried in `var/script-inventory/latest/` into `keep`, `archive`, or `ephemeral` tiers. Produce a deterministic `forge-verdict.json` mapping each script path to its verdict, metadata, and rationale. No deletions or moves; Phase 2 is read-only classification to enable Phase 3 archival/ephemeral pruning.

   *Constraints:* 
   - Target environment: Shared NFS mounted across T440 and cesarops2 nodes.
   - Strictly read-only operations; zero destructive writes or renames.
   - Must not interfere with or break active fleet, forge-health, or nomad orchestration scripts.
   - All outputs must reside under `var/script-inventory/latest/` and `forge-verdict.json` at the repo root.
   - Idempotent execution; reruns must yield identical verdicts given unchanged inputs.
   - Classification must rely on static analysis, manifest metadata, and historical usage patterns—no live execution.

   *Architecture Sketch:* 
   1. **Thinker (Spec & Routing):** Defines classification rules, schema for `forge-verdict.json`, and dependency graph mapping. Routes inventory from `var/script-inventory/latest/manifest.json` to the coder agent.
   2. **Coder (Analysis & Verdict Generation):** Parses manifest tiers. Runs static analysis (shebang, source imports, cron/systemd hooks, nomad/fleet job references). Cross-references with `var/script-inventory/latest/report.md` for usage frequency. Outputs structured verdicts to `forge-verdict.json`.
   3. **Reviewer (Validation & Gate):** Validates schema conformance, checks for fleet/forge-health/nomad script protection rules, verifies idempotency, and signs off. If rejected, returns diff to coder for correction. Final artifact is committed to `forge-verdict.json`.

   *Handoff to Coders:*
   1. **Parse Inventory & Baseline:** Read `var/script-inventory/latest/manifest.json` and `report.md`. Extract absolute paths, current tier, and last-run timestamps.
   2. **Static Dependency & Usage Analysis:** For each script, scan for `source`, `exec`, `cron`, `systemd`, `nomad`, and `fleet` references. Classify as `keep` if actively referenced or part of fleet/forge-health/nomad pipelines; `archive` if legacy but still in use; `ephemeral` if orphaned or one-off.
   3. **Generate `forge-verdict.json`:** Write a JSON array/object mapping each script path to `{ "path": "...", "verdict": "keep|archive|ephemeral", "rationale": "...", "confidence": 0.0-1.0 }`. Ensure deterministic output.
   4. **Validation & Idempotency Check:** Run diff against previous run (if exists). Verify no fleet/forge-health/nomad scripts are misclassified. Confirm file is valid JSON under repo root.
   *Acceptance Criteria for `forge-verdict.json`:*
   - Valid JSON, schema matches spec (path, verdict, rationale, confidence).
   - Covers all ~185 scripts from `var/script-inventory/latest/manifest.json`.
   - Zero `keep` verdicts for scripts flagged as fleet/forge-health/nomad critical.
   - Idempotent: rerun with same inputs produces byte-identical output.
   - No destructive side effects; read-only execution confirmed.

   *Risks & Open Questions:*
   - NFS latency/locking may stall static scans; consider batched reads or local staging.
   - Static analysis may miss dynamic sourcing (`eval`, `source $VAR`); open question: should we allow manual override flags in the manifest?
   - `forge-verdict.json` merge conflicts if multiple agents write concurrently; need file locking or atomic write pattern.
   - Archival/ephemeral pruning criteria undefined; Phase 3 needs explicit retention policies.
   - How do we handle scripts that are both fleet-critical and ephemeral in nature (e.g., health checks)?

4.  **Word Count & Constraint Check:**
   - Goal: ~40
   - Constraints: ~70
   - Architecture: ~90
   - Handoff: ~180
   - Risks: ~90
   - Total: ~470 words. Well under 700.
   - Sections match exactly.
   - Specific paths included: `var/script-inventory/latest/`, `manifest.json`, `report.md`, `forge-verdict.json`, T440+cesarops2, fleet/forge-health/nomad.
   - No code. Structured outline format.
   - Ready. Output matches draft. I'll refine slightly for precision and flow. Ensure tone matches "THINKER" role. All constraints met. Proceed. 
   Self-Correction/Verification during drafting:
   - Check path specificity: `var/script-inventory/latest/` is used. `forge-verdict.json` at repo root is specified.
   - Check constraints: NFS shared T440+cesarops2, do not break fleet/forge-health/nomad scripts. Explicitly stated.
   - Architecture: thinker → coder → reviewer pipeline clearly mapped.
   - Handoff: numbered tasks + acceptance criteria for forge-verdict.json.
   - Risks & open questions: included.
   - Word count: ~460. Perfect.
   - Output matches exactly. Proceeds. 
   Generate response. 
   [Output Generation] -> *Proceeds*
   *(Self-Correction/Refinement during thought)*: I'll make sure the formatting strictly matches the requested headings. I'll tighten the language to be more pipeline-focused. I'll ensure the acceptance criteria are explicitly tied to `forge-verdict.json`. All good.✅
   Output matches the final refined version. 
   [Output Generation] -> *Proceeds*
