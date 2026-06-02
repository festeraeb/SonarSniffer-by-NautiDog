# enhance satellite/b02_download.py

Here's a thinking process:

1.  **Analyze User Input:**
   - **Goal:** Improve an existing Rust integrate-layer port (`hls_b02_download.rs`) by adding unit tests and deepening logic.
   - **Python Source:** `/mnt/t440/codebase/projects/pipelines/satellite/b02_download.py` (truncated)
   - **Target Rust File:** `/mnt/t440/codebase/repos/wreckhunter2000-1/cesarops-inference/src/integrate/hls_b02_download.rs`
   - **Existing Rust Code:** Provides basic structs, constants, and helper functions (`product_for_title`, `b02_dest_name`, `granule_query_params`, `pick_b02_href`, `should_skip_existing`). It's thin, lacks tests, and doesn't match the full Python behavior (which involves reading directories, parsing titles, checking existing files, querying CMR, downloading with streaming, tracking stats).
   - **Requirements:**
     - Output markdown ONLY with specific sections: `## Verdict`, `## Changes`, `## Rust path`, `## Rust source`, `## mod.rs wire`, `## Risks`
     - `## Rust source` must be a FULL replacement ` ```rust ... ``` ` module
     - Must include at least 2 `#[cfg(test)] mod tests` with real assertions
     - Must expand `pub fn` API matching Python behavior where practical
     - Must use serde types, minimal deps, style of `cesarops-inference/src/integrate/*.rs`
     - Must include `pub mod line` for `mod.rs` wire
     - Must include `## Risks` bullets
     - Start with `## Verdict`. No chain-of-thought.

2.  **Deconstruct Python Behavior:**
   - Reads `.env` for Earthdata token
   - Sets up `requests.Session` with auth headers
   - Iterates over directories in `output_dir`
   - Extracts granule title from existing `.tif` files or directory name
   - Checks if `title.B02.tif` exists and has size > 0
   - Queries CMR with `short_name`, `page_size=1`, `granule_ur=title`
   - Parses JSON response to find B02 link (`href` contains `B02.tif`, `rel` contains `data#`)
   - Downloads B02 with streaming, chunk size 1MB, timeout 300s
   - Tracks downloaded bytes and skipped count
   - Prints progress/stats

3.  **Map to Rust API Design:**
   - Need a config struct for paths, token, output dir
   - Need a job/result struct to track progress
   - Need functions for:
     - Title extraction from directory/filenames
     - CMR query params generation (already exists, can keep/refine)
     - B02 link extraction from CMR JSON (already exists, can keep/refine)
     - Skip check (already exists, can keep/refine)
     - Product name mapping (already exists, can keep/refine)
     - Download function (streaming, chunk size, timeout, auth)
     - Main orchestration function (iterates dirs, checks existence, queries CMR, downloads, aggregates stats)
   - Use `reqwest` for HTTP (standard in Rust ecosystem, matches Python's `requests`)
   - Use `serde_json` for CMR response parsing
   - Keep it minimal and focused on the integrate layer (no heavy filesystem scanning if not needed, but I'll add a realistic orchestration function that matches Python's loop)

   Let's design the Rust module:
   ```rust
   //! HLS B02 band download — port of `satellite/b02_download.py`.
   //! Provides CMR querying, B02 link extraction, and streaming download with progress tracking.

   use serde::{Deserialize, Serialize};
   use std::path::Path;

   pub const CMR_GRANULES_URL: &str = "https://cmr.earthdata.nasa.gov/search/granules.json";

   #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
   pub struct B02DownloadJob {
       pub granule_title: String,
       pub product_short_name: String,
       pub dest_path: String,
   }

   #[derive(Debug, Default, Clone, Serialize, Deserialize)]
   pub struct B02DownloadStats {
       pub downloaded_bytes: u64,
       pub skipped_count: usize,
       pub failed_count: usize,
   }

   // ... helper functions ...
   // ... download function ...
   // ... orchestration function ...
   // ... tests ...
   ```

   I'll need to add `reqwest` and `tokio` as dependencies? The prompt says "minimal deps, style of cesarops-inference/src/integrate/*.rs". I'll assume `reqwest` and `tokio` are available or I'll write async functions that can be run with a runtime. I'll stick to `reqwest` for HTTP and `tokio` for async, but I'll make it clear in comments. Actually, integrate layers often use `reqwest` directly. I'll write async functions.

   Let's refine the functions to match Python closely but in idiomatic Rust:
   - `product_for_title` -> keep
   - `b02_dest_name` -> keep
   - `granule_query_params` -> keep, maybe return `HashMap` or `Vec`
   - `pick_b02_href` -> keep, but improve to handle nested JSON properly
   - `should_skip_existing` -> keep, but maybe rename to `is_b02_complete`
   - Add `extract_granule_title` (from dir or existing files)
   - Add `query_cmr_granule` (async, returns parsed entry or error)
   - Add `download_b02_stream` (async, returns bytes downloaded)
   - Add `run_b02_download_cycle` (orchestrates the loop)

   I'll structure it carefully. Since I don't know the exact crate dependencies, I'll use standard `reqwest` and `tokio` patterns, and note them. I'll keep it self-contained.

   Let's draft the Rust code mentally:
   ```rust
   //! HLS B02 band download — port of `satellite/b02_download.py`.
   //! Provides CMR querying, B02 link extraction, and streaming download with progress tracking.

   use serde::{Deserialize, Serialize};
   use std::path::Path;

   pub const CMR_GRANULES_URL: &str = "https://cmr.earthdata.nasa.gov/search/granules.json";

   #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
   pub struct B02DownloadJob {
       pub granule_title: String,
       pub product_short_name: String,
       pub dest_path: String,
   }

   #[derive(Debug, Default, Clone, Serialize, Deserialize)]
   pub struct B02DownloadStats {
       pub downloaded_bytes: u64,
       pub skipped_count: usize,

