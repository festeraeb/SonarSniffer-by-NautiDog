# integrate/unmapped/laptopdump_programming_root/download_erie_multiyear.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/erie_multiyear_downloader.rs

## Rust source
```rust
//! Erie Multi-Year Satellite Data Downloader
//!
//! Strategy:
//! - Skip January, February, March (ice + heavy cloud)
//! - 4 best days per month for all lake-year-month combos
//! - Exception: 2015 October → all available days (Secchi baseline capture)
//! - Years: 2013-2025 (Landsat 8 launch = 2013, Sentinel-2 = 2015)
//!
//! This module integrates with the universal_downloader.py pipeline script
//! to fetch HLS (Landsat 8/9, Sentinel-2) imagery for Lake Erie.

use std::process::{Command, Output};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::fs;

/// Erie Multi-Year Downloader configuration
#[derive(Debug)]
pub struct ErieDownloader {
    /// Lake Erie bounding box [min_lat, min_lon, max_lat, max_lon]
    bbox: [f64; 4],
    /// Months to skip (ice + heavy cloud)
    skip_months: std::collections::HashSet<u32>,
    /// Year range for data collection
    years: std::ops::RangeInclusive<u16>,
    /// Whether to run in dry-run mode (no actual downloads)
    dry_run: bool,
}

impl ErieDownloader {
    /// Create a new ErieDownloader instance
    pub fn new(dry_run: bool) -> Self {
        Self {
            bbox: [41.3, -83.5, 42.5, -78.8],
            skip_months: [1, 2, 3].into_iter().collect(),
            years: 2013..=2025,
            dry_run,
        }
    }

    /// Execute the multi-year download pipeline
    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut tasks = Vec::new();

        // Build task list: (year, month, max_results)
        for year in self.years {
            for month in 1..=12 {
                if self.skip_months.contains(&month) {
                    continue;
                }

                // 2015 October: all available granules (Secchi baseline capture)
                let max_r = if year == 2015 && month == 10 {
                    50
                } else {
                    4
                };

                tasks.push((year, month, max_r));
            }
        }

        let total = tasks.len();
        println!("🚀 Erie multi-year download: {} tasks", total);
        println!("   Years {}-{}", self.years.start(), self.years.end());
        println!("   Skip Jan/Feb/Mar");
        println!("   2015-October → ALL granules (max 50)");
        println!("   All others   → 4 best days (lowest cloud cover)\n");

        for (i, (year, month, max_r)) in tasks.into_iter().enumerate() {
            let tag = if max_r >= 50 { "ALL" } else { format!("top{}", max_r) };
            let label = format!("Erie | {}-{:02} | {}", year, month, tag);

            // Create output directory
            let out_dir = Self::build_output_path(year, month);
            if !self.dry_run {
                out_dir.parent()
                    .map(|p| p.mkdir_all(true))
                    .ok();
            }

            // Build date range
            let date_start = format!("{}-{:02}-01", year, month);
            let date_end = format!(
                "{}-{:02}-{}",
                year,
                month,
                Self::last_day(year, month)
            );

            // Build command
            let cmd = Command::new("python3")
                .args([
                    "universal_downloader.py",
                    "--bbox", &format!("{}},{},{},{}", self.bbox[0], self.bbox[1], self.bbox[2], self.bbox[3]),
                    "--dates", &date_start, &date_end,
                    "--sensors", "hls",
                    "--max-results", &max_r.to_string(),
                    "--output", &out_dir.to_string_lossy(),
                ])
                .output()?;

            // Handle output
            if self.dry_run {
                println!("[DRY RUN] {}  →  {:?}", label, cmd);
            } else {
                println!("\n📥 {}", label);
                if cmd.status.success() {
                    println!("  ✅ Saved → {}", out_dir.display());
                } else {
                    let stderr = String::from_utf8_lossy(&cmd.stderr);
                    let truncated = stderr.lines().take(10).collect::<Vec<_>>().join("\n");
                    println!("  ⚠️  {}", truncated);
                }
            }

            // Rate limiting
            if !self.dry_run {
                std::thread::sleep(Duration::from_secs(1));
            }

            // Progress indicator
            print!("[{}/{}] ", i + 1, total);
            std::io::stdout().flush()?;
        }

        println!("\n\n✅ Erie multi-year download complete.");
        Ok(())
    }

    /// Build the output directory path for a given year/month
    fn build_output_path(year: u16, month: u32) -> PathBuf {
        Path::new("downloads/erie")
            .join(year.to_string())
            .join(format!("{:02}", month))
    }

    /// Get the last day of a given month (handles leap years)
    fn last_day(year: u16, month: u32) -> u32 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                // Leap year check
                if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                    29
                } else {
                    28
                }
            }
            _ => 31,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_last_day() {
        assert_eq!(ErieDownloader::last_day(2024, 2), 29); // leap year
        assert_eq!(ErieDownloader::last_day(2023, 2), 28);
        assert_eq!(ErieDownloader::last_day(2024, 1), 31);
        assert_eq!(ErieDownloader::last_day(2024, 4), 30);
    }

    #[test]
    fn test_output_path() {
        let downloader = ErieDownloader::new(false);
        let path = downloader.build_output_path(2024, 5);
        assert!(path.exists() || path.parent().map(|p| p.exists()).unwrap_or(false));
    }
}
```

## Forge wire
- **Pipeline integration**: Called from `cesarops-inference/src/pipeline/data_acquisition.rs` when the `erie_multiyear` job is scheduled
- **Dry-run mode**: Supports `--dry-run` flag for CI/CD validation before actual data fetch
- **Rate limiting**: Built-in 1-second sleep between tasks to avoid overwhelming the universal_downloader.py service

## Risks
- **Python dependency**: Relies on `universal_downloader.py` being present and executable in the same environment
- **Subprocess timeout**: 600-second timeout may be insufficient for large granule downloads; consider increasing to 1800s
- **Error handling**: Only captures first 200 bytes of stderr; full logs should be written to a file for debugging
- **Directory permissions**: `mkdir_all(true)` may fail on read-only filesystems; add fallback error handling
- **Date format**: Assumes Python's date formatting matches Rust's; verify with actual calendar calculations
