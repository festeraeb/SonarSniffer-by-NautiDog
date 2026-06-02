<!-- meta: {"endpoint": "http://10.0.0.61:5002", "finish_reason": "stop", "usage": {"completion_tokens": 3244, "prompt_tokens": 260, "total_tokens": 3504, "prompt_tokens_details": {"cached_tokens": 0}}, "elapsed_s": 212.1, "content_words": 482, "reasoning_words": 984} -->

<!-- final content -->
## Goal
Classify the ~185 inventoryed shell scripts into `keep`, `archive`, or `ephemeral` tiers using metadata, dependency heuristics, and `report.md` annotations. Produce a validated `forge-verdict.json` in `var/script-inventory/latest/` without modifying, moving, or deleting any source files. Preserve operational integrity of fleet, forge-health, and nomad scripts.

## Constraints
- **Environment:** Shared NFS mount accessible by T440 and cesarops2. All reads must tolerate NFS latency/caching.
- **Zero Destructive Ops:** No `rm`, `mv`, `chmod`, or symlink changes. Only read/append.
- **Critical Path Protection:** Any script under `fleet/`, `forge-health/`, or `nomad/` defaults to `keep` unless `var/script-inventory/latest/report.md` explicitly marks it deprecated.
- **Idempotency:** Re-runs must overwrite `forge-verdict.json` atomically (write to `.tmp` → `mv`).
- **Path Specificity:** Source of truth is `var/script-inventory/latest/manifest.json`. Output target is `var/script-inventory/latest/forge-verdict.json`.

## Architecture sketch
1. **Thinker (Me):** Define classification schema, rule matrix, confidence thresholds, and validation criteria. Output this blueprint.
2. **Coder:**
   - Parse `var/script-inventory/latest/manifest.json` to extract `script_path`, `size`, `last_modified`, and tier hints.
   - Run static heuristics: grep for `cron`, `mktemp`, `rm -rf`, `systemd`, `nomad`, `fleet`, `forge-health`.
   - Cross-reference with `var/script-inventory/latest/report.md` for explicit deprecation/retirement notes.
   - Apply decision tree: `nomad/fleet/forge-health` → `keep`; explicit `report.md` deprecation → `archive`; ephemeral markers + stale age → `ephemeral`; ambiguity → `keep` with low confidence.
   - Generate `forge-verdict.json` with schema `{script_path: {tier, confidence, rationale, depends_on}}`.
   - Validate JSON schema and write atomically to `var/script-inventory/latest/forge-verdict.json`.
3. **Reviewer:**
   - Diff `forge-verdict.json` against `manifest.json` to ensure 1:1 coverage.
   - Spot-check 10% of `archive`/`ephemeral` verdicts against `report.md` and source heuristics.
   - Verify zero misclassifications for `fleet/forge-health/nomad` paths.
   - Approve or return with schema/rationale fixes.

## Handoff to coders
1. **Parse & Align:** Read `var/script-inventory/latest/manifest.json`. Map every `script_path` to a verdict entry. Skip nothing.
2. **Classify:** Apply tier rules. Default to `keep` for ambiguity. Flag low-confidence items with actionable `rationale`.
3. **Generate Output:** Write `var/script-inventory/latest/forge-verdict.json` matching:
   ```json
   {"version":"1.0","generated_at":"ISO8601","scripts":{"<path>":{"tier":"keep|archive|ephemeral","confidence":0.0-1.0,"rationale":"string","depends_on":["path"]}}}
   ```
4. **Validate & Commit:** Run JSON schema validation. Ensure atomic write. No side effects.

**Acceptance Criteria:**
- `forge-verdict.json` exists in `var/script-inventory/latest/` and passes schema validation.
- 100% of paths in `manifest.json` have a corresponding entry.
- Zero `keep`→`archive`/`ephemeral` flips for any path under `fleet/`, `forge-health/`, or `nomad/`.
- `confidence < 0.7` items have actionable `rationale` strings.
- File is idempotent; re-run produces identical output given unchanged inputs.

## Risks & open questions
- **NFS Consistency:** T440/cesarops2 may cache stale `stat` metadata. Use explicit NFS sync or `O_NOFOLLOW` if `manifest.json` timestamps drift.
- **Dependency Blind Spots:** Shell scripts often `source` or `eval` dynamically. `depends_on` will capture static links only; runtime deps may be missed.
- **Threshold Tuning:** What stale age triggers `ephemeral`? 30d? 90d? Need explicit policy from fleet owners.
- **Missing Metadata:** If `manifest.json` lacks `last_modified` or `usage_tags`, confidence scores may skew. How to handle?
- **Rollback:** If `forge-verdict.json` contains a critical misclassification, revert via Git history; Phase 2 is read-only until Phase 3.
- **Open:** Should `archive` scripts be moved to a staging NFS subdir in Phase 3, or kept in-place with metadata flags?

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
   - *Goal:* Clear, concise statement of Phase 2 objective. Classify ~185 scripts, output `forge-verdict.json`, preserve fleet/forge-health/nomad scripts.
   - *Constraints:* NFS shared between T440 and cesarops2. Must not break existing fleet/forge-health/nomad scripts. Read-only/append-only operations for now. Path specificity: `var/script-inventory/latest/`, `forge-verdict.json`, NFS mount paths.
   - *Architecture Sketch:* Thinker (me) defines schema/rules → Coder implements classification logic (likely Python/Shell) → Reviewer validates output against manifest & constraints. Flow: Read manifest → Analyze scripts (metadata, usage, dependencies) → Apply classification rules → Generate `forge-verdict.json` → Validate schema.
   - *Handoff to Coders:* Numbered tasks. Acceptance criteria for `forge-verdict.json`. Need specific paths, schema expectations, validation steps.
   - *Risks & Open Questions:* NFS latency/consistency, false positives in classification, dependency tracking, schema validation, rollback plan, how to handle scripts with no clear tier.

3.  **Draft - Section by Section (mental refinement, keeping word count in mind):**
   *## Goal*
   Classify the ~185 inventoryed shell scripts into `keep`, `archive`, or `ephemeral` tiers using metadata, dependency graphs, and usage heuristics. Output a validated `forge-verdict.json` in `var/script-inventory/latest/` without modifying or deleting any source files. Preserve operational integrity of fleet, forge-health, and nomad scripts.

   *## Constraints*
   - Target environment: Shared NFS mount (`/mnt/nfs` or equivalent) accessible by T440 and cesarops2.
   - Zero destructive operations: No `rm`, `mv`, or permission changes. Only read/append.
   - Critical path protection: Scripts under `fleet/`, `forge-health/`, and `nomad/` must default to `keep` unless explicitly deprecated in `report.md`.
   - Idempotent execution: Re-runs must overwrite `forge-verdict.json` atomically (write to temp → `mv`).
   - Path specificity: All references must use `var/script-inventory/latest/manifest.json` as source of truth and `var/script-inventory/latest/forge-verdict.json` as output.

   *## Architecture sketch*
   1. **Thinker (Me):** Define classification schema, rule matrix, and validation criteria. Output this blueprint.
   2. **Coder:** 
      - Parse `var/script-inventory/latest/manifest.json` to extract script paths, sizes, last-modified, and tier hints.
      - Run static analysis: grep for `cron`, `tmp`, `mktemp`, `rm -rf`, `systemd`, `nomad`, `fleet`, `forge-health`.
      - Cross-reference with `var/script-inventory/latest/report.md` for explicit deprecation notes.
      - Apply decision tree: `nomad/fleet/forge-health` → `keep`; explicit `report.md` deprecation → `archive`; `tmp`/ephemeral markers + >30d stale → `ephemeral`; else → `keep`.
      - Generate `forge-verdict.json` with schema `{script_path: {tier, confidence, rationale, deps}}`.
      - Validate against JSON schema and write atomically to `var/script-inventory/latest/forge-verdict.json`.
   3. **Reviewer:** 
      - Diff `forge-verdict.json` against `manifest.json` to ensure 1:1 coverage.
      - Spot-check 10% of `archive`/`ephemeral` verdicts against `report.md` and source heuristics.
      - Verify no `fleet/forge-health/nomad` scripts were misclassified.
      - Approve or return with schema/rationale fixes.

   *## Handoff to coders*
   1. **Parse & Align:** Read `var/script-inventory/latest/manifest.json`. Map every `script_path` to a verdict entry. Skip nothing.
   2. **Classify:** Apply tier rules. Default to `keep` for ambiguity. Flag low-confidence items with `rationale`.
   3. **Generate Output:** Write `var/script-inventory/latest/forge-verdict.json` matching this schema:
      ```json
      {
        "version": "1.0",
        "generated_at": "ISO8601",
        "scripts": {
          "<relative_or_abs_path>": {
            "tier": "keep|archive|ephemeral",
            "confidence": 0.0-1.0,
            "rationale": "string",
            "depends_on": ["path1", "path2"]
          }
        }
      }
      ```
   4. **Validate & Commit:** Run JSON schema validation. Ensure atomic write. No side effects.
   **Acceptance Criteria:**
   - `forge-verdict.json` exists in `var/script-inventory/latest/` and passes schema validation.
   - 100% of paths in `manifest.json` have a corresponding entry.
   - Zero `keep`→`archive`/`ephemeral` flips for any path under `fleet/`, `forge-health/`, or `nomad/`.
   - `confidence < 0.7` items have actionable `rationale` strings.
   - File is idempotent; re-run produces identical output given unchanged inputs.

   *## Risks & open questions*
   - **NFS Consistency:** T440/cesarops2 may cache stale metadata. Use `stat` with `O_NOFOLLOW` or explicit NFS sync flags if needed.
   - **Dependency Blind Spots:** Shell scripts often source files dynamically. Will `depends_on` capture runtime vs. static links?
   - **Threshold Tuning:** What stale age triggers `ephemeral`? 30d? 90d? Need explicit policy from fleet owners.
   - **Schema Rigidity:** If `manifest.json` lacks `last_modified` or `usage_tags`, confidence scores may skew. How to handle missing metadata?
   - **Rollback:** If `forge-verdict.json` contains a critical misclassification, how do we revert without touching NFS? (Answer: Git history + `forge-verdict.json` is append-only until Phase 3.)
   - **Open:** Should `archive` scripts be moved to a staging NFS subdir in Phase 3, or kept in-place with metadata flags?

4.  **Word Count & Constraint Check:**
   - Goal: ~40
   - Constraints: ~90
   - Architecture: ~130
   - Handoff: ~180
   - Risks: ~150
   - Total: ~590 words. Well under 700.
   - Sections match exactly.
   - Specific paths used: `var/script-inventory/latest/manifest.json`, `var/script-inventory/latest/report.md`, `var/script-inventory/latest/forge-verdict.json`, `fleet/`, `forge-health/`, `nomad/`, T440, cesarops2.
   - No code blocks except minimal JSON schema (allowed, not "full code").
   - Structure matches request exactly.
   - Ready. Output matches draft.✅
   - Self-Correction/Refinement during thought: 
     - Check constraint about "do not break fleet/forge-health/nomad scripts" - explicitly addressed.
     - Check "NFS shared T440+cesarops2" - addressed in constraints & risks.
     - Check "no deletes yet" - explicitly stated.
     - Check schema format - provided as requested.
     - Word count is tight and precise.
     - All sections present.
     - Output matches exactly. Proceed. 
   - Generate response. 
   - *Self-Correction/Verification during drafting:* 
     - I'll make sure the JSON schema is inline but minimal to save words.
     - I'll ensure the tone is technical and pipeline-ready.
     - All constraints met. Output matches request.✅
   - Proceed. 
   - Output generation. 
   - [Done] 
   - *Output Generation* (matches the refined draft)
   - `## Goal` ... `## Constraints` ... `## Architecture sketch` ... `## Handoff to coders` ... `## Risks & open questions`
   - Word count: ~580. Perfect.✅
   - All constraints satisfied.✅
   - Ready. 
   - *Output Generation* (proceeds)
