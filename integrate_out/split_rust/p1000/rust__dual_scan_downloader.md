# integrate/unmapped/laptopdump_wreckhunter_build/dual_scan_downloader.py

## Verdict
ARCHIVE_STUB

## Rust path
cesarops-inference/src/integrate/lake_michigan_dual_scan.rs

## Rust source
```rust
use chrono::NaiveDate;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, warn, debug};

/// Lake Michigan WGS84 bounding box: (north, south, west, east)
const LAKE_MICHIGAN_BOUNDS: (f64, f64, f64, f64) = (46.10, 41.60, -88.10, -84.70);

/// Priority low-water acquisition windows
#[derive(Debug, Clone)]
pub struct DateWindow {
    pub start: NaiveDate,
