

<!-- reasoning -->
Here's a thinking process:

1.  **Analyze User Input:**
   - **Context:** `run_temporal_stack_local` persistence map caps at ~0.08. This indicates over-damping, misaligned normalization, or static thresholds suppressing transient events.
   - **Goal:** Implement leave-one-out (LOO) z-score baseline and adaptive thresholding to stabilize detection and improve sensitivity.
   - **Output Format:** Spec bullets for a "1070 coder" (implies concise, actionable, technical directives).
   - **Target Files:** `cesarops-satellite/src/temporal.rs`, `mission.rs`
   - **Constraints:** Under 500 words. Lane B Qwen14 Coder on P100. (This is just a persona/system prompt, I'll focus on the technical spec).

2.  **Deconstruct Requirements from the Prompt:**
   - **`temporal.rs`:**
     - LOO Z-Score implementation: `fn leave_one_out_zscore(window: &[f64]) -> Vec<f64>`
     - Math: $\mu_{-i}$, $\sigma_{-i}$ with clamping $\sigma \ge \epsilon$
     - Persistence map integration: Apply LOO z-score before accumulation, replace static scaling with `persistence_z = z_score * persistence_decay`
     - Expose `max_persistence_z` metric, target cap ~0.85–1.15
     - Threshold config: Replace `0.08` with `Z_THRESHOLD` (default 2.0) and `PERSISTENCE_SCALE` (default 0.15)
     - Dynamic trigger: `trigger = persistence_z > (Z_THRESHOLD * PERSISTENCE_SCALE)`
     - Window/state management: `window_len >= 5`, fallback to static mean, thread-safe state (`AtomicUsize`, `Mutex<f64>`)
     - Tests: `test_loo_zscore_symmetry()`, `test_loo_edge_cases()`
   - **`mission.rs`:**
     - Threshold consumer: Replace `if persistence > 0.08` with `if persistence_z > Z_THRESHOLD * PERSISTENCE_SCALE`
     - Add `min_duration_secs: u64` guard
     - Signature: `fn evaluate_temporal_trigger(persistence_z: f64, duration: Duration) -> bool`
     - Logging/Telemetry: `mission::temporal::threshold_crossed`, `mission::temporal::zscore_baseline`, `mission::temporal::false_positive_guard`
     - Config/Sync: Pull from `mission_config.toml` or env vars, validate ranges, expose runtime setter guarded by `is_nominal()`
   - **Validation/Testing:**
     - Unit: LOO matches analytical baseline (<1e-9 error), trigger logic correct
     - Integration: 100-step with outlier at 50, false positive rate <0.01 over 10k steps
     - Performance: LOO O(N), memory ≤ 2*N*8 bytes, eval latency ≤ 50μs
   - **Implementation Notes:** Keep LOO math in `temporal.rs`, use `f64`, `#[must_use]`, `#[inline]`, doc comments, PR template assets.

3.  **Synthesize & Structure (Mental Draft):**
   I need to condense this into a tight, actionable spec under 500 words. I'll organize by file, then validation, keeping it direct and technical.

   *Structure:*
   - **Rationale** (1 line)
   - **`cesarops-satellite/src/temporal.rs`** (bullets)
   - **`mission.rs`** (bullets)
   - **Validation & Performance** (bullets)
   - **Implementation Notes** (bullets)

   *Draft Generation (mental refinement for conciseness & precision):*
   **Rationale:** Current `run_temporal_stack_local` caps at ~0.08 due to static thresholds and outlier self-influence. LOO z-score normalization stabilizes baseline estimation and enables adaptive triggering.

   **`cesarops-satellite/src/temporal.rs`**
   - Implement `#[must_use] #[inline] fn leave_one_out_zscore(window: &[f64]) -> Vec<f64>`
   - Compute $\mu_{-i} = \frac{1}{N-1}\sum_{j\neq i}x_j$, $\sigma_{-i} = \sqrt{\frac{1}{N-2}\sum_{j\neq i}(x_j-\mu_{-i})^2}$
   - Clamp $\sigma_{-i} \ge 1\text{e-}6$; return aligned `Vec<f64>`
   - Enforce `window_len >= 5`; log warning & fallback to static mean if violated
   - Apply LOO z-score to raw telemetry *before* persistence accumulation
   - Replace static scaling: `persistence_z = z_score * PERSISTENCE_SCALE`
   - Expose `max_persistence_z` metric; target post-tweak cap: `0.85–1.15`
   - Thread-safe state: `AtomicUsize` for window pointer, `Mutex<f64>` for running stats cache
   - Add `#[cfg(test)]` module: `test_loo_zscore_symmetry()`, `test_loo_edge_cases()`

   **`mission.rs`**
   - Replace `if persistence > 0.08` with `if persistence_z > Z_THRESHOLD * PERSISTENCE_SCALE`
   - Add `min_duration_secs: u64` guard to suppress micro-fluctuations
   - Implement `fn evaluate_temporal_trigger(persistence_z: f64, duration: Duration) -> bool`
   - Emit telemetry: `mission::temporal::threshold_crossed` (z_score, persistence_z, duration, triggered)
   - Log startup baseline: `mission::temporal::zscore_baseline` (Z_THRESHOLD, PERSISTENCE_SCALE, window_len)
   - Track `mission::temporal::false_positive_guard`; reset on confirmed event
   - Config sync: Pull `Z_THRESHOLD`/`PERSISTENCE_SCALE` from `mission_config.toml` or env (`MISSION_Z_THRESH`, `MISSION_PERSIST_SCALE`)
   - Validate at init: `Z_THRESHOLD ∈ [1.0, 4.0]`, `PERSISTENCE_SCALE ∈ [0.05, 0.5]`
   - Expose `mission::temporal::set_thresholds(z: f64, scale: f64)` guarded by `mission::state::is_nominal()`

   **Validation & Performance**