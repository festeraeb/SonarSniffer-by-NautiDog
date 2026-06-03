### 📋 Issue #1070 Spec: Temporal Persistence Scoring & Threshold Refinement
**Target Branch:** `cesarops-satellite`  
**Files:** `src/temporal.rs`, `src/mission.rs`  
**Priority:** P100 (Lead) | **Model:** Qwen2.5-Coder

---

#### 🎯 Objective
Replace static/under-scaled persistence scoring in `run_temporal_stack_local` with a **leave-one-out (LOO) z-score normalization** and **adaptive thresholding** to resolve the `max ~0.08` persistence map ceiling. Improve sensitivity to genuine temporal events while suppressing false positives from baseline drift.

---

#### 🔍 Current State & Root Cause
- Persistence map peaks at `~0.08`, indicating score compression or scale mismatch.
- Likely causes: raw accumulation without distribution normalization, static threshold misaligned with current data variance, or outlier contamination in baseline statistics.
- LOO z-score will decouple per-window scoring from global outliers and stabilize variance estimation for small temporal windows.

---

#### 📝 `src/temporal.rs` Modifications
- **Implement LOO z-score computation:**
  - For each window `W` of size `N`, compute `μ_{-i}` and `σ_{-i}` excluding index `i`.
  - Formula: `z_i = (x_i - μ_{-i}) / max(σ_{-i}, ε)` where `ε = 1e-6`.
  - Add `fn compute_loo_zscores(window: &[f64]) -> Vec<f64>` with early return for `N < 3`.
- **Apply z-score to raw persistence values:**
  - Replace direct accumulation with `persistence_i = sigmoid(z_i * scale_factor)` or `persistence_i = clamp(z_i, -3.0, 3.0)` depending on downstream expectation.
  - Maintain original `run_temporal_stack_local` signature; inject normalization as a post-processing step before map aggregation.
- **Threshold logic

<!-- reasoning -->
Here's a thinking process:

1.  **Analyze User Input:**
   - **Role/Context:** "Qwen lead (P100). Spec for 1070 Qwen2.5-Coder." - This implies I'm acting as a lead developer/architect providing a specification for a coding task (issue/PR #1070) targeting a specific model/version (Qwen2.5-Coder).
   - **Task:** "Review temporal.rs run_temporal_stack_local persistence map (max ~0.08). Propose leave-one-out z-score and threshold tweaks."
   - **Output Format:** "Output spec bullets for 1070 coder — files: cesarops-satellite/src/temporal.rs, mission.rs."
   - **Key Technical Concepts:**
     - `temporal.rs` file
     - `run_temporal_stack_local` function
     - Persistence map (max ~0.08) - likely refers to some metric, probability, or confidence score in a temporal analysis context
     - Leave-one-out z-score - a statistical method for outlier detection or normalization
     - Threshold tweaks - adjusting decision boundaries
     - Files to modify: `cesarops-satellite/src/temporal.rs`, `cesarops-satellite/src/mission.rs` (implied path)

2.  **Deconstruct the Technical Problem:**
   - The user mentions a "persistence map (max ~0.08)". This suggests the current output/score is very low, possibly indicating poor sensitivity, over-regularization, or incorrect scaling.
   - They want to implement a "leave-one-out z-score" method. This is typically used to normalize data points by computing the mean and standard deviation of a dataset excluding the current point. It's robust for small datasets or when outliers shouldn't skew the baseline.
   - They want "threshold tweaks" - adjusting the cutoff for what constitutes a "persistent" or "significant" event.
   - The context seems to be satellite mission data processing (`cesarops-satellite`), likely dealing with temporal patterns, anomaly detection, or event persistence tracking.

3.  **Formulate Specification Bullets (Mental Draft):**
   I need to structure this as a clear, actionable spec for a coder. It should include:
   - Objective
   - Current state analysis
   - Proposed changes (LOO z-score, thresholds)
   - File-specific modifications
   - Testing/Validation requirements
   - Edge cases/considerations

   *Structure:*
   - **Objective:** Improve sensitivity and robustness of temporal persistence detection in `run_temporal_stack_local` by implementing leave-one-out z-score normalization and dynamic thresholding.
   - **Current State:** Persistence map peaks at ~0.08, indicating under-scoring or scale mismatch. Likely causes: raw score accumulation without normalization, static thresholds too high for current data distribution, or outlier contamination in baseline stats.
   - **Proposed Changes:**
     - Implement LOO z-score for each temporal window/stack element
     - Replace/static threshold with adaptive/dynamic threshold based on LOO stats
     - Update persistence map scaling to reflect normalized scores
   - **File: `temporal.rs`**
     - Modify `run_temporal_stack_local` to compute LOO z-scores per window
     - Add helper function for LOO mean/std calculation
     - Apply z-score to raw persistence values
     - Adjust threshold logic (e.g., `threshold = base + k * std` or percentile-based)
   - **File: `mission.rs`**
     - Update threshold configuration/parameters
     - Adjust persistence map aggregation/output formatting if needed
     - Ensure backward compatibility or version flag for new scoring
   - **Testing/Validation:**
     - Verify max persistence map scales appropriately (target >0.3-0.5 depending on use case)
     - Check LOO z-score stability with small windows (<5 points)
     - Validate threshold behavior on known positive/negative cases
     - Add unit tests for LOO computation and threshold edge cases
   - **Constraints/Notes:**
     - Avoid division by zero in LOO std
     - Maintain O(N) or O(N log N) complexity if possible
     - Document new threshold semantics

4.  **Refine & Format as Spec Bullets (Targeting "1070 coder"):**
   I'll structure it exactly as requested: spec bullets, focused on the two files, with clear technical directives. I'll make it concise, actionable, and aligned with satellite/temporal processing conventions.

   *Self-Correction/Verification during drafting:*
   - Is "persistence map (max ~0.08)" a probability, confidence, or raw score? I'll assume it's a normalized persistence metric that's currently too low. LOO z-score will help standardize it.
   - Threshold tweaks should be explicit: static -> adaptive, or parameterized with clear defaults.
   - I should specify exact function signatures or modification points.
   - I'll keep it strictly in spec-bullet format as requested.
   - I'll add validation criteria and implementation notes relevant to temporal/satellite data.

   Let's draft it carefully. 
   Structure:
   - Objective
   - Current State & Root Cause
   - Implementation Spec (temporal.rs)
   - Implementation Spec (mission.rs)
   - Validation & Testing
   - Constraints & Edge Cases
   All as concise bullets.

5.  **Final Output Generation:** (matches the refined version)
   I'll ensure technical precision, clear file-specific instructions, and actionable thresholds/z-score logic. I'll avoid fluff and stick to the spec format. I'll also note that exact threshold values should be data-driven but provide a starting point. I'll mention numerical stability for LOO. All aligned with the prompt.✅
   Output matches the response. 
   Proceeds. 
   *Self-Correction/Note during thought:* 
   The prompt says "persistence map (max ~0.08)". This is likely a confidence/persistence score that's too low. LOO z-score will standardize it. I should explicitly state how to compute it: `z_i = (x_i - μ_{-i}) / σ_{-i}`. Threshold should be adaptive: `T = μ_{-i} + k·σ_{-i}` or percentile-based. I'll make sure the spec bullets are direct and coder-ready. I'll also note that `mission.rs` likely handles threshold config or persistence map consumption. I'll keep it tight.✅
   Output generation. 
   [Done.] 
   *Output Generation* (matches the final response)
   - Objective
   - Current State Analysis
   - `temporal.rs` Modifications
   - `mission.rs` Modifications
   - Validation & Testing
   - Constraints & Edge Cases
   All in bullet format as requested. Technical, precise, ready for implementation.✅
   Proceeds. 
   [Output Generation] -> *See response.* 
   *(Self-Correction/Verification during output gen prep)*: 
   Check prompt: "Output spec bullets for 1070 coder — files: cesarops-satellite/src/temporal.rs, mission.rs."
   I'll format exactly as requested. I'll make sure each bullet is actionable. I'll include the LOO formula, threshold logic, and file-specific changes. I'll keep it concise. All good.✅
   Output matches response. 
   [Done.]
