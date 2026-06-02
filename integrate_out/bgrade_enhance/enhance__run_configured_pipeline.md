# enhance wreckhunter/run_configured_pipeline.py

Here's a thinking process:

1.  **Analyze User Input:**
   - **Goal:** Improve an existing Rust integrate-layer port (`run_configured_pipeline.rs`) by adding unit tests and deepening logic.
   - **Python Source:** `/mnt/t440/codebase/projects/pipelines/wreckhunter/run_configured_pipeline.py` (provided in truncated form)
   - **Target Rust File:** `/mnt/t440/codebase/repos/wreckhunter2000-1/cesarops-inference/src/integrate/run_configured_pipeline.rs`
   - **Existing Rust Code:** Provides `PipelineConfig`, `Default` impl, `THERMAL_GLOBS`, `gpu_command`, `load_config_json`.
   - **Python Code:** Shows `load_config`, `find_tiffs`, `run_rust_gpu`, `calculate_detection_score`, `export_results`, `main`. Key features: config loading from JSON, finding TIFFs with glob patterns, running a subprocess (GPU engine), parsing output for anomaly counts, calculating detection scores, exporting results (JSON/KML/CSV), batch/limit handling.
   - **Requirements:**
     - Output markdown ONLY with specific sections: `## Verdict`, `## Changes`, `## Rust path`, `## Rust source`, `## mod.rs wire`, `## Risks`
     - `## Rust source` must be a FULL replacement of the module with:
       - At least 2 `#[cfg(test)] mod tests` with real assertions
       - Expanded `pub fn` API matching Python behavior where practical
       - Serde types, minimal deps, style of `cesarops-inference/src/integrate/*.rs`
     - Start with `## Verdict`. No chain-of-thought.

2.  **Identify Gaps & Improvements Needed:**
   - The existing Rust code is very thin. It only handles config parsing and command building.
   - Missing: `find_tiffs` logic (glob matching), `calculate_detection_score`, result struct, export logic (or at least data structures for it), batch/limit handling, proper error handling.
   - Need to add tests for config parsing, score calculation, glob matching, and command generation.
   - Need to match Python's API structure but adapt it to Rust idioms (e.g., using `glob` crate or `walkdir`, but to keep deps minimal, I'll implement a simple glob matcher or use `std::fs::read_dir` with pattern matching, or just stick to `glob` if allowed. The prompt says "minimal deps", so I'll avoid external crates if possible, or use `glob` if it's standard in the repo. I'll stick to `std::fs` and `std::path` for simplicity, or implement a basic pattern matcher. Actually, `glob` is common, but I'll just use `std::fs::read_dir` and filter by suffix/pattern to keep it dependency-free, or I'll add `glob` to the prompt's implied context. I'll stick to `std` for minimal deps, but implement a robust `find_tiffs` using `std::fs::read_dir` recursively or just `glob` if I assume it's available. Wait, the prompt says "minimal deps", so I'll avoid `glob` and use `std::fs::read_dir` with a simple recursive search, or just implement a pattern matcher. Actually, `std::fs::read_dir` is fine. I'll write a helper that matches `**/*B10.tif` patterns.
   - I'll create structs for `PipelineResult`, `DetectionScore`, `ExportConfig`.
   - I'll implement `calculate_detection_score` with proper f32/f64 handling.
   - I'll implement `find_tiffs` using `std::fs::read_dir` recursively.
   - I'll add tests for config loading, score calculation, tiff finding, and command generation.
   - I'll ensure the style matches `cesarops-inference/src/integrate/*.rs` (typically uses `serde`, `thiserror` or standard `Result`, clear doc comments, modular functions).

3.  **Draft - Module Structure:**
   ```rust
   //! Config-driven GPU pipeline — port of `wreckhunter/run_configured_pipeline.py`.

   use serde::{Deserialize, Serialize};
   use std::path::{Path, PathBuf
