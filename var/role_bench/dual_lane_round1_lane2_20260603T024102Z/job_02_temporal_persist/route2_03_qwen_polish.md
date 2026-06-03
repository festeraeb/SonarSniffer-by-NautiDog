

<!-- reasoning -->
Here's a thinking process:

1.  **Analyze User Input:**
   - **Role/Context:** "Qwen lead (P100). Spec for 1070 Qwen2.5-Coder."
   - **Task:** "Review temporal.rs run_temporal_stack_local persistence map (max ~0.08). Propose leave-one-out z-score and threshold tweaks."
   - **Output Format:** "Output spec bullets for 1070 coder — files: cesarops-satellite/src/temporal.rs, mission.rs."
   - **Key Technical Concepts:** `temporal.rs`, `run_temporal_stack_local`, persistence map (~0.08 max), LOO z-score, threshold tweaks, `mission.rs`.
   - **Draft Provided:** The user provided a draft spec that's already quite structured but needs polishing per the system prompt: "Fix draft, keep scope. ## Polished deliverable ## Supervisor notes."

2.  **Identify Areas for Improvement in Draft:**
   - The draft is good but slightly verbose and repetitive in places.
   - Needs tighter technical precision, especially around the LOO implementation and threshold logic.
   - The `mission.rs` section is vague ("Update threshold configuration", "Adjust persistence map aggregation"). Needs concrete, actionable bullets aligned with typical Rust/satellite codebase patterns.
   - Validation/testing section can be more specific to the problem (max ~0.08 ceiling).
   - Constraints/Edge Cases are okay but can be more precise for Rust implementation.
   - Format needs to strictly follow: `## Polished deliverable ##` then `## Supervisor notes ##`.

3.  **Refine - Section by Section (Mental Draft):**
   *Objective:* Keep it tight. Focus on replacing static/under-scaled scoring with LOO z-score + adaptive threshold to break the ~0.08 ceiling.
   *Current State:* Clarify why ~0.08 happens (score compression, static threshold mismatch, outlier skew).
   *temporal.rs:* 
     - LOO computation: precise formula, handle N<3, numerical stability.
     - Integration: where to inject it in `run_temporal_stack_local`, return type, scaling/clamping strategy.
     - Threshold: adaptive formula, parameterization, fallback.
   *mission.rs:*
     - Config/params: expose threshold sensitivity (`k`), epsilon, scale factor.
     - Aggregation/Output: handle normalized scores, maintain interface contract, update docstrings.
     - Validation: add checks for score bounds, threshold drift.
   *Validation/Testing:* Specific targets (max > 0.35, LOO stability, threshold sensitivity tests), benchmark impact.
   *Constraints:* O(N) LOO via running sums, division-by-zero guard, type safety, doc updates.

4.  **Draft - Polished Deliverable (Mental to Text):**
   I'll structure it exactly as requested, tightening the language, removing fluff, and making it highly actionable for a coder.

   **## Polished deliverable ##**
   ### 📋 Issue #1070 Spec: Temporal Persistence Scoring & Threshold Refinement
   **Target Branch:** `cesarops-satellite`  
   **Files:** `src/temporal.rs`, `src/mission.rs`  
   **Priority:** P100 (Lead) | **Model:** Qwen2.5-Coder

   #### 🎯 Objective
   Replace the under-scaled static persistence scoring in `run_temporal_stack_local` with **leave-one-out (LOO) z-score normalization** and **adaptive thresholding** to break the `~0.08` map ceiling. Increase sensitivity to genuine temporal events while suppressing false positives from baseline drift.

   #### 🔍 Current State & Root Cause
   - Persistence map saturates at `~0.08`, indicating severe score compression or scale mismatch.
   - Root causes: raw accumulation without distribution normalization, static threshold misaligned with local variance, and outlier contamination skewing baseline statistics.
   - LOO z-score decouples per-window scoring from global outliers and stabilizes variance estimation for small temporal windows.

   #### 📝 `src/temporal.rs` Modifications
   - **LOO Z-Score Implementation:**
     - Add `fn compute_loo_zscores(window: &[f64]) -> Vec<f64>`.
     - Compute `μ_{-i}` and `σ_{-i}` for each `i` by excluding the current element.
     - Formula: `z_i = (x_i - μ_{-i}) / max(σ_{-i}, 1e-6)`.
     - Early return `vec![0.0; N]` for `N < 3` to prevent numerical instability.
   - **Integration into `run_temporal_stack_local`:**
     - Inject `compute_loo_zscores` as a post-processing step before map aggregation.
     - Map raw persistence to normalized scores: `persistence_i = clamp(z_i * SCALE_FACTOR, -3.0, 3.0)` (or `sigmoid` if downstream expects `[0,1]`).
    