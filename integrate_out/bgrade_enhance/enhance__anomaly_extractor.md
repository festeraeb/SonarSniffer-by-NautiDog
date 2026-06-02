# enhance processing/anomaly_extractor.py

Here's a thinking process:

1.  **Analyze User Input:**
   - **Goal:** Improve an existing Rust integrate-layer port (`process_with_coordinates.rs`) by adding unit tests and deepening logic.
   - **Python Source:** `/mnt/t440/codebase/projects/pipelines/processing/anomaly_extractor.py` (truncated but shows the core logic: parsing GPU output, extracting dimensions, parsing anomaly lines, converting pixel to UTM, UTM to WGS84, building a structured result).
   - **Target Rust File:** `/mnt/t440/codebase/repos/wreckhunter2000-1/cesarops-inference/src/integrate/process_with_coordinates.rs`
   - **Current Rust Code:** Has basic structs, a parser for GPU output lines, pixel-to-UTM conversion, UTM-to-WGS84 approximation, and a builder function. Missing tests, lacks robustness, doesn't match Python's full output structure (dimensions, GPU info, JSON-like structure), and has some potential issues in parsing/conversion.
   - **Requirements:**
     - Output markdown ONLY with specific sections: `## Verdict`, `## Changes`, `## Rust path`, `## Rust source`, `## mod.rs wire`, `## Risks`
     - `## Rust source` must be a FULL replacement with ` ```rust ... ``` `
     - Must include at least 2 `#[cfg(test)] mod tests` with real assertions
     - Must expand `pub fn` API to match Python behavior where practical
     - Use serde types, minimal deps, style of `cesarops-inference/src/integrate/*.rs`
     - Start with `## Verdict`. No chain-of-thought.

2.  **Identify Issues & Improvements in Current Rust:**
   - *Parsing:* `parse_gpu_anomaly_line` uses a naive split on non-digits/dots. Python uses regex `r'Pixel \((\d+), (\d+)\): Z-Score ([\d.]+)'`. The Rust version is fragile. I'll improve it to use regex or a more robust string parsing approach. Since minimal deps are preferred, I'll stick to standard library string methods but make it more precise, or use `regex` if allowed. The prompt says "minimal deps", so I'll avoid `regex` and use standard `split`/`find`/`parse` carefully, or just implement a robust parser without external crates. Actually, `regex` is common in Rust, but I'll stick to std to be safe, or just use `split` and `find` carefully. Let's use a simple state-machine or `split` with `find` for robustness.
   - *Dimensions Parsing:* Python extracts `width` and `height` from a line containing "Loaded" and "thermal data". Rust doesn't have this. I'll add `parse_gpu_dimensions_line`.
   - *Coordinate Conversion:* The approximations are hardcoded. I'll make them configurable or at least document them. The Python code uses `450000 + col * 30` and `4700000 + (height - row) * 30`. The Rust code uses `450_000.0` and `4_700_000.0`. I'll keep the math but make it explicit. I'll also add a proper `ProcessResult` struct matching Python's output structure.
   - *API Expansion:* Python returns a dict with `tiff`, `dimensions`, `anomalies`, `gpu`. I'll create a `ProcessResult` struct with serde. I'll add a `parse_gpu_output` function that processes the full stdout string, extracting dimensions, anomalies, and GPU info.
   - *Tests:* Need at least 2 test modules with real assertions. I'll test `parse_gpu_anomaly_line`, `parse_gpu_dimensions_line`, `pixel_to_approx_utm`, `approx_utm_to_wgs84`, and `parse_gpu_output`.

3.  **Design the New Rust Module:**
   - Structs: `PixelAnomalyOut` (keep), `ProcessDimensions`, `ProcessResult`, `GpuInfo`
   - Functions:
     - `parse_gpu_dimensions_line(line: &str) -> Option<(u
