//! Download-source preflight checks for satellite missions.

use reqwest::Client;
use serde::Serialize;
use std::{collections::HashSet, time::Instant};

#[derive(Debug, Clone)]
pub struct SourceDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub url: &'static str,
    pub sensors: &'static [&'static str],
    pub auth_env_any: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceProbe {
    pub id: String,
    pub label: String,
    pub url: String,
    pub selected: bool,
    pub auth_required: bool,
    pub auth_present: bool,
    pub reachable: bool,
    pub status_code: Option<u16>,
    pub elapsed_ms: u128,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadPreflightReport {
    pub sensors: Vec<String>,
    pub strict_auth: bool,
    pub ok: bool,
    pub reachable_selected: usize,
    pub auth_missing_selected: usize,
    pub unreachable_selected: usize,
    pub probes: Vec<SourceProbe>,
}

fn source_catalog() -> Vec<SourceDefinition> {
    vec![
        SourceDefinition {
            id: "stac_e84",
            label: "Element84 STAC (Sentinel-2)",
            url: "https://earth-search.aws.element84.com/v1",
            sensors: &[
                "all",
                "aws",
                "stac",
                "optical",
                "optical_aws",
                "sentinel2",
                "landsat",
                "landsat_aws",
                "sentinel2_aws",
            ],
            auth_env_any: &[],
        },
        SourceDefinition {
            id: "copernicus_odata",
            label: "Copernicus Data Space OData",
            url: "https://catalogue.dataspace.copernicus.eu/odata/v1/Products",
            sensors: &["copernicus", "optical_cdse", "sar", "sentinel1"],
            auth_env_any: &["COPERNICUS_USER", "COPERNICUS_USERNAME"],
        },
        SourceDefinition {
            id: "asf_search",
            label: "ASF Search API",
            url: "https://api.daac.asf.alaska.edu/services/search/param",
            sensors: &["all", "sar", "sentinel1"],
            auth_env_any: &[],
        },
        SourceDefinition {
            id: "hyp3_api",
            label: "ASF HyP3 API",
            url: "https://hyp3-api.asf.alaska.edu/jobs",
            sensors: &["all", "sar", "sentinel1", "hyp3"],
            auth_env_any: &["EARTHDATA_TOKEN", "NASA_EARTHDATA_TOKEN"],
        },
        SourceDefinition {
            id: "cmr_granules",
            label: "NASA CMR Granules",
            url: "https://cmr.earthdata.nasa.gov/search/granules.json",
            sensors: &[
                "all", "hls", "modis", "viirs", "icesat2", "swot", "thermal", "podaac",
            ],
            auth_env_any: &[],
        },
        SourceDefinition {
            id: "usgs_m2m",
            label: "USGS EarthExplorer M2M",
            url: "https://m2m.cr.usgs.gov/api/api/json/stable/",
            sensors: &["all", "usgs", "thermal"],
            auth_env_any: &["USGS_API_KEY"],
        },
        SourceDefinition {
            id: "noaa_glsea",
            label: "NOAA CoastWatch GLSEA",
            url: "https://coastwatch.glerl.noaa.gov/erddap/griddap/GLSEA_GCS.nc",
            sensors: &["all", "sst", "thermal"],
            auth_env_any: &[],
        },
        SourceDefinition {
            id: "usgs_3dep",
            label: "USGS 3DEP TNM API",
            url: "https://tnmapi.cr.usgs.gov/api/products",
            sensors: &["all", "lidar", "dem", "bathymetry"],
            auth_env_any: &[],
        },
    ]
}

fn parse_sensor_set(raw: &str) -> HashSet<String> {
    if raw.trim().is_empty() {
        return ["all".to_string()].into_iter().collect();
    }
    raw.split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn source_selected(source: &SourceDefinition, wanted: &HashSet<String>) -> bool {
    if wanted.contains("all") {
        return true;
    }
    source.sensors.iter().any(|s| wanted.contains(&s.to_string()))
}

fn auth_present(source: &SourceDefinition) -> bool {
    if source.auth_env_any.is_empty() {
        return true;
    }
    source.auth_env_any.iter().any(|k| {
        std::env::var(k)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    })
}

pub async fn preflight_sources(
    client: &Client,
    sensors_csv: &str,
    strict_auth: bool,
) -> DownloadPreflightReport {
    let wanted = parse_sensor_set(sensors_csv);
    let sensors: Vec<String> = wanted.iter().cloned().collect();
    let mut probes: Vec<SourceProbe> = Vec::new();
    let mut reachable_selected = 0usize;
    let mut auth_missing_selected = 0usize;
    let mut unreachable_selected = 0usize;

    for src in source_catalog() {
        let selected = source_selected(&src, &wanted);
        let auth_ok = auth_present(&src);
        let auth_required = !src.auth_env_any.is_empty();
        let t0 = Instant::now();

        let mut reachable = false;
        let mut code = None;
        let mut message = String::new();

        if selected {
            if strict_auth && auth_required && !auth_ok {
                message = format!(
                    "missing auth env (need one of: {})",
                    src.auth_env_any.join(", ")
                );
                auth_missing_selected += 1;
            } else {
                match client.get(src.url).send().await {
                    Ok(resp) => {
                        code = Some(resp.status().as_u16());
                        if resp.status().is_success() || resp.status().is_redirection() {
                            reachable = true;
                            reachable_selected += 1;
                            message = "reachable".to_string();
                        } else {
                            message = format!("http {}", resp.status());
                            unreachable_selected += 1;
                        }
                    }
                    Err(e) => {
                        message = e.to_string();
                        unreachable_selected += 1;
                    }
                }
            }
        }

        probes.push(SourceProbe {
            id: src.id.to_string(),
            label: src.label.to_string(),
            url: src.url.to_string(),
            selected,
            auth_required,
            auth_present: auth_ok,
            reachable,
            status_code: code,
            elapsed_ms: t0.elapsed().as_millis(),
            message,
        });
    }

    let ok = reachable_selected > 0 && auth_missing_selected == 0;

    DownloadPreflightReport {
        sensors,
        strict_auth,
        ok,
        reachable_selected,
        auth_missing_selected,
        unreachable_selected,
        probes,
    }
}
