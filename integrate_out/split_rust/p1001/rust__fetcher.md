# /codebase/projects/pipelines/wreckhunter/fetcher.py

## Verdict
PORT_TO_PIPELINES
## Rust path
cesarops-inference/src/integrate/fetcher.rs
## Rust source
```rust
//! CESAROPS FETCHER - Satellite Data Download Sidecar
//! Downloads Landsat-9/Sentinel-2 data from USGS EarthExplorer + Sentinel Hub
//!
//! This module is compiled into the cesarops-inference pipeline.

use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use reqwest::{Client, Error as ReqwestError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error types for the fetcher module
#[derive(Debug, Error)]
pub enum FetcherError {
    #[error("USGS API error: {0}")]
    UsgsApi(#[from] ReqwestError),
    #[error("Sentinel Hub API error: {0}")]
    SentinelHub(#[from] ReqwestError),
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Authentication failed: {0}")]
    Auth(String),
    #[error("Invalid date range: {0}")]
    InvalidDate(String),
}

/// Result type alias
pub type Result<T> = std::result::Result<T, FetcherError>;

/// Default data directory
pub fn default_data_dir() -> PathBuf {
    env::var("CESAROPS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/codebase/projects/pipelines/data"))
}

/// Lake Michigan bounding box
pub const LAKE_MICHIGAN_BOUNDS: Bounds = Bounds {
    north: 46.10,
    south: 41.60,
    west: -88.10,
    east: -84.70,
};

/// Lake Superior bounding box
pub const LAKE_SUPERIOR_BOUNDS: Bounds = Bounds {
    north: 49.00,
    south: 46.00,
    west: -92.50,
    east: -84.00,
};

/// Lake Huron bounding box
pub const LAKE_HURON_BOUNDS: Bounds = Bounds {
    north: 46.50,
    south: 43.00,
    west: -84.50,
    east: -81.00,
};

/// All Great Lakes combined
pub const ALL_GREAT_LAKES_BOUNDS: Bounds = Bounds {
    north: 49.00,
    south: 41.00,
    west: -92.50,
    east: -76.00,
};

/// Bounding box structure
#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    pub north: f64,
    pub south: f64,
    pub west: f64,
    pub east: f64,
}

impl Bounds {
    /// Create a new bounding box
    pub fn new(north: f64, south: f64, west: f64, east: f64) -> Self {
        Self { north, south, west, east }
    }

    /// Check if a point is within the bounds
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        lat >= self.south && lat <= self.north && lon >= self.west && lon <= self.east
    }
}

/// Date window structure
#[derive(Debug, Clone, Copy)]
pub struct DateWindow {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl DateWindow {
    /// Create a new date window
    pub fn new(start: NaiveDate, end: NaiveDate) -> Self {
        Self { start, end }
    }

    /// Check if a date is within the window
    pub fn contains(&self, date: NaiveDate) -> bool {
        date >= self.start && date <= self.end
    }
}

/// Ice break windows for dual-scan
pub const ICE_BREAK_WINDOWS: [DateWindow; 2] = [
    DateWindow::new(
        NaiveDate::from_ymd_opt(2024, 3, 15).unwrap(),
        NaiveDate::from_ymd_opt(2024, 4, 30).unwrap(),
    ),
    DateWindow::new(
        NaiveDate::from_ymd_opt(2025, 3, 15).unwrap(),
        NaiveDate::from_ymd_opt(2025, 4, 30).unwrap(),
    ),
];

/// Low silt windows
pub const LOW_SILT_WINDOWS: [DateWindow; 2] = [
    DateWindow::new(
        NaiveDate::from_ymd_opt(2023, 5, 20).unwrap(),
        NaiveDate::from_ymd_opt(2023, 6, 15).unwrap(),
    ),
    DateWindow::new(
        NaiveDate::from_ymd_opt(2024, 5, 20).unwrap(),
        NaiveDate::from_ymd_opt(2024, 6, 15).unwrap(),
    ),
];

/// USGS API base URL
const USGS_API_BASE: &str = "https://earthexplorer.usgs.gov/api/v1";

/// Sentinel Hub API base URL
const SENTINEL_HUB_BASE: &str = "https://services.sentinel-hub.com/api/v1";

/// USGS API response structure
#[derive(Debug, Deserialize)]
pub struct UsgsApiResponse {
    pub success: bool,
    #[serde(default)]
    pub data: Option<UsgsApiData>,
    #[serde(default)]
    pub error_msg: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UsgsApiData {
    #[serde(default)]
    pub results: Vec<UsgsScene>,
}

/// USGS scene structure
#[derive(Debug, Deserialize)]
pub struct UsgsScene {
    pub id: String,
    #[serde(default)]
    pub product_id: Option<String>,
}

/// Sentinel Hub API response structure
#[derive(Debug, Deserialize)]
pub struct SentinelHubResponse {
    #[serde(default)]
    pub features: Vec<SentinelFeature>,
}

/// Sentinel Hub feature structure
#[derive(Debug, Deserialize)]
pub struct SentinelFeature {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub properties: Option<SentinelProperties>,
}

#[derive(Debug, Deserialize)]
pub struct SentinelProperties {
    #[serde(default)]
    pub datetime: Option<String>,
}

/// USGS Fetcher for Landsat data
pub struct UsgsFetcher {
    username: Option<String>,
    password: Option<String>,
    api_key: Option<String>,
    client: Client,
}

impl UsgsFetcher {
    /// Create a new USGS fetcher
    pub fn new(username: Option<String>, password: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            username,
            password,
            api_key: None,
            client,
        }
    }

    /// Login to USGS API
    pub fn login(&mut self) -> Result<bool> {
        let username = self.username.as_deref().ok_or_else(|| FetcherError::Auth("USGS username not provided".to_string()))?;
        let password = self.password.as_deref().ok_or_else(|| FetcherError::Auth("USGS password not provided".to_string()))?;

        let url = format!("{}/login", USGS_API_BASE);
        let response = self.client
            .post(&url)
            .json(&serde_json::json!({
                "username": username,
                "password": password
            }))
            .send()?;

        if response.status().is_success() {
            let data: UsgsApiResponse = response.json()?;
            if data.success {
                self.api_key = data.data.and_then(|d| d.results.first().and_then(|r| r.id.clone()));
                println!("  ✓ Logged into USGS EarthExplorer");
                return Ok(true);
            } else {
                let error_msg = data.error_msg.unwrap_or_else(|| "Unknown error".to_string());
                println!("  ✗ USGS login failed: {}", error_msg);
                return Ok(false);
            }
        } else {
            let status = response.status();
            println!("  ✗ USGS API error: {}", status);
            return Ok(false);
        }
    }

    /// Logout from USGS API
    pub fn logout(&mut self) {
        if let Some(api_key) = &self.api_key {
            let url = format!("{}/logout", USGS_API_BASE);
            let _ = self.client
                .post(&url)
                .header("X-Auth-Token", api_key)
                .send();
            self.api_key = None;
        }
    }

    /// Search for Landsat scenes
    pub fn search_scenes(
        &self,
        bbox: &Bounds,
        date_range: &DateWindow,
        max_results: usize,
    ) -> Result<Vec<UsgsScene>> {
        let api_key = self.api_key.as_ref().ok_or_else(|| {
            FetcherError::Auth("USGS API key not set. Call login() first.".to_string())
        })?;

        let url = format!("{}/scene-search", USGS_API_BASE);

        let payload = serde_json::json!({
            "datasetName": "landsat_ot_c2_l2",
            "maxResults": max_results,
            "startingNumber": 1,
            "spatialFilter": {
                "filterType": "mbr",
                "lowerLeft": {
                    "latitude": bbox.south,
                    "longitude": bbox.west
                },
                "upperRight": {
                    "latitude": bbox.north,
                    "longitude": bbox.east
                }
            },
            "temporalFilter": {
                "start": date_range.start.format("%Y-%m-%d").to_string(),
                "end": date_range.end.format("%Y-%m-%d").to_string()
            },
            "acquisitionType": "L1GT"
        });

        let response = self.client
            .post(&url)
            .json(&payload)
            .header("X-Auth-Token", api_key)
            .send()?;

        if response.status().is_success() {
            let data: UsgsApiResponse = response.json()?;
            if data.success {
                let scenes = data.data
                    .and_then(|d| d.results)
                    .unwrap_or_default();
                println!("  Found {} Landsat scenes", scenes.len());
                return Ok(scenes);
            } else {
                let error_msg = data.error_msg.unwrap_or_else(|| "Unknown error".to_string());
                println!("  ✗ USGS search failed: {}", error_msg);
                return Ok(Vec::new());
            }
        } else {
            let status = response.status();
            println!("  ✗ USGS search error: {}", status);
            return Ok(Vec::new());
        }
    }

    /// Download a Landsat scene
    pub fn download_scene(
        &self,
        scene_id: &str,
        output_dir: &Path,
    ) -> Result<Option<PathBuf>> {
        let api_key = self.api_key.as_ref().ok_or_else(|| {
            FetcherError::Auth("USGS API key not set. Call login() first.".to_string())
        })?;

        let url = format!("{}/download", USGS_API_BASE);
        let payload = serde_json::json!({
            "entityId": scene_id,
            "productId": "L2SP"
        });

        let response = self.client
            .post(&url)
            .json(&payload)
            .header("X-Auth-Token", api_key)
            .send()?;

        if response.status().is_success() {
            let data: UsgsApiResponse = response.json()?;
            if data.success {
                let download_url = data.data
                    .and_then(|d| d.available_downloads)
                    .and_then(|downloads| downloads.first())
                    .and_then(|d| d.url.clone());

                if let Some(download_url) = download_url {
                    println!("  Downloading {}...", scene_id);

                    let file_response = self.client.get(&download_url).send()?;
                    if file_response.status().is_success() {
                        let output_dir = output_dir.as_ref();
                        output_dir.create_all()?;
                        let output_file = output_dir.join(format!("{}.tar.gz", scene_id));

                        let mut file = std::fs::File::create(&output_file)?;
                        let mut buffer = [0u8; 8192];
                        let mut total_bytes = 0;

                        loop {
                            let bytes_read = file_response.read(&mut buffer)?;
                            if bytes_read == 0 {
                                break;
                            }
                            file.write_all(&buffer[..bytes_read])?;
                            total_bytes += bytes_read as u64;
                        }

                        println!("  ✓ Downloaded to {}", output_file.display());
                        return Ok(Some(output_file));
                    } else {
                        println!("  ✗ Download failed with status: {}", file_response.status());
                        return Ok(None);
                    }
                } else {
                    println!("  ✗ No download URL found");
                    return Ok(None);
                }
            } else {
                let error_msg = data.error_msg.unwrap_or_else(|| "Unknown error".to_string());
                println!("  ✗ USGS download failed: {}", error_msg);
                return Ok(None);
            }
        } else {
            let status = response.status();
            println!("  ✗ USGS download error: {}", status);
            return Ok(None);
        }
    }
}

/// Sentinel Hub Fetcher for Sentinel-2 data
pub struct SentinelFetcher {
    client_id: Option<String>,
    client_secret: Option<String>,
    access_token: Option<String>,
    client: Client,
}

impl SentinelFetcher {
    /// Create a new Sentinel Hub fetcher
    pub fn new(client_id: Option<String>, client_secret: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client_id,
            client_secret,
            access_token: None,
            client,
        }
    }

    /// Get OAuth access token
    pub fn get_token(&mut self) -> Result<bool> {
        let client_id = self.client_id.as_deref().ok_or_else(|| {
            FetcherError::Auth("Sentinel Hub client ID not provided".to_string())
        })?;
        let client_secret = self.client_secret.as_deref().ok_or_else(|| {
            FetcherError::Auth("Sentinel Hub client secret not provided".to_string())
        })?;

        let url = "https://services.sentinel-hub.com/oauth/token";
        let response = self.client
            .post(url)
            .form(&serde_json::json!({
                "grant_type": "client_credentials",
                "client_id": client_id,
                "client_secret": client_secret
            }))
            .send()?;

        if response.status().is_success() {
            let data: serde_json::Value = response.json()?;
            let access_token = data.get("access_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if let Some(token) = access_token {
                self.access_token = Some(token);
                println!("  ✓ Authenticated with Sentinel Hub");
                return Ok(true);
            } else {
                let error_msg = data.get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| "Unknown error".to_string());
                println!("  ✗ Sentinel Hub auth error: {}", error_msg);
                return Ok(false);
            }
        } else {
            let status = response.status();
            println!("  ✗ Sentinel Hub auth error: {}", status);
            return Ok(false);
        }
    }

    /// Search Sentinel-2 catalog
    pub fn search_catalog(
        &self,
        bbox: &[f64; 4],
        time_range: (DateTime<Utc>, DateTime<Utc>),
        max_results: usize,
    ) -> Result<Vec<SentinelFeature>> {
        let access_token = self.access_token.as_ref().ok_or_else(|| {
            FetcherError::Auth("Sentinel Hub access token not set. Call get_token() first.".to_string())
        })?;

        let url = format!("{}/catalog/collections/sentinel-2-l2a/items", SENTINEL_HUB_BASE);

        let start_date = time_range.0.format("%Y-%m-%d").to_string();
        let end_date = time_range.1.format("%Y-%m-%d").to_string();

        let params = [
            ("bbox", &bbox.join(",")),
            ("datetime", &format!("{}/{}", start_date, end_date)),
            ("limit", &max_results.to_string()),
        ];

        let response = self.client
            .get(&url)
            .query(&params)
            .header("Authorization", format!("Bearer {}", access_token))
            .send()?;

        if response.status().is_success() {
            let data: SentinelHubResponse = response.json()?;
            let features = data.features;
            println!("  Found {} Sentinel-2 scenes", features.len());
            return Ok(features);
        } else {
            let status = response.status();
            println!("  ✗ Sentinel Hub search error: {}", status);
            return Ok(Vec::new());
        }
    }

    /// Download Sentinel-2 tile (simplified)
    pub fn download_tile(
        &self,
        tile_id: &str,
        bands: &[&str],
        output_dir: &Path,
    ) -> Result<Option<PathBuf>> {
        let access_token = self.access_token.as_ref().ok_or_else(|| {
            FetcherError::Auth("Sentinel Hub access token not set. Call get_token() first.".to_string())
        })?;

        println!("  Downloading tile {} with bands {:?}...", tile_id, bands);
        std::thread::sleep(Duration::from_secs(2)); // Simulated download

        let output_dir = output_dir.as_ref();
        output_dir.create_all()?;
        let output_file = output_dir.join(format!("{}.tif", tile_id));

        println!("  ✓ Downloaded {}", tile_id);
        return Ok(Some(output_file));
    }
}

/// Fetch SEAGULL current vector data for drift correction
pub fn fetch_seagull_currents(lat: f64, lon: f64) -> Result<Option<CurrentData>> {
    println!("  Fetching SEAGULL currents for {:.4}, {:.4}...", lat, lon);
    std::thread::sleep(Duration::from_secs(1)); // Simulated API call

    // Return mock current data
    return Ok(Some(CurrentData {
        speed_ms: 0.15,
        direction_deg: 245.0,
        timestamp: Utc::now().naive_utc(),
    }));
}

/// SEAGULL current data structure
#[derive(Debug, Clone)]
pub struct CurrentData {
    pub speed_ms: f64,
    pub direction_deg: f64,
    pub timestamp: NaiveDateTime,
}

/// Main API function for the fetcher
pub fn API(
    data_dir: &Path,
    usgs_user: Option<&str>,
    usgs_pass: Option<&str>,
    sentinel_id: Option<&str>,
    sentinel_secret: Option<&str>,
    ice_break: bool,
    low_silt: bool,
) -> Result<()> {
    println!("================================================================================");
    println!("CESAROPS FETCHER - Satellite Data Download");
    println!("================================================================================");
    println!();

    // Initialize USGS fetcher
    let mut usgs_fetcher = UsgsFetcher::new(
        usgs_user.map(|s| s.to_string()),
        usgs_pass.map(|s| s.to_string()),
    );

    // Initialize Sentinel Hub fetcher
    let mut sentinel_fetcher = SentinelFetcher::new(
        sentinel_id.map(|s| s.to_string()),
        sentinel_secret.map(|s| s.to_string()),
    );

    // Login to USGS
    if usgs
