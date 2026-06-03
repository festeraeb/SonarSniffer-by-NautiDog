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
- **Threshold logic:**
  - Replace static threshold with adaptive threshold based on LOO statistics.
  - Example: `threshold = μ_{-i} + k * σ_{-i}` where `k` is a configurable sensitivity parameter.
  - Ensure threshold is dynamic and responsive to data variance.

---

#### 📝 `src/mission.rs` Modifications
- **Update threshold configuration:**
  - Introduce or update threshold parameters to support adaptive thresholding.
  - Ensure backward compatibility or introduce version flags for new scoring.
- **Adjust persistence map aggregation/output formatting:**
  - Modify map aggregation logic to handle normalized persistence scores.
  - Ensure output format remains consistent with existing interfaces.
- **Validation logic:**
  - Update validation logic to account for new scoring and thresholding methods.
  - Ensure robustness against outliers and baseline drift.

---

#### 🧪 Validation & Testing
- **Verify max persistence map scales appropriately:**
  - Ensure the maximum persistence score exceeds `0.3` after normalization.
- **Check LOO z-score stability:**
  - Validate LOO z-score computation with small windows (`N < 5`).
- **Validate threshold behavior:**
  - Test threshold logic on known positive and negative cases.
- **Add unit tests:**
  - Implement unit tests for LOO computation and threshold edge cases.

---

#### ⚠️ Constraints & Edge Cases
- **Avoid division by zero:**
  - Ensure `σ_{-i}` is never zero by using `max(σ_{-i}, ε)` where `ε = 1e-6`.
- **Maintain computational complexity:**
  - Ensure LOO z-score computation remains efficient with `O(N)` or `O(N log N)` complexity.
- **Document new threshold semantics:**
  - Clearly document the new threshold logic and its parameters for future reference.

---

This specification provides a clear roadmap for implementing leave-one-out z-score normalization and adaptive thresholding in the `run_temporal_stack_local` function, ensuring improved sensitivity and robustness in temporal persistence detection.