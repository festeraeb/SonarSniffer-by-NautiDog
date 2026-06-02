

<!-- reasoning -->
Here's a thinking process:

1.  **Analyze User Input:**
   - **Shard:** `shard_c_rtx_ops (ops_planner)`
   - **Model Output:** A reasoning block analyzing user input for a satellite pipeline task. It mentions:
     - Context: Handoff for `cesarops-satellite` (`sat-run`) on local Sentinel-2 tiles
     - Mission: Run `sat-run` with local offline data, parallelized with rayon, on P100 GPUs (optional later)
     - Focus: STEPS 3-4 + ACCEPTANCE
     - Key Targets: Cedarville (calibration), Robert Burns (masked), distance threshold ~300m
     - Data Paths: `data` (cut off)
   - **Task:** Grade against handoff requirements. Output specific sections. Be strict but fair. Under 350 words.

2.  **Identify Handoff Requirements (Implicit from context):**
   - The prompt implies a handoff to an `ops_planner` shard.
   - Requirements typically include: clear task breakdown, data paths, execution parameters, acceptance criteria, parallelization/GPU specs, and next steps.
   - The model output cuts off at "Data Paths: `data`". This is a critical failure for an ops planner. It lacks concrete paths, command structure, step definitions, and acceptance metrics.

3.  **Evaluate Model Output:**
   - **Strengths:** Correctly identifies context, targets, coordinates, threshold, and technical constraints (rayon, P100, steps 3-4). Shows good comprehension of the mission scope.
   - **Gaps:** 
     - Cuts off mid-sentence at "Data Paths: `data`". Incomplete.
     - No concrete execution plan, commands, or file paths.
     - Missing acceptance criteria details beyond the threshold.
     - No clear step-by-step breakdown for STEPS 3-4.
     - Fails to act as an `ops_planner` (should output a plan, not just analysis).
   - **Verdict:** FAIL (due to incompleteness and lack of actionable planning output)
   - **Score:** 3/10 (Good comprehension, but functionally useless as a planner due to cutoff and lack of structure)

4.  **Draft Response (mental refinement, checking constraints):**
   ## VERDICT: FAIL
   ## Score (0-10)
   3/10
   ## Strengths
   - Accurately extracts mission context, target coordinates, and technical constraints (rayon, P100, steps 3-4).
   - Correctly identifies calibration vs. masked detection roles and the 300m threshold.
   - Maintains clear focus on the satellite pipeline scope.
   ## Gaps / must-fix
   - **Critical Incompleteness:** Output truncates at `data`. An ops planner must deliver a complete, executable plan.
   - **Missing Concrete Paths:** No explicit file directories, tile IDs, or `sat-run` CLI arguments.
   - **No Step Breakdown:** Fails to detail STEPS 3-4 (e.g., preprocessing, inference, post-processing, validation).
   - **Absent Acceptance Criteria:** Threshold is noted but not operationalized (e.g., validation scripts, metric thresholds, pass/fail logic).
   - **Role Misalignment:** Reads like an analyst summary rather than an ops planner output. Should structure commands, resource allocation, and execution flow.
   ## Merge notes (what the final plan must include
   - Explicit data paths (local tiles, cache, output dirs) and `sat-run` invocation syntax.
   - Step-by-step execution flow for 3-4: data ingestion, parallel rayon dispatch, GPU inference, post-processing, and validation.
   - Concrete acceptance criteria: how the 300m threshold is enforced, calibration validation steps, and pass/fail gates.
   - Resource specs: rayon thread count, P100 memory limits, fallback CPU behavior.
   - Error handling & retry logic for tile failures.
   - Ensure output is complete, structured, and directly actionable for execution.

   Word count check: ~148 words. Well under 350. Matches required sections exactly. Strict but fair. Covers all points. Ready.✅
   Output matches response.✅
   Proceed. 
   Self-Correction/Refinement during thought: 
   Check if "PARTIAL" might be more appropriate? The prompt says "Be strict but fair." A cutoff at a critical field is a hard fail for an ops planner. I'll stick with FAIL. Score 3 is fair. Structure matches exactly. Word count is fine. All good. 
   Output matches