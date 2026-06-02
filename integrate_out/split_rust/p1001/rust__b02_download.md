# integrate/unmapped/laptopdump_programming_root/b02_download.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/hls_b02_download.rs

## Rust source
```rust
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;
use reqwest::Client;
use log::{info, warn, debug, error};

/// HLS B02 Band Download Module
/// Downloads B02 band TIFF files from NASA EarthData CMR for HLS L30/S30 granules
pub mod hls_b02_download {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    struct GranuleEntry {
        id: String,
        title: String,
        links: Vec<Link>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Link {
        href: String,
        rel: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct SearchResponse {
        feed: Option<Feed>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Feed {
        entry: Option<Vec<GranuleEntry>>,
    }

    #[derive(Debug)]
    struct Config {
        output_dir: PathBuf,
        earthdata_token: Option<String>,
    }

    impl Config {
        pub fn new(output_dir: &Path) -> io::Result<Self> {
            let mut config = Self {
                output_dir: output_dir.to_path_buf(),
                earthdata_token: None,
            };

            // Load .env file if exists
            let env_path = output_dir.parent().unwrap_or(Path::new("."))
                .join(".env");
            if env_path.exists() {
                let env_content = fs::read_to_string(&env_path)?;
                for line in env_content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((key, value)) = line.split_once('=') {
                        let key = key.trim();
                        let value = value.trim();
                        if key == "EARTHDATA_TOKEN" {
                            config.earthdata_token = Some(value.to_string());
                        }
                    }
                }
            }

            Ok(config)
        }

        pub fn create_client(&self) -> io::Result<Client> {
            let mut client = Client::builder()
                .timeout(Duration::from_secs(300))
                .build()?;

            let token = self.earthdata_token.as_deref()
                .unwrap_or("");
            let auth_header = format!("Bearer {}", token);

            let mut headers = client.default_headers();
            headers.insert(
                "Authorization",
                auth_header.parse().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
            );
            headers.insert(
                "Accept",
                "application/json".parse().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
            );

            Ok(client)
        }
    }

    /// Check if a file already exists and has content
    fn file_exists_with_size(path: &Path) -> io::Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        let metadata = fs::metadata(path)?;
        Ok(metadata.len() > 0)
    }

    /// Extract title from existing TIFF file
    fn extract_title_from_file(path: &Path) -> io::Result<String> {
        let filename = path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "No filename")
        })?
        .to_str()
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Invalid filename")
        })?;

        // Remove .Fmask and .B extensions to get base title
        let base = filename
            .strip_suffix(".Fmask")
            .or_else(|| filename.strip_suffix(".B"))
            .unwrap_or(filename);

        // Remove .v2.0 if present
        let title = base.strip_suffix(".v2.0").unwrap_or(base);

        Ok(title.to_string())
    }

    /// Search CMR for a specific granule
    async fn search_granule(
        client: &Client,
        title: &str,
    ) -> io::Result<Option<String>> {
        let short_name = if title.contains("L30") {
            "HLSL30"
        } else {
            "HLSS30"
        };

        let url = format!(
            "https://cmr.earthdata.nasa.gov/search/granules.json?short_name={}&page_size=1&granule_ur={}",
            short_name, title
        );

        let response = client.get(&url).send().await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let json: SearchResponse = response.json().await?;
        let entries = match json.feed {
            Some(feed) => feed.entry,
            None => return Ok(None),
        };

        let entries = entries.unwrap_or_default();
        if entries.is_empty() {
            return Ok(None);
        }

        let entry = &entries[0];

        // Find B02 link
        for link in &entry.links {
            let href = &link.href;
            let rel = link.rel.as_deref();
            if href.contains("B02.tif") && rel.map(|r| r.contains("data#")).unwrap_or(false) {
                return Ok(Some(href.clone()));
            }
        }

        Ok(None)
    }

    /// Download a single B02 band file
    async fn download_b02(
        client: &Client,
        dest_path: &Path,
        url: &str,
        progress_callback: impl Fn(u64) + Send + Sync,
    ) -> io::Result<u64> {
        let mut file = File::create(dest_path)?;
        let mut downloaded = 0u64;

        let response = client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                format!("HTTP {}", response.status()),
            ));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = vec![0u8; 1 << 20]; // 1MB chunks

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            let chunk_size = chunk.len() as u64;
            file.write_all(&chunk)?;
            downloaded += chunk_size;
            progress_callback(downloaded);
        }

        Ok(downloaded)
    }

    /// Process a single granule directory
    async fn process_granule(
        client: &Client,
        granule_dir: &Path,
        config: &Config,
        progress_callback: impl Fn(u64) + Send + Sync,
    ) -> io::Result<()> {
        let existing_files: Vec<_> = granule_dir
            .glob("*.tif")
            .filter_map(|path| path.to_path_buf().ok())
            .collect();

        if existing_files.is_empty() {
            debug!("No TIFF files found in granule directory: {:?}", granule_dir);
            return Ok(());
        }

        // Extract title from first existing file
        let title = extract_title_from_file(&existing_files[0])?;

        // Check if already correct
        if title.ends_with(".v2.0") {
            debug!("Title already correct: {:?}", title);
        }

        // Check if B02 already exists
        let b02_dest = granule_dir.join(format!("{}.B02.tif", title));
        if file_exists_with_size(&b02_dest)? {
            debug!("B02 already exists: {:?}", b02_dest);
            return Ok(());
        }

        // Search for B02 link
        let b02_link = search_granule(client, &title).await?;
        if b02_link.is_none() {
            debug!("No B02 link found for granule: {:?}", title);
            return Ok(());
        }

        let b02_link = b02_link.unwrap();
        info!("Found B02 link: {}", b02_link);

        // Download B02
        let downloaded = download_b02(client, &b02_dest, &b02_link, progress_callback).await?;
        info!("Downloaded {} bytes for {}", downloaded, b02_dest.display());

        Ok(())
    }

    /// Main download function
    pub async fn run(config: Config) -> io::Result<()> {
        let client = config.create_client()?;
        let output_dir = &config.output_dir;

        // Ensure output directory exists
        if !output_dir.exists() {
            fs::create_dir_all(output_dir)?;
        }

        // Get all granule directories
        let granule_dirs: Vec<_> = output_dir
            .read_dir()?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.path())
            .collect();

        info!("Found {} granule directories", granule_dirs.len());

        let mut downloaded_bytes = 0u64;
        let mut skipped = 0u64;

        for (i, granule_dir) in granule_dirs.iter().enumerate() {
            let progress_callback = |bytes: u64| {
                let mb = bytes / 1024 / 1024;
                eprintln!("[{}/{}] {} MB", i + 1, granule_dirs.len(), mb);
            };

            match process_granule(&client, granule_dir, &config, progress_callback).await {
                Ok(()) => {
                    // Check if we actually downloaded something
                    let b02_dest = granule_dir.join(format!("{}.B02.tif", extract_title_from_file(&granule_dir.join("*.tif").as_ref().unwrap().unwrap_or(Path::new(""))).unwrap_or("unknown")));
                    if b02_dest.exists() {
                        let size = b02_dest.metadata().unwrap().len();
                        downloaded_bytes += size;
                    }
                }
                Err(e) => {
                    error!("Error processing granule {:?}: {}", granule_dir, e);
                }
            }
        }

        info!("=== B02 DOWNLOAD COMPLETE ===");
        info!("  Downloaded: {} MB", downloaded_bytes / 1024 / 1024);
        info!("  Skipped (already exists): {}", skipped);

        Ok(())
    }
}
```

## Forge wire
- **Pipeline Integration**: The `hls_b02_download` module is called by the HLS data pipeline after granule directories are created but before B02 band processing
- **Trigger**: Invoked when the pipeline detects new granule directories in the `downloads/hls/straits_2015_2016` output directory
- **Output**: Produces `.B02.tif` files in each granule directory, which are then consumed by downstream band processing modules

## Risks
- **Token Management**: EarthData token must be properly rotated and stored securely; missing token will cause all downloads to fail
- **Network Timeouts**: 300-second timeout may be insufficient for large files on slow connections; consider exponential backoff
- **Partial Downloads**: Network interruptions could leave partial files; add file integrity checks (MD5/SHA256) after download
- **Path Resolution**: The `.env` file path resolution assumes a specific directory structure; add fallback to environment variables
- **Memory Usage**: Streaming with 1MB chunks is memory-efficient, but ensure the buffer size is appropriate for the target hardware
