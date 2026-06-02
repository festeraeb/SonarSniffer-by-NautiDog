

<!-- reasoning -->
Here's a thinking process:

1.  **Analyze User Input:**
   - **Role:** THINKER in a multi-agent pipeline. Produce clear handoffs. Do not write full code unless a 5-line sketch is essential.
   - **Mission A:** Create a spec for a NEW watchdog to replace `mission_service_watchdog.sh`. Requirements:
     - Read last GPU slot heartbeat snapshot (per `gpu_uuid` + `port` + `model_path`)
     - Bring up the LAST KNOWN model on each card (dynamic), not fixed triple-stack preset
     - Unload/stop the port before reload (same port)
     - Output format: `## Dynamic watchdog request for Forge`
   - **Mission B:** Dispatch to three coders (different slices, same repo `wreckhunter2000-1` under `/data/codebase/repos` or `/mnt/t440/codebase/repos`):
     - **P100-Gemma-MoE**: UX/script inventory angle
     - **P100-Qwen36**: data/schema/reviewer angle
     - **1070-Qwen25-Coder7B**: implementation sketch for cesarops-detection
     - Output format: Exactly the specified section headers with task + acceptance criteria for each.
   - **Constraints:** Under 900 words. No full implementations — handoff only. Clear handoffs.

2.  **Deconstruct Mission A (Spec):**
   - Need to specify a shell script (or similar) that replaces `mission_service_watchdog.sh`.
   - Core logic:
     - Read heartbeat snapshot file (JSON/CSV/structured format) containing `gpu_uuid`, `port`, `model_path`, `timestamp`, `status`.
     - Group by `gpu_uuid`.
     - For each GPU, find the latest heartbeat entry (by timestamp).
     - Extract `model_path` and `port`.
     - Check if service is running on that port. If not, stop/unload any lingering process on that port.
     - Launch the extracted `model_path` on that port.
     - Log actions.
     - Handle errors gracefully.
   - Format: `## Dynamic watchdog request for Forge` followed by spec details.

3.  **Deconstruct Mission B (Dispatch):**
   - Repo: `wreckhunter2000-1`
   - Path: `/data/codebase/repos/wreckhunter2000-1` (or `/mnt/t440/codebase/repos/wreckhunter2000-1`)
   - **P100-Gemma-MoE**: UX/script inventory angle. Task: Inventory existing monitoring/launch scripts, map dependencies, create a unified CLI/UX wrapper spec. Acceptance criteria: Clear inventory table, dependency graph, UX spec draft.
   - **P100-Qwen36**: data/schema/reviewer angle. Task: Define the heartbeat snapshot schema, validation rules, and a reviewer script to parse/verify snapshots before watchdog consumption. Acceptance criteria: Schema definition, validation script outline, reviewer acceptance test.
   - **1070-Qwen25-Coder7B**: implementation sketch for cesarops-detection. Task: Provide a 5-line sketch/core logic for detecting cesarops (custom ops) in the loaded models, integrating with the watchdog's reload cycle. Acceptance criteria: Sketch provided, integration points defined, detection logic outline.

4.  **Draft - Section by Section (Mental Refinement):**
   *Check constraints:* Under 900 words total. No full code. Clear handoffs. Exact headers.

   **## Dynamic watchdog request for Forge**
   - Objective: Replace `mission_service_watchdog.sh` with a dynamic, state-aware reload engine.
   - Input: Heartbeat snapshot file (e.g., `/var/run/gpu_heartbeats.jsonl` or `.csv`) containing records: `{gpu_uuid, port, model_path, timestamp, status}`.
   - Logic Flow:
     1. Parse snapshot; group records by `gpu_uuid`.
     2. For each group, select the record with the maximum `timestamp`.
     3. Extract target `model_path` and `port`.
     4. Port Sanitization: Check for stale processes on the target port. If found, send SIGTERM/SIGKILL and wait for port release.
     5. Dynamic Launch: Execute the extracted `model_path` bound to the target port. Do not assume a fixed model triple.
     6. State Sync: Write a new heartbeat entry confirming the reload.
   - Constraints: Idempotent, handles concurrent GPU slots, logs all transitions, exits 0 on success, non-zero on port/model failure.
   - Handoff: Pass to Forge for implementation.

   **## Worker: P100-Gemma-MoE**
   - Task: Conduct a UX/script inventory audit of `wreckhunter2000-1`. Map all existing launch, monitor, and teardown scripts. Identify overlapping functionality and define a unified CLI/UX interface spec that abstracts the new dynamic watchdog.
   - Acceptance Criteria:
     - Deliverable: `inventory/UX_SPEC.md` with a complete script map, dependency tree, and proposed CLI commands.
     - Must identify at least 3 legacy scripts to deprecate or wrap.
     - UX spec must explicitly route user commands to the dynamic watchdog’s port/model resolution logic.
     - Handoff: Pass spec to P100-Qwen36 for schema alignment.

   **## Worker: P100-Qwen36**
   - Task: Define the heartbeat snapshot schema and build a lightweight reviewer/validation layer. Create a schema definition (JSON Schema or YAML) and a reviewer script that ingests raw snapshots, validates field types/timestamps, and flags malformed entries before the watchdog consumes them.
   - Acceptance Criteria:
     - Deliverable: `schema/heartbeat_schema.json` + `reviewer/validate_snapshot.sh` (outline only).
     - Schema must enforce `gpu_uuid` uniqueness per slot, ISO8601 timestamps, and valid `model_path` prefixes.
     - Reviewer must output a pass/fail exit code and a structured error log.
     - Handoff: Pass schema/reviewer to 1070-Qwen25-Coder7B for cesarops-detection integration.

   **## Worker: 1070-Qwen25-Coder7B**
   - Task: Draft a 5-line implementation sketch for `cesarops-detection` integration within the watchdog’s reload cycle. Focus on detecting custom ops (cesarops) in the dynamically loaded `model_path` before binding to the port, ensuring compatibility checks happen pre-launch.
   - Acceptance Criteria:
     - Deliverable: `ops/cesarops_sketch.py` (max 5 lines of core logic) + integration notes.
     - Sketch must show how to intercept `model_path`, run a lightweight cesarops signature scan, and return a boolean flag to the watchdog’s launch sequence.
     - Must define the exact hook point (pre-port-bind) and error fallback behavior.
     - Handoff: Finalize pipeline spec for Forge.

5.  **Word Count & Constraint Check:**
   - Headers match exactly.
   - Mission A is a spec, not code.
   - Mission B has exactly three workers with task + acceptance criteria.
   - Repo mentioned in context.
   - Under 900 words? Let's count roughly.
     - Mission A: ~150
     - Worker 1: ~130
     - Worker 2: ~130
     - Worker 3: ~130
     - Total: ~540 words. Well under 900.
   - No full implementations? Checked. Only outlines/specs/sketches.
   - Clear handoffs? Each worker ends with a handoff instruction.
   - Format matches request exactly.

   One minor adjustment: Ensure the repo path is explicitly referenced in the dispatch as requested. I'll add it to the worker tasks naturally.
   Check exact header format:
   ## Dynamic watchdog request for Forge
   ## Worker: P100-Gemma-MoE
   ## Worker: P100-Qwen36
   ## Worker: 1070-Qwen25-Coder7B
   Matches.

   All constraints met. Proceed. 
   Self-Correction/Refinement during thought: 
   The prompt says "Repo: wreckhunter2000-1 under /data/codebase/repos or /mnt/t440/codebase/repos." I should explicitly mention this path in the dispatch to ground the tasks. I'll add it to the worker descriptions concisely.
   Also, ensure the watchdog spec explicitly mentions the requirements: read last snapshot per gpu_uuid+port+model_path, bring up last known model dynamically, unload/stop port before reload.
   I'll tighten the spec to match exactly.

   Draft looks solid. Output matches response.✅
   Proceeds. 
   Output generation. 
   [Output Generation] -> *Proceeds*
   *(Self-Correction/Verification during drafting)*
   - Check: "Read the last GPU slot heartbeat snapshot (per gpu_uuid + port + model_path)" -> I'll make sure the spec explicitly states grouping by this composite key.
   - Check: "Bring up the LAST KNOWN model