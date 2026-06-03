### Context & Rationale
Current `run_temporal_stack_local` persistence map caps at ~0.08, indicating either over-damped windowing, misaligned normalization, or static thresholds that suppress transient but mission-relevant events. A leave-one-out (LOO) z-score baseline reduces outlier self-influence in small/rolling windows and stabilizes thresholding. Thresholds should shift from fixed persistence values to z-score-adaptive triggers aligned with mission risk profiles.

---

### `cesarops-satellite/src/temporal.rs` — Spec Bullets
- **LOO Z-Score Implementation**
  - Add `fn leave_one_out_zscore(window: &[f64]) -> Vec<f64>`
  - Compute $\mu_{-i} = \frac{1}{N-1}\sum_{j\neq i}x_j$, $\sigma_{-i} = \sqrt{\frac{1}{N-2}\sum_{j\neq i}(x_j-\mu_{-i})^2}$
  - Clamp $\sigma_{-i} \ge \epsilon$ (default `1e-6`) to prevent division-by-zero
  - Return aligned `Vec<f64>` matching input length
- **Persistence Map Integration**
  - Apply LOO z-score to raw telemetry window *before* persistence accumulation
  - Replace static persistence scaling with `persistence_z = z_score * persistence_decay`
  - Expose `max_persistence_z` metric; expect post-tweak cap ~0.85–1.15 (configurable)
- **Threshold Configuration**
  - Replace `PERSISTENCE_THRESHOLD: f64 = 0.08` with `Z_THRESHOLD: f64` (default `2.0`)
  - Add `PERSISTENCE_SCALE: f64` (default `0.15`) for mission-tuned sensitivity
  - Dynamic trigger: `trigger = persistence_z > (Z_THRESHOLD * PERSISTENCE_SCALE)`
- **Window & State Management**
  - Enforce `window_len >= 5` for valid LOO statistics; log warning & fallback to static mean if `< 5`
  - Thread-safe state: `AtomicUsize` for window pointer, `Mutex<f64>` for running stats cache
  - Add `#[cfg(test)]` module with `test_loo_zscore_symmetry()` and `test_loo_edge_cases()`

---

### `mission.rs` — Spec Bullets
- **Threshold Consumer Update**
  - Replace `if persistence > 0.08` with `if persistence_z > Z_THRESHOLD * PERSISTENCE_SCALE`
  - Add `min_duration_secs: u64` guard to suppress micro-fluctuations
  - Signature: `fn evaluate_temporal_trigger(persistence_z: f64, duration: Duration) -> bool`
- **Mission-State Logging & Telemetry**
  - Emit `mission::temporal::threshold_crossed` event with `z_score`, `persistence_z`, `duration`, `triggered`
  - Log `mission::temporal::zscore_baseline` at startup with `Z_THRESHOLD`, `PERSISTENCE_SCALE`, `window_len`
  - Add `mission::temporal::false_positive_guard` counter; reset on confirmed event
- **Config & Sync**
  - Pull `Z_THRESHOLD` and `PERSISTENCE_SCALE` from `mission_config.toml` or env vars (`MISSION_Z_THRESH`, `MISSION_PERSIST_SCALE`)
  - Validate config at init: `Z_THRESHOLD ∈ [1.0, 4.0]`, `PERSISTENCE_SCALE ∈ [0.05, 0.5]`
  - Expose `mission::temporal::set_thresholds(z: f64, scale: f64)` for runtime calibration (guarded by `mission::state::is_nominal()`)

---

### Validation & Testing Bullets
- **Unit**
  - `temporal.rs`: LOO z-score matches analytical baseline on synthetic windows (error `< 1e-9`)
  - `mission.rs`: `evaluate_temporal_trigger` returns `true` only when `persistence_z > threshold` AND `duration >= min_duration`
- **Integration**
  - Inject 100-step telemetry with known outlier at index 50; verify `persistence_z` spike ≥ `Z_THRESHOLD * PERSISTENCE_SCALE`
  - Confirm no trigger fires on noise-only stream (false positive rate `< 0.01` over 10k steps)
- **Performance**
  - LOO computation ≤ `O(N)` per window; memory ≤ `2 * window_len * 8` bytes
  - Mission eval latency ≤ `50μs` on target hardware (profile with `criterion`)

---

### Implementation Notes for 1070 Coder
- Keep LOO math in `temporal.rs`; do not leak statistical internals to `mission.rs`
- Use `f64` consistently; avoid `f32` drift in threshold comparisons
- Add `#[must_use]` and `#[inline]` to hot paths; document numerical bounds in doc comments
- PR template: include `z_score_distribution.png`, `threshold_sweep.csv`, and `mission_trigger_log.txt`

<!-- reasoning -->
Here's a thinking process:

1.  **Analyze User Input:**
   - **Task:** Review `temporal.rs` run_temporal_stack_local persistence map (max ~0.08). Propose leave-one-out z-score and threshold tweaks. Output spec bullets for 1070 coder — files: `cesarops-satellite/src/temporal.rs`, `mission.rs`.
   - **Key Components:**
     - File: `temporal.rs` (specifically `run_temporal_stack_local`)
     - Metric: persistence map (max ~0.08)
     - Proposed changes: leave-one-out z-score, threshold tweaks
     - Output format: spec bullets for a "1070 coder" (likely refers to a specific coding standard, team, or internal tool, but I'll treat it as a technical specification format)
     - Target files: `cesarops-satellite/src/temporal.rs`, `mission.rs`
   - **Context/Domain:** Satellite operations, temporal data processing, anomaly detection/persistence mapping, statistical thresholds (z-score, leave-one-out), mission planning/execution.

2.  **Deconstruct Requirements:**
   - **Review:** Analyze the current state of `run_temporal_stack_local` and its persistence map (max ~0.08). What does a max persistence of 0.08 imply? Likely low persistence, meaning events are transient or the threshold/window is too strict.
   - **Propose:** Leave-one-out z-score calculation and threshold adjustments.
   - **Output:** Spec bullets targeting two files: `temporal.rs` and `mission.rs`.
   - **Audience:** "1070 coder" - I'll assume this means a developer following a specific internal spec format (likely concise, actionable, file-specific, with clear before/after or implementation steps).

3.  **Identify Key Technical Concepts:**
   - **Persistence Map:** Likely tracks how long/often a condition persists over time windows. Max ~0.08 suggests normalization or probability scale [0,1].
   - **Leave-One-Out Z-Score:** Statistical method for anomaly detection where each point's z-score is calculated excluding itself. Useful for small datasets or avoiding self-influence.
   - **Threshold Tweaks:** Adjusting detection thresholds to balance sensitivity/specificity, likely tied to the persistence map and z-scores.
   - **Files:**
     - `cesarops-satellite/src/temporal.rs`: Core temporal processing, stack execution, persistence calculation.
     - `mission.rs`: Mission planning/execution, likely consumes temporal outputs, applies thresholds, triggers actions.

4.  **Formulate Technical Analysis & Proposals:**
   - *Current State:* `run_temporal_stack_local` computes a persistence map with max ~0.08. This indicates either:
     - The window/decay function is too aggressive
     - Thresholds are too high
     - Data normalization/scale is off
     - Z-score baseline is unstable
   - *Leave-One-Out Z-Score:* 
     - Formula: $z_i = \frac{x_i - \mu_{-i}}{\sigma_{-i}}$ where $\mu_{-i}, \sigma_{-i}$ are mean/std excluding $x_i$
     - Benefits: Reduces bias from outliers, better for streaming/small windows
     - Implementation: Need sliding window, compute stats excluding current, apply to persistence map
   - *Threshold Tweaks:*
     - Current max persistence 0.08 → likely needs dynamic thresholding or scaling
     - Propose: Adaptive threshold based on z-score distribution, or fixed threshold on transformed persistence (e.g., `threshold = base * (1 + z_score * scale)`)
     - Consider mission context: `mission.rs` needs to interpret these values for action triggers

5.  **Draft Spec Bullets (Mental Refinement):**
   *Structure per file:*
   - Objective
   - Changes/Implementation
   - Parameters/Constants
   - Validation/Testing
   - Dependencies/Notes

   *For `temporal.rs`:*
   - Replace static persistence scaling with leave-one-out z-score normalization
   - Implement `leave_one_out_zscore(window: &[f64]) -> Vec<f64>`
   - Adjust persistence map threshold from fixed `0.08` to dynamic `z_threshold * scale_factor`
   - Add window size/config parameter for LOO calculation
   - Ensure numerical stability (handle σ < ε)

   *For `mission.rs`:*
   - Update threshold consumer to accept z-scored persistence values
   - Implement adaptive trigger: `if persistence_z > z_thresh && duration > min_dur { trigger }`
   - Add mission-state logging for threshold crossings
   - Sync with temporal.rs config via shared constants or trait

6.  **Check Against Requirements:**
   - Reviews persistence map max ~0.08? Yes, addresses scaling/thresholding
   - Proposes LOO z-score? Yes, with formula/implementation notes
   - Proposes threshold tweaks? Yes, dynamic/adaptive
   - Output spec bullets? Yes
   - Targets `temporal.rs` and `mission.rs`? Yes
   - Format appropriate for "1070 coder"? I'll use concise, actionable, file-scoped spec bullets with clear implementation directives.

7.  **Final Output Generation:** (Structure carefully)
   - Start with brief context/rationale
   - Provide file-specific spec bullets
   - Include parameters, implementation steps, validation
   - Keep it technical, precise, and ready for coding
   - Use markdown formatting as requested

   *Self-Correction/Refinement during thought:*
   - Need to clarify how LOO z-score integrates with persistence map (is it applied to raw signals or persistence values?)
   - Max 0.08 suggests normalized persistence; LOO z-score should likely be applied to the underlying time-series before or after persistence calculation
   - Threshold tweaks should account for mission constraints (false positives vs missed detections)
   - Add explicit constants/config keys for the "1070 coder" to reference
   - Ensure spec is actionable: function signatures, parameter ranges, test expectations

   I'll structure it as:
   - **Rationale** (brief)
   - **`cesarops-satellite/src/temporal.rs`** spec bullets
   - **`mission.rs`** spec bullets
   - **Validation/Testing** bullets
   - Keep it tight. No fluff. Ready.✅
