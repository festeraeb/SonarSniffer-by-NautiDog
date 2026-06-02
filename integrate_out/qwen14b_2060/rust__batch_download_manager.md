# wreckhunter/batch_download_manager.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/batch_download_manager.rs

## Rust source
```rust
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
struct Opt {
    #[structopt(short, long, default_value = "all")]
    lakes: String,
    #[structopt(short, long, default_value = "2013")]
    start: i32,
    #[structopt(short, long, default_value = "2025")]
    end: i32,
    #[structopt(short, long, default_value = "hls,sar")]
    sensors: String,
    #[structopt(short, long, default_value = "15")]
    max_results: i32,
    #[structopt(short, long)]
    chunk_id: Option<String>,
    #[structopt(short, long)]
    dry_run: bool,
}

#[derive(Debug)]
struct Lake {
    bbox: [f64; 4],
    label: String,
}

const LAKES: HashMap<&str, Lake> = HashMap::from([
    (
        "superior",
        Lake {
            bbox: [46.5, -92.0, 48.0, -84.5],
            label: "Lake Superior".to_string(),
        },
    ),
    (
        "michigan",
        Lake {
            bbox: [41.5, -88.0, 46.0, -85.5],
            label: "Lake Michigan".to_string(),
        },
    ),
    (
        "straits",
        Lake {
            bbox: [45.65, -85.0, 46.10, -84.10],
            label: "Straits of Mackinac".to_string(),
        },
    ),
    (
        "huron",
        Lake {
            bbox: [42.5, -84.0, 46.0, -81.0],
            label: "Lake Huron".to_string(),
        },
    ),
    (
        "erie",
        Lake {
            bbox: [41.3, -83.5, 42.5, -78.8],
            label: "Lake Erie".to_string(),
        },
    ),
    (
        "ontario",
        Lake {
            bbox: [43.2, -79.5, 44.2, -76.0],
            label: "Lake Ontario".to_string(),
        },
    ),
]);

fn run_download(lake_key: &str, year: i32, sensors: &str, chunk
