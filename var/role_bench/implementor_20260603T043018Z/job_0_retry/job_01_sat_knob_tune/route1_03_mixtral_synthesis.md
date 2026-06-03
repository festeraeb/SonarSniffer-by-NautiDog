Based on the review of the blue-green clarity scoring system used in cesarops-satellite POC, I propose the following Knobs changes in types.rs and the mission spec for the clear-water Straits seasons:

1. Cap the z-scores:
To prevent extreme values, we can cap the z-scores at a threshold of 3.5. This can be implemented by adding a new field `z_score_cap` to the `ClarityKnobs` struct in types.rs:
```rust
pub struct ClarityKnobs {
    pub z_score_cap: f32, // Proposed: 3.5 (Standard deviation limit)
}
```
2. Use a different weighting for blue and green bands:
We can introduce a weighting factor to adjust the importance of each band. For example, we can give more weight to the blue band for clear-water Straits seasons. This can be implemented by adding two new fields `blue_weight` and `green_weight` to a new struct `SpectralKnobs` in types.rs:
```rust
pub struct SpectralKnobs {
    pub blue_weight: f32, // Proposed: 0.7
    pub green_weight: f32, // Proposed: 0.3
}
```
3. Introduce a seasonal factor:
We can introduce a seasonal factor to adjust the baseline expected clarity based on the Straits' seasonal cycle. This can be implemented by adding a new section `[seasonal_adjustment]` in the mission spec file:
```makefile
[seasonal_adjustment]
straits_clear_water_factor = 1.25 # Boosts score during high-transparency months
```
To test these changes, we can use the following three test commands:

1. Test command 1:
```css
cargo test -- --nocapture clarity_cap_validation
```
This command tests the impact of capping the z-scores on the overall scoring system.

2. Test command 2:
```css
cargo test -- --nocapture spectral_weight_sensitivity
```
This command tests the impact of adjusting the importance of each band on the overall scoring system.

3. Test command 3:
```css
cargo test -- --nocapture seasonal_factor_drift