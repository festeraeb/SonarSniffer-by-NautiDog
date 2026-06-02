//! Rust replacement for `cmr_search.py`.

use clap::Parser;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

const CMR_BASE: &str = "https://cmr.earthdata.nasa.gov/search";

#[derive(Parser, Debug)]
#[command(name = "cmr-search", about = "Query NASA CMR for satellite granules")]
struct Args {
    /// lat_min,lon_min,lat_max,lon_max
    #[arg(long)]
    bbox: String,
    /// YYYY-MM-DD
    #[arg(long)]
    start: String,
    /// YYYY-MM-DD
    #[arg(long)]
    end: String,
    /// comma-separated sensor keys: hls,sar,swot,atl13,modis
    #[arg(long, default_value = "hls")]
    sensor: String,
    #[arg(long, default_value_t = 20)]
    max_results: usize,
}

#[derive(Debug, Clone)]
struct CmrCtx {
    client: reqwest::Client,
    token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SearchResult {
    granules: Vec<Value>,
    count: usize,
    errors: Vec<Value>,
    bbox: [f64; 4],
    start: String,
    end: String,
    sensors_queried: Vec<String>,
    auth: String,
}

fn concept_ids() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("hls_l30", "C2021957657-LPCLOUD"),
        ("hls_s30", "C2021957295-LPCLOUD"),
        ("sar", "C1214354438-ASF"),
        ("swot", "C2799438271-POCLOUD"),
        ("atl13", "C2144800918-NSIDC_ECS"),
        ("modis_lst", "C1621091311-LPDAAC_ECS"),
    ])
}

fn read_token_from_env_file(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let raw = std::fs::read_to_string(path).ok()?;
    for line in raw.lines() {
        let s = line.trim();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = s.split_once('=') {
            if k.trim() == "EARTHDATA_TOKEN" {
                let token = v.trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }
    }
    None
}

fn load_earthdata_token() -> Option<String> {
    if let Ok(v) = std::env::var("EARTHDATA_TOKEN") {
        if !v.trim().is_empty() {
            return Some(v);
        }
    }

    let candidates = [Path::new(".env"), Path::new("../.env")];
    for c in candidates {
        if let Some(v) = read_token_from_env_file(c) {
            return Some(v);
        }
    }
    None
}

fn parse_bbox(s: &str) -> anyhow::Result<[f64; 4]> {
    let parts: Vec<f64> = s
        .split(',')
        .map(|x| x.trim().parse::<f64>())
        .collect::<Result<Vec<_>, _>>()?;
    if parts.len() != 4 {
        anyhow::bail!("--bbox must be lat_min,lon_min,lat_max,lon_max");
    }
    Ok([parts[0], parts[1], parts[2], parts[3]])
}

fn first_download_url(links: &[Value]) -> String {
    const DATA_REL: &str = "http://esipfed.org/ns/fedsearch/1.1/data#";
    for l in links {
        let rel = l.get("rel").and_then(Value::as_str).unwrap_or("");
        let href = l.get("href").and_then(Value::as_str).unwrap_or("");
        if rel == DATA_REL && href.starts_with("http") {
            return href.to_string();
        }
    }
    String::new()
}

async fn cmr_query(
    ctx: &CmrCtx,
    concept_id: &str,
    bbox: [f64; 4],
    start: &str,
    end: &str,
    max_results: usize,
) -> Vec<Value> {
    let [lat_min, lon_min, lat_max, lon_max] = bbox;
    let cmr_bbox = format!("{lon_min},{lat_min},{lon_max},{lat_max}");

    let mut req = ctx.client.get(format!("{CMR_BASE}/granules.json")).query(&[
        ("concept_id", concept_id),
        ("temporal", &format!("{start}T00:00:00Z,{end}T23:59:59Z")),
        ("bounding_box", &cmr_bbox),
        ("page_size", &max_results.min(200).to_string()),
        ("sort_key", "-start_date"),
    ]);

    if let Some(t) = &ctx.token {
        req = req.header(AUTHORIZATION, format!("Bearer {t}"));
    }

    let res = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return vec![serde_json::json!({"_error": format!("CMR unreachable: {e}")})];
        }
    };

    if !res.status().is_success() {
        let code = res.status().as_u16();
        let body = res.text().await.unwrap_or_default();
        let body_short = body.chars().take(200).collect::<String>();
        return vec![serde_json::json!({"_error": format!("CMR HTTP {code}: {body_short}")})];
    }

    match res.json::<Value>().await {
        Ok(v) => v
            .get("feed")
            .and_then(|f| f.get("entry"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        Err(e) => vec![serde_json::json!({"_error": e.to_string()})],
    }
}

async fn query_hls(
    ctx: &CmrCtx,
    cids: &HashMap<&'static str, &'static str>,
    bbox: [f64; 4],
    start: &str,
    end: &str,
    max_results: usize,
) -> (Vec<Value>, Vec<Value>) {
    let mut granules = Vec::new();
    let mut errors = Vec::new();

    for key in ["hls_l30", "hls_s30"] {
        let label = if key == "hls_l30" { "HLS-Landsat" } else { "HLS-Sentinel" };
        let Some(cid) = cids.get(key) else {
            continue;
        };
        for g in cmr_query(ctx, cid, bbox, start, end, max_results).await {
            if let Some(err) = g.get("_error").and_then(Value::as_str) {
                errors.push(serde_json::json!({"sensor": label, "error": err}));
            } else {
                let links = g.get("links").and_then(Value::as_array).cloned().unwrap_or_default();
                granules.push(serde_json::json!({
                    "id": g.get("id").and_then(Value::as_str).unwrap_or(""),
                    "sensor": label,
                    "title": g.get("title").and_then(Value::as_str).unwrap_or(""),
                    "time_start": g.get("time_start").and_then(Value::as_str).unwrap_or(""),
                    "cloud_cover": g.get("cloud_cover").cloned().unwrap_or(Value::Null),
                    "download_url": first_download_url(&links),
                }));
            }
        }
    }

    (granules, errors)
}

async fn query_generic(
    ctx: &CmrCtx,
    cids: &HashMap<&'static str, &'static str>,
    key: &str,
    label: &str,
    bbox: [f64; 4],
    start: &str,
    end: &str,
    max_results: usize,
    polarization: Option<&str>,
) -> (Vec<Value>, Vec<Value>) {
    let mut granules = Vec::new();
    let mut errors = Vec::new();

    let Some(cid) = cids.get(key) else {
        return (granules, vec![serde_json::json!({"sensor": label, "error": "missing concept id"})]);
    };

    for g in cmr_query(ctx, cid, bbox, start, end, max_results).await {
        if let Some(err) = g.get("_error").and_then(Value::as_str) {
            errors.push(serde_json::json!({"sensor": label, "error": err}));
        } else {
            let links = g.get("links").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut obj = serde_json::Map::new();
            obj.insert("id".into(), Value::String(g.get("id").and_then(Value::as_str).unwrap_or("").to_string()));
            obj.insert("sensor".into(), Value::String(label.to_string()));
            obj.insert("title".into(), Value::String(g.get("title").and_then(Value::as_str).unwrap_or("").to_string()));
            obj.insert("time_start".into(), Value::String(g.get("time_start").and_then(Value::as_str).unwrap_or("").to_string()));
            if let Some(pol) = polarization {
                obj.insert("polarization".into(), Value::String(pol.to_string()));
            }
            obj.insert("download_url".into(), Value::String(first_download_url(&links)));
            granules.push(Value::Object(obj));
        }
    }

    (granules, errors)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let bbox = parse_bbox(&args.bbox)?;

    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let token = load_earthdata_token();
    let ctx = CmrCtx { client, token: token.clone() };
    let cids = concept_ids();

    let mut all_granules = Vec::<Value>::new();
    let mut all_errors = Vec::<Value>::new();
    let mut seen = HashSet::<String>::new();

    for s in args.sensor.split(',').map(|v| v.trim().to_lowercase()) {
        if s.is_empty() || seen.contains(&s) {
            continue;
        }

        let (g, e) = match s.as_str() {
            "hls" | "hls_l30" | "hls_s30" => {
                query_hls(&ctx, &cids, bbox, &args.start, &args.end, args.max_results).await
            }
            "sar" => {
                query_generic(
                    &ctx,
                    &cids,
                    "sar",
                    "SAR-Sentinel1",
                    bbox,
                    &args.start,
                    &args.end,
                    args.max_results,
                    Some("VV+VH"),
                )
                .await
            }
            "swot" => {
                query_generic(&ctx, &cids, "swot", "SWOT", bbox, &args.start, &args.end, args.max_results, None).await
            }
            "atl13" => {
                query_generic(&ctx, &cids, "atl13", "ICESat2", bbox, &args.start, &args.end, args.max_results, None).await
            }
            "modis" => {
                query_generic(
                    &ctx,
                    &cids,
                    "modis_lst",
                    "MODIS-LST",
                    bbox,
                    &args.start,
                    &args.end,
                    args.max_results,
                    None,
                )
                .await
            }
            other => {
                all_errors.push(serde_json::json!({"sensor": other, "error": "unknown sensor key"}));
                continue;
            }
        };

        seen.insert(s);
        all_granules.extend(g);
        all_errors.extend(e);
    }

    let mut sensors_queried: Vec<String> = seen.into_iter().collect();
    sensors_queried.sort();

    let result = SearchResult {
        count: all_granules.len(),
        granules: all_granules,
        errors: all_errors,
        bbox,
        start: args.start,
        end: args.end,
        sensors_queried,
        auth: if token.is_some() {
            "token".to_string()
        } else {
            "none (public only)".to_string()
        },
    };

    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
