### Physics-Based Review & Proposed Knobs Implementation

**Review:**
The current uncapped z-score implementation in `cesarops-satellite` is susceptible to high-frequency noise and sensor outliers, which disproportionately skew the clarity index. In the Straits' clear-water seasons, the signal-to-noise ratio in the blue band is high, but extreme outliers can create non-physical clarity spikes.

**Proposed Changes in `types.rs` / Mission Spec:**

1.  **Z-Score Capping (Outlier Suppression):**
    Introduce a `ClarityThreshold` knob to clamp the z-score distribution.
    ```rust
    // types.rs
    pub struct ClarityKnobs {
        pub z_score_cap: f32, // Proposed: 3.5 (Standard deviation limit)
    }
    ```

2.  **Spectral Weighting (Band Sensitivity):**
    Introduce a `BandWeight` knob to prioritize the blue band ($\lambda_{blue}$) over the green band ($\lambda_{green}$) during clear-water seasons, as blue light penetrates deeper and provides a more precise proxy for particulate matter in low-turbidity environments.
    ```rust
    // types.rs
    pub struct SpectralKnobs {
        pub blue_weight: f32, // Proposed: 0.7
        pub green_weight: f32, // Proposed: 0.3
    }
    ```

3.  **Seasonal Normalization Factor:**
    Introduce a `SeasonalCoefficient` to adjust the baseline expected clarity based on the Straits' seasonal cycle (e.g., Monsoon vs. Dry season).
    ```rust
    // mission_spec.toml
    [seasonal_adjustment]
    straits_clear_water_factor = 1.25 // Boosts score during high-transparency months
    ```

**Test Commands:**

1.  `cargo test -- --nocapture clarity_cap_validation`
    *(Evaluates the delta between uncapped and capped z-scores using a synthetic dataset with extreme outliers.)*

2.  `cargo test -- --nocapture spectral_weight_sensitivity`
    *(Compares the clarity index output when toggling `blue_weight` from 0.5 to 0.7.)*

3.  `cargo test -- --nocapture seasonal_factor_drift`
    *(Verifies that the `straits_clear_water_factor` correctly shifts the baseline score without inducing mathematical instability.)*