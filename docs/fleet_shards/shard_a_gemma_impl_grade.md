

<!-- reasoning -->
Here's a thinking process:

1.  **Analyze User Input:**
   - **Shard:** `shard_a_gemma_impl (integrate_coder)`
   - **Model Output:** Contains files to touch, code changes for `chip.rs`, `poc.rs`, `mission.rs`, compile/test commands, and pitfalls.
   - **Task:** Grade the model's output against "handoff requirements" (implied context: satellite pipeline shard integration, likely for a Rust project dealing with Sentinel-2 data, GDAL, Rayon, coordinate transformations, etc.).
   - **Output Format:** Strictly these sections:
     - `## VERDICT: PASS | PARTIAL | FAIL`
     - `## Score (0-10)`
     - `## Strengths`
     - `## Gaps / must-fix`
     - `## Merge notes (what the final plan must include)`
   - **Constraint:** Under 350 words.

2.  **Evaluate Model Output against Implicit/Explicit Requirements:**
   - *Files touched:* `chip.rs`, `poc.rs`, `mission.rs` - reasonable for a satellite pipeline shard.
   - *Code changes:*
     - `chip.rs`: Mentions GDAL API fix, `bbox_to_pixel_window`, `decode_local_band`, coordinate conversion (WGS84 -> UTM -> Pixel), `read_as` signature, DN->Reflectance scaling (0.0001), return type. Good technical detail.
     - `poc.rs`: Function signature matches requirements. Logic covers globbing, Rayon parallelism, concept adaptation (blue/green clarity), temporal stack, cross-reference. Code snippet shows parallel map. Good.
     - `mission.rs`: Wiring logic with `if knobs.use_local_scenes`. Clear.
   - *Compile/test commands:* `cargo build`, calibration run command with JSON flag. Reasonable.
   - *Pitfalls:* GDAL coord mismatch, Rayon/GDAL thread safety, memory exhaustion, band mapping, scale factors. All highly relevant and technically sound.
   - *Overall:* The output is technically dense, addresses core integration challenges (GDAL, Rayon, coordinate systems, scaling), and provides actionable steps. It seems to fulfill a typical "handoff" for a coding task in this domain.

3.  **Identify Strengths:**
   - Precise GDAL API guidance (`read_as` signature, coordinate transformation pipeline).
   - Correctly identifies critical thread-safety issue with GDAL handles in `rayon`.
   - Explicitly addresses Sentinel-2 specific details (0.0001 scale factor, B02/B03 mapping).
   - Clear wiring logic in `mission.rs`.
   - Practical calibration/test command provided.

4.  **Identify Gaps / Must-Fix:**
   - Missing explicit error handling strategy for `decode_local_band` inside `par_iter` (e.g., `?` operator in closure needs proper `Result` propagation or `try_collect`).
   - No mention of `Dataset::flush_cache()` or explicit resource cleanup, which is critical for GDAL in Rust to avoid memory leaks.
   - The `bbox_to_pixel_window` description lacks concrete implementation hints for the GDAL `SpatialReference` transformation (e.g., `gdal::spatial_ref::SpatialRef` usage).
   - Test command assumes binary `sat-run` exists; should specify `cargo run --bin sat-run` or similar for reproducibility.

5.  **Formulate Merge Notes:**
   - Final plan must include explicit GDAL resource management (`Drop`/`flush_cache`).
   - Must verify `rayon`'s `try_collect` or equivalent for error propagation in parallel closures.
   - Add unit tests for `bbox_to_pixel_window` with known WGS84/UTM boundaries.
   - Pin GDAL crate version to avoid API drift.

6.  **Draft Response (mental refinement, checking constraints):**
   Need to keep it under 350 words total. Structure exactly as requested.

   ## VERDICT: PASS
   ## Score (0-10)
   8.5/10

   ## Strengths
   - Technically precise GDAL guidance, correctly specifying `read_as` signatures and the WGS84→UTM→Pixel transformation pipeline.
   - Accurately flags critical thread-safety constraints: GDAL handles must be opened per-thread inside `par_iter`, not shared.
   - Explicitly addresses Sentinel-2 specifics: 0.0001 DN-to-reflectance scaling, B02/B03 band mapping, and memory spike mitigation.
   - Clean wiring logic in `mission.rs` with clear conditional branching.

   ## Gaps /