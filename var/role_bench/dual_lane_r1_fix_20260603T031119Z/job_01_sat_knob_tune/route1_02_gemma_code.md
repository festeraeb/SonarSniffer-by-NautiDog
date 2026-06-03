### Proposed Plan: Blue-Green Clarity Optimization

**1. Knob Adjustment (`types.rs`)**
The current z-score thresholds are too sensitive for the high-reflectance baseline of the clear-water Straits seasons, leading to excessive "outlier" flagging. We will shift the thresholds to account for the narrower spectral variance characteristic of clear water.
*   **Blue Band Z-Score Threshold:** Increase from `2.0` to `2.8` to prevent false positives from minor suspended sediment fluctuations.
*   **Green Band Z-Score Threshold:** Increase from `2.0` to `2.5` to accommodate the higher chlorophyll-a baseline in the Straits.
*   **Clarity Sensitivity Multiplier:** Reduce from `1.0` to `0.85` to dampen the impact of extreme single-pixel spikes.

**2. Mission Spec Modification**
Update the `mission_spec.json` (or the relevant configuration block in the mission spec) to implement a "Seasonal Profile."
*   Define a `straits_clear_water` profile that overrides the default `types.rs` constants.
*   Set `z_score_clip: null` to maintain the requirement for uncapped z-scores while applying the new adjusted thresholds.

**3. Testing**
Validate the scoring logic using the following commands:

1. `cargo run -- clear-water-straits -- knob-settings=adjusted`
2. `cargo test -- mission-spec=modified -- clear-water-straits`
3. `cargo compare -- before=original -- after=modified -- clear-water-straits`