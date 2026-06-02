//! STAC scene discovery — Element84 Earth Search v1 + NASA CMR.
//!
//! Ports `_stac_search()` / `_stac_scenes()` from:
//!   wh2k_sentinel_wreck_targeting.py, wh2k_sentinel_optical_poc.py,
//!   temporal_stack_engine.py, nasa_earthdata_client.py

use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

pub const STAC_URL: &str = "https://earth-search.aws.element84.com/v1";
pub const S2_COLLECTION: &str = "sentinel-2-l2a";
pub const CMR_URL: &str = "https://cmr.earthdata.nasa.gov/search/granules.json";

// ── Scene type ────────────────────────────────────────────────────────────────

/// Minimal STAC feature representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub datetime: NaiveDate,
    pub cloud_cover: f64,
    /// STAC asset hrefs keyed by band name (e.g. "B03", "B08", "B04", "B11")
    pub assets: std::collections::HashMap<String, String>,
}

impl Scene {
    pub fn asset_href(&self, band: &str) -> Option<&str> {
        // Try exact key, then lowercase, then B0X aliases
        let aliases: &[&str] = &[
            band,
            &band.to_lowercase(),
            &format!("B0{}", band.chars().last().unwrap_or('0')),
            &format!("b0{}", band.chars().last().unwrap_or('0')),
        ];
        for alias in aliases {
            if let Some(href) = self.assets.get(*alias) {
                return Some(href.as_str());
            }
        }
        None
    }
}

// ── STAC raw JSON helpers ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct StacFeature {
    id: String,
    properties: serde_json::Value,
    assets: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct StacCollection {
    features: Vec<StacFeature>,
}

fn parse_feature(f: StacFeature) -> Option<Scene> {
    let props = &f.properties;
    let dt_str = props.get("datetime")?.as_str()?.get(..10)?;
    let datetime = NaiveDate::parse_from_str(dt_str, "%Y-%m-%d").ok()?;
    let cloud_cover = props
        .get("eo:cloud_cover")
        .and_then(|v| v.as_f64())
        .unwrap_or(100.0);

    let mut assets = std::collections::HashMap::new();
    if let Some(obj) = f.assets.as_object() {
        for (k, v) in obj {
            if let Some(href) = v.get("href").and_then(|h| h.as_str()) {
                assets.insert(k.clone(), href.to_string());
            }
        }
    }

    Some(Scene { id: f.id, datetime, cloud_cover, assets })
}

// ── Public search API ─────────────────────────────────────────────────────────

/// Search parameters for a STAC query.
pub struct StacQuery<'a> {
    /// [west, south, east, north]
    pub bbox: [f64; 4],
    pub date_start: NaiveDate,
    pub date_end: NaiveDate,
    pub max_cloud: f64,
    /// If non-empty, keep only scenes whose month is in this list.
    pub month_filter: &'a [u32],
    pub limit: usize,
}

/// Search Element84 Earth Search STAC for Sentinel-2 L2A scenes.
///
/// Returns scenes sorted by ascending cloud cover, filtered by month.
pub async fn search_scenes(client: &Client, q: &StacQuery<'_>) -> Result<Vec<Scene>> {
    let url = format!("{STAC_URL}/collections/{S2_COLLECTION}/items");
    let bbox_str = format!(
        "{},{},{},{}",
        q.bbox[0], q.bbox[1], q.bbox[2], q.bbox[3]
    );
    let dt_range = format!(
        "{}T00:00:00Z/{}T23:59:59Z",
        q.date_start, q.date_end
    );
    let cloud_filter = format!("eo:cloud_cover <= {}", q.max_cloud);

    let resp = client
        .get(&url)
        .query(&[
            ("bbox", bbox_str.as_str()),
            ("datetime", dt_range.as_str()),
            ("limit", &q.limit.to_string()),
            ("filter", &cloud_filter),
            ("filter-lang", "cql2-text"),
        ])
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        warn!("STAC search returned {}: {}", status, &text[..text.len().min(300)]);
        anyhow::bail!("STAC search failed with status {}", status);
    }

    let collection: StacCollection = resp.json().await?;
    debug!("STAC returned {} raw features", collection.features.len());

    let mut scenes: Vec<Scene> = collection
        .features
        .into_iter()
        .filter_map(parse_feature)
        .filter(|s| {
            if q.month_filter.is_empty() {
                true
            } else {
                q.month_filter.contains(&s.datetime.month())
            }
        })
        .collect();

    scenes.sort_by(|a, b| a.cloud_cover.partial_cmp(&b.cloud_cover).unwrap());
    Ok(scenes)
}

/// POST-based search (used by temporal_stack_engine — avoids query-string length limits).
pub async fn search_scenes_post(client: &Client, q: &StacQuery<'_>) -> Result<Vec<Scene>> {
    let url = format!("{STAC_URL}/search");
    let body = serde_json::json!({
        "collections": [S2_COLLECTION],
        "bbox": q.bbox,
        "datetime": format!(
            "{}T00:00:00Z/{}T23:59:59Z",
            q.date_start, q.date_end
        ),
        "limit": q.limit,
    });

    let resp = client.post(&url).json(&body).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        warn!("STAC POST search returned {}", status);
        anyhow::bail!("STAC POST search failed: {}", status);
    }

    let collection: StacCollection = resp.json().await?;
    let mut scenes: Vec<Scene> = collection
        .features
        .into_iter()
        .filter_map(parse_feature)
        .filter(|s| {
            s.cloud_cover <= q.max_cloud
                && (q.month_filter.is_empty()
                    || q.month_filter.contains(&s.datetime.month()))
        })
        .collect();

    scenes.sort_by(|a, b| a.cloud_cover.partial_cmp(&b.cloud_cover).unwrap());
    Ok(scenes)
}

// ── NASA CMR / Earthdata ──────────────────────────────────────────────────────

/// Search NASA CMR for granules by collection concept ID.
/// Token read from `NASA_EARTHDATA_TOKEN` env var.
pub async fn search_nasa_granules(
    client: &Client,
    collection_concept_id: &str,
    bbox: [f64; 4],
    start_date: NaiveDate,
    end_date: NaiveDate,
    page_size: usize,
) -> Result<Vec<serde_json::Value>> {
    let token = std::env::var("NASA_EARTHDATA_TOKEN").ok();
    let bbox_str = format!("{},{},{},{}", bbox[0], bbox[1], bbox[2], bbox[3]);
    let temporal = format!("{},{}", start_date, end_date);

    let mut req = client
        .get(CMR_URL)
        .query(&[
            ("collection_concept_id", collection_concept_id),
            ("bounding_box", &bbox_str),
            ("temporal", &temporal),
            ("page_size", &page_size.to_string()),
        ]);

    if let Some(tok) = &token {
        req = req.header("Authorization", format!("Bearer {tok}"));
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("CMR search failed: {}", resp.status());
    }
    let body: serde_json::Value = resp.json().await?;
    Ok(body
        .get("feed")
        .and_then(|f| f.get("entry"))
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default())
}

// ── OPERA DSWx ────────────────────────────────────────────────────────────────

/// OPERA DSWx-HLS collection concept ID (POCLOUD).
///
/// Mirrors `apply_opera_dswx.py::OPERA_DSWX_COLLECTION_ID`.
pub const OPERA_DSWX_COLLECTION_ID: &str = "C2617126679-POCLOUD";

/// Extract candidate download URLs from a CMR granule entry.
///
/// Ports `EarthdataClient.extract_download_urls` (nasa_earthdata_client.py):
/// prefers links whose `rel` mentions data/browse/http(s) or whose `type` is a
/// known download content-type, then falls back to known file extensions.
pub fn extract_download_urls(granule_entry: &serde_json::Value) -> Vec<String> {
    let mut urls = Vec::new();
    let links = match granule_entry.get("links").and_then(|l| l.as_array()) {
        Some(l) => l,
        None => return urls,
    };
    for link in links {
        let rel = link.get("rel").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        let content_type = link.get("type").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        let href = match link.get("href").and_then(|v| v.as_str()) {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };

        let rel_ok = rel.contains("data")
            || rel.contains("browse")
            || rel == "http"
            || rel == "https";
        let type_ok = matches!(
            content_type.as_str(),
            "application/octet-stream"
                | "application/x-hdf"
                | "application/x-netcdf"
                | "image/tiff"
                | "image/png"
        );
        if rel_ok || type_ok {
            urls.push(href.to_string());
            continue;
        }
        let lower = href.to_lowercase();
        if lower.ends_with(".tif")
            || lower.ends_with(".tiff")
            || lower.ends_with(".nc")
            || lower.ends_with(".hdf")
            || lower.ends_with(".zip")
            || lower.ends_with(".tar.gz")
        {
            urls.push(href.to_string());
        }
    }
    urls
}

/// Result of an OPERA DSWx fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperaDswxResult {
    pub collection_concept_id: String,
    pub n_granules: usize,
    /// Download URLs of the first granule (the one `fetch_opera_dswx` downloads).
    pub download_urls: Vec<String>,
    /// Path the file was written to (None when the live download is gated off).
    pub output_path: Option<String>,
    /// True when the live download actually ran.
    pub downloaded: bool,
    pub note: String,
}

/// Fetch OPERA DSWx data for a bounding box + time range.
///
/// Ports `apply_opera_dswx.py::fetch_opera_dswx`:
///   1. CMR granule search for collection C2617126679-POCLOUD,
///   2. take the first granule and extract download URLs,
///   3. download the first URL to `<output_dir>/opera_dswx_data.nc`.
///
/// `bbox` is STAC order `[west, south, east, north]`.  Auth is read from
/// `NASA_EARTHDATA_TOKEN` (see [`search_nasa_granules`]).
///
/// `live_download` gates step 3: when `false` (or no token is available) the
/// request building + granule parsing run and the URLs are returned, but the
/// (potentially large, auth-required) file download is skipped.  This lets the
/// fetch be exercised offline / in CI without network credentials.
pub async fn fetch_opera_dswx(
    client: &Client,
    bbox: [f64; 4],
    start_date: NaiveDate,
    end_date: NaiveDate,
    output_dir: &std::path::Path,
    live_download: bool,
) -> Result<OperaDswxResult> {
    let granules = search_nasa_granules(
        client,
        OPERA_DSWX_COLLECTION_ID,
        bbox,
        start_date,
        end_date,
        200,
    )
    .await?;

    if granules.is_empty() {
        anyhow::bail!("No granules found for the specified parameters");
    }

    let urls = extract_download_urls(&granules[0]);
    if urls.is_empty() {
        anyhow::bail!("No download URLs found for the first granule");
    }

    let token_present = std::env::var("NASA_EARTHDATA_TOKEN")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);

    // Gate the live download: requires opt-in AND a token.
    if !live_download || !token_present {
        return Ok(OperaDswxResult {
            collection_concept_id: OPERA_DSWX_COLLECTION_ID.to_string(),
            n_granules: granules.len(),
            download_urls: urls,
            output_path: None,
            downloaded: false,
            note: if !token_present {
                "download skipped: NASA_EARTHDATA_TOKEN not set (request + parse only)".into()
            } else {
                "download gated off (live_download=false); request + parse only".into()
            },
        });
    }

    std::fs::create_dir_all(output_dir)?;
    let output_path = output_dir.join("opera_dswx_data.nc");
    let token = std::env::var("NASA_EARTHDATA_TOKEN").ok();
    let mut req = client.get(&urls[0]);
    if let Some(tok) = &token {
        req = req.header("Authorization", format!("Bearer {tok}"));
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("OPERA download failed: {}", resp.status());
    }
    let bytes = resp.bytes().await?;
    std::fs::write(&output_path, &bytes)?;

    Ok(OperaDswxResult {
        collection_concept_id: OPERA_DSWX_COLLECTION_ID.to_string(),
        n_granules: granules.len(),
        download_urls: urls,
        output_path: Some(output_path.to_string_lossy().to_string()),
        downloaded: true,
        note: "downloaded first granule".into(),
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_download_urls_prefers_data_rel() {
        let granule = serde_json::json!({
            "links": [
                { "rel": "http://esipfed.org/ns/fedsearch/1.1/data#", "href": "https://x/file.nc" },
                { "rel": "describedby", "href": "https://x/meta.xml" },
                { "rel": "via", "type": "image/tiff", "href": "https://x/browse.tif" }
            ]
        });
        let urls = extract_download_urls(&granule);
        // rel contains "data" → included.
        assert!(urls.contains(&"https://x/file.nc".to_string()));
        // image/tiff content-type → included.
        assert!(urls.contains(&"https://x/browse.tif".to_string()));
        // rel "describedby", no download type, .xml extension → excluded
        // (matches Python: no data/browse rel, no known type/extension).
        assert!(!urls.contains(&"https://x/meta.xml".to_string()));
    }

    #[test]
    fn extract_download_urls_metadata_rel_matches_substring() {
        // Faithful to Python's `"data" in rel`: "metadata" contains "data", so a
        // metadata link IS picked up (documents the substring behaviour).
        let granule = serde_json::json!({
            "links": [
                { "rel": "http://esipfed.org/ns/fedsearch/1.1/metadata#", "href": "https://x/m.json" }
            ]
        });
        let urls = extract_download_urls(&granule);
        assert!(urls.contains(&"https://x/m.json".to_string()));
    }

    #[test]
    fn extract_download_urls_handles_missing_links() {
        let granule = serde_json::json!({ "title": "no links here" });
        assert!(extract_download_urls(&granule).is_empty());
    }
}
