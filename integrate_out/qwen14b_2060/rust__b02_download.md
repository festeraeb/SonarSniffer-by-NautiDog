# satellite/b02_download.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/hls_b02_download.rs

## Rust source
```rust
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use reqwest::blocking::{Client, Response};
use reqwest::header::{AUTHORIZATION, ACCEPT};
use serde_json::Value;

const REPO: &str = env!("CARGO_MANIFEST_DIR");
const OUTPUT_DIR: &str = "downloads/hls/straits_2015_2016";

fn read_env_file(path: &Path) -> HashMap<String, String> {
    let mut env = HashMap::new();
    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') && line.contains('=') {
                let parts: Vec<&str> = line.splitn(2, '=').collect();
                env.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
            }
        }
    }
    env
}

fn get_granule_dirs(output_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(output_dir) {
        for entry in entries {
            if let Ok(entry) = entry {
                if entry.file_type().unwrap().is_dir() {
                    dirs.push(entry.path());
                }
            }
        }
    }
    dirs.sort();
    dirs
}

fn download_b02(session: &Client, granule_dirs: &[PathBuf]) -> io::Result<(u64, u32)> {
    let mut downloaded_bytes = 0;
    let mut skipped = 0;

    for (i, gdir) in granule_dirs.iter().enumerate() {
        let existing_files: Vec<_> = gdir.read_dir()?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().unwrap().is_file() && entry.file_name().to_string_lossy().ends_with(".tif"))
            .collect();

        if existing_files.is_empty() {
            continue;
        }

        let title = existing_files[0].file_name().to_string_lossy().split(".Fmask").next().unwrap_or("").split(".B").next().unwrap_or("");
        let title = if title.ends_with(".v2.0") {
            title.to_string()
        } else {
            gdir.file_name().unwrap().to_string_lossy().to_string()
        };

        let b02_dest = gdir.join(format!("{}.B02.tif", title));
        if b02_dest.exists() && b02_dest.metadata()?.len() > 0 {
            skipped += 1;
            continue;
        }

        let short_name = if title.contains("L30") { "HLSL30" } else { "HLSS30" };
        let params = [
            ("short_name", short_name),
            ("page_size", "1"),
            ("granule_ur", &title),
        ];

        let response: Response = session.get("https://cmr.earthdata.nasa.gov/search/granules.json")
            .query(&params)
            .send()?;

        if response.status().is_success() {
            let json: Value = response.json()?;
            if let Some(entries) = json["feed"]["entry"].as_array() {
                if let Some(entry) = entries.first() {
                    if let Some(links) = entry["links"].as_array() {
                        for link in links {
                            if let Some(href) = link["href"].as_str() {
                                if href.contains("B02.tif") && link["rel"].as_str() == Some("data#") {
                                    let response = session.get(href)
                                        .send()?;

                                    if response.status().is_success() {
                                        let mut file = fs::File::create(&b02_dest)?;
                                        let content = response.bytes()?;
                                        file.write_all(&content)?;
                                        let sz = content.len() as u64;
                                        downloaded_bytes += sz;
                                        println!("[{}/{}] {} B02... {:.1}MB", i + 1, granule_dirs.len(), title, sz as f64 / 1024.0 / 1024.0);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok((downloaded_bytes, skipped))
}
