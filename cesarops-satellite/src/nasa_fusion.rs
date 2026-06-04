//! NASA multi-sensor fusion scoring.
//!
//! Ports `nasa_fusion_test.py`:
//!   `FusionScorer.score_clusters` → [`FusionScorer::score_clusters`]
//!       (average of swot_score + ecostress_score + opera_score)
//!   `FusionScorer.score_to_geojson` → [`FusionScorer::score_to_geojson`]
//!
//! The Python `fetch_swot_data` / `fetch_ecostress_data` / `fetch_opera_data`
//! helpers are placeholders that return fixed neutral scores.  Here:
//!   * OPERA is wired to the real OPERA DSWx fetch in `stac.rs`
//!     (`fetch_opera_dswx`); a successful granule search yields a positive
//!     score, otherwise a neutral one.
//!   * SWOT and ECOSTRESS have no real client in this crate yet, so they remain
//!     documented TODO stubs returning a neutral score (see [`fetch_swot_score`]
//!     / [`fetch_ecostress_score`]).

use crate::{stac::fetch_opera_dswx, types::SarCluster};
use chrono::NaiveDate;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::debug;

/// Neutral per-sensor score used when a live client is unavailable.
pub const NEUTRAL_SENSOR_SCORE: f64 = 0.5;

// NASA CMR Collection Concept IDs (PO.DAAC / LP DAAC).
/// SWOT L2 High-Resolution Raster (inland water surface elevation).
pub const SWOT_COLLECTION_ID: &str = "C2758130541-POCLOUD";
/// ECOSTRESS L2T Land Surface Temperature & Emissivity (Cloud-Optimized GeoTIFF).
pub const ECOSTRESS_COLLECTION_ID: &str = "C2076090826-LPCLOUD";
/// ICESat-2 ATL03 Global Geolocated Photon Data V006.
pub const ICESAT2_ATL03_COLLECTION_ID: &str = "C2592541243-NSIDC_ECS";

/// SWOT score — coverage-aware, degrades to NEUTRAL on no data / sparse passes.
///
/// SWOT's narrow swath + 21-day repeat means MOST queries will return no
/// coverage (expected; sparse ≠ negative). Granule presence → 0.6; when full
/// COG analysis is wired it can rise to 0.9 on WSE local z-anomaly.
pub async fn fetch_swot_score(client: &Client, bbox: [f64; 4], start: NaiveDate, end: NaiveDate) -> f64 {
    match crate::stac::search_nasa_granules(client, SWOT_COLLECTION_ID, bbox, start, end, 5).await {
        Ok(granules) if !granules.is_empty() => {
            debug!("SWOT: {} granule(s) found → base coverage score 0.6", granules.len());
            0.6 // Coverage confirmed; upgrade to z-anomaly scoring when COG fetch wired.
        }
        Ok(_) => NEUTRAL_SENSOR_SCORE,
        Err(e) => {
            debug!("SWOT CMR query failed ({e}); returning neutral");
            NEUTRAL_SENSOR_SCORE
        }
    }
}

/// ECOSTRESS thermal score — coverage-aware, day/night tagged.
///
/// ECOSTRESS (ISS orbit, ~70 m, irregular revisit) provides an independent
/// thermal radiometer for cross-sensor thermal confirmation with Landsat TIRS.
/// Granule presence → 0.6; full COG + annular z-score can rise to 0.95.
pub async fn fetch_ecostress_score(client: &Client, bbox: [f64; 4], start: NaiveDate, end: NaiveDate) -> f64 {
    match crate::stac::search_nasa_granules(client, ECOSTRESS_COLLECTION_ID, bbox, start, end, 5).await {
        Ok(granules) if !granules.is_empty() => {
            debug!("ECOSTRESS: {} granule(s) found → base coverage score 0.6", granules.len());
            // TODO: download COG tile → window → annular LST z-score → 0.6..0.95.
            // Also extract overpass hour for day/night tag (thermal_regime hint).
            0.6
        }
        Ok(_) => NEUTRAL_SENSOR_SCORE,
        Err(e) => {
            debug!("ECOSTRESS CMR query failed ({e}); returning neutral");
            NEUTRAL_SENSOR_SCORE
        }
    }
}

/// ICESat-2 ATL03 photon-cloud score — coverage-aware.
///
/// ATL03 raw photon returns can penetrate deeper than ATL13 processed surface.
/// Presence → 0.6; with token + h_ph download, surface roughness + density
/// anomaly can raise to 0.95. This is the highest-value altimetry sensor.
pub async fn fetch_icesat2_score(client: &Client, bbox: [f64; 4], start: NaiveDate, end: NaiveDate) -> f64 {
    match crate::stac::search_nasa_granules(client, ICESAT2_ATL03_COLLECTION_ID, bbox, start, end, 5).await {
        Ok(granules) if !granules.is_empty() => {
            debug!("ICESat-2 ATL03: {} granule(s) found → base coverage score 0.6", granules.len());
            0.6
        }
        Ok(_) => NEUTRAL_SENSOR_SCORE,
        Err(e) => {
            debug!("ICESat-2 CMR query failed ({e}); returning neutral");
            NEUTRAL_SENSOR_SCORE
        }
    }
}

/// Per-cluster breakdown of the three sensor sub-scores plus the fused average.
#[derive(Debug, Clone)]
pub struct SensorScores {
    pub swot_score: f64,
    pub ecostress_score: f64,
    pub opera_score: f64,
    pub fusion_score: f64,
}

/// TODO stub: SWOT (Surface Water and Ocean Topography) score.
///
/// Mirrors `nasa_fusion_test.py::fetch_swot_data` (a placeholder returning
/// OPERA DSWx score wired to the real granule search in `stac.rs`.
///
/// Mirrors `nasa_fusion_test.py::fetch_opera_data` but uses a live CMR search:
/// if granules are found for the bbox/time-range the cluster sits in a region
/// with OPERA surface-water coverage → score 0.9 (the Python placeholder
/// value); otherwise the neutral score.  The (auth-gated) file download is left
/// off — only the granule search runs.
async fn fetch_opera_score(
    client: &Client,
    bbox: [f64; 4],
    start: NaiveDate,
    end: NaiveDate,
) -> f64 {
    match fetch_opera_dswx(client, bbox, start, end, std::path::Path::new("."), false).await {
        Ok(r) if r.n_granules > 0 => 0.9,
        Ok(_) => NEUTRAL_SENSOR_SCORE,
        Err(e) => {
            debug!("OPERA score fetch failed ({e}); using neutral score");
            NEUTRAL_SENSOR_SCORE
        }
    }
}

/// Multi-sensor fusion scorer.
pub struct FusionScorer;

impl FusionScorer {
    pub fn new() -> Self {
        Self
    }

    /// Score a set of SAR clusters by averaging SWOT + ECOSTRESS + OPERA scores.
    ///
    /// Ports `FusionScorer.score_clusters`: for each cluster a ±0.1° bbox is
    /// built around its centroid and the three sensor scores are averaged.
    /// Returns one fused score per cluster, in input order.
    ///
    /// `start` / `end` are the temporal window for the OPERA granule search
    /// (mirrors the Python `start`/`end` arguments).
    pub async fn score_clusters(
        &self,
        client: &Client,
        clusters: &[SarCluster],
        start: NaiveDate,
        end: NaiveDate,
    ) -> Vec<f64> {
        let mut scores = Vec::with_capacity(clusters.len());
        for cluster in clusters {
            let bbox = [
                cluster.lon - 0.1,
                cluster.lat - 0.1,
                cluster.lon + 0.1,
                cluster.lat + 0.1,
            ];
            let swot = fetch_swot_score(client, bbox, start, end).await;
            let ecostress = fetch_ecostress_score(client, bbox, start, end).await;
            let opera = fetch_opera_score(client, bbox, start, end).await;
            scores.push((swot + ecostress + opera) / 3.0);
        }
        scores
    }

    /// Detailed per-cluster sensor breakdown (same averaging as
    /// [`score_clusters`], but exposes the sub-scores).  Useful for reports.
    pub async fn score_clusters_detailed(
        &self,
        client: &Client,
        clusters: &[SarCluster],
        start: NaiveDate,
        end: NaiveDate,
    ) -> Vec<SensorScores> {
        let mut out = Vec::with_capacity(clusters.len());
        for cluster in clusters {
            let bbox = [
                cluster.lon - 0.1,
                cluster.lat - 0.1,
                cluster.lon + 0.1,
                cluster.lat + 0.1,
            ];
            let swot = fetch_swot_score(client, bbox, start, end).await;
            let ecostress = fetch_ecostress_score(client, bbox, start, end).await;
            let opera = fetch_opera_score(client, bbox, start, end).await;
            out.push(SensorScores {
                swot_score: swot,
                ecostress_score: ecostress,
                opera_score: opera,
                fusion_score: (swot + ecostress + opera) / 3.0,
            });
        }
        out
    }

    /// Build a GeoJSON FeatureCollection of scored clusters.
    ///
    /// Ports `FusionScorer.score_to_geojson`: one Point feature per cluster with
    /// `cluster_id`, `persistence`, `confidence`, and `fusion_score` properties.
    pub fn score_to_geojson(&self, clusters: &[SarCluster], scores: &[f64]) -> Value {
        let features: Vec<Value> = clusters
            .iter()
            .zip(scores.iter())
            .map(|(cluster, &score)| {
                json!({
                    "type": "Feature",
                    "geometry": {
                        "type": "Point",
                        "coordinates": [cluster.lon, cluster.lat]
                    },
                    "properties": {
                        "cluster_id": cluster.cluster_id,
                        "persistence": cluster.persistence,
                        "confidence": cluster.confidence,
                        "fusion_score": score
                    }
                })
            })
            .collect();
        json!({ "type": "FeatureCollection", "features": features })
    }
}

impl Default for FusionScorer {
    fn default() -> Self {
        Self::new()
    }
}

/// Pure averaging of three sensor scores — extracted so the core of
/// `score_clusters` is unit-testable without network access.
///
/// Mirrors `(swot + ecostress + opera) / 3` from `FusionScorer.score_clusters`.
pub fn average_sensor_scores(swot: f64, ecostress: f64, opera: f64) -> f64 {
    (swot + ecostress + opera) / 3.0
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cluster(id: i64, lat: f64, lon: f64) -> SarCluster {
        SarCluster {
            cluster_id: id,
            lat,
            lon,
            persistence: 0.9,
            confidence: 0.85,
            n_points: 7,
            orbit: "ascending".into(),
            props: HashMap::new(),
        }
    }

    #[test]
    fn average_matches_python_formula() {
        // Python placeholders: 0.8 + 0.7 + 0.9 → 2.4 / 3 = 0.8.
        let avg = average_sensor_scores(0.8, 0.7, 0.9);
        assert!((avg - 0.8).abs() < 1e-12);
    }

    #[test]
    fn average_of_neutral_scores() {
        let avg = average_sensor_scores(
            NEUTRAL_SENSOR_SCORE,
            NEUTRAL_SENSOR_SCORE,
            NEUTRAL_SENSOR_SCORE,
        );
        assert!((avg - NEUTRAL_SENSOR_SCORE).abs() < 1e-12);
    }

    #[test]
    fn geojson_structure_and_properties() {
        let scorer = FusionScorer::new();
        let clusters = vec![cluster(1, 45.255, -81.621), cluster(2, 45.271, -81.615)];
        let scores = vec![0.8, 0.75];
        let gj = scorer.score_to_geojson(&clusters, &scores);

        assert_eq!(gj["type"], "FeatureCollection");
        let feats = gj["features"].as_array().unwrap();
        assert_eq!(feats.len(), 2);

        // First feature: geometry coords are [lon, lat] and fusion_score carried.
        let f0 = &feats[0];
        assert_eq!(f0["geometry"]["type"], "Point");
        let coords = f0["geometry"]["coordinates"].as_array().unwrap();
        assert!((coords[0].as_f64().unwrap() - (-81.621)).abs() < 1e-9);
        assert!((coords[1].as_f64().unwrap() - 45.255).abs() < 1e-9);
        assert_eq!(f0["properties"]["cluster_id"], 1);
        assert!((f0["properties"]["fusion_score"].as_f64().unwrap() - 0.8).abs() < 1e-9);
    }

    #[test]
    fn geojson_zips_to_shorter_length() {
        // zip stops at the shorter of clusters/scores (matches Python zip).
        let scorer = FusionScorer::new();
        let clusters = vec![cluster(1, 1.0, 1.0), cluster(2, 2.0, 2.0)];
        let scores = vec![0.5];
        let gj = scorer.score_to_geojson(&clusters, &scores);
        assert_eq!(gj["features"].as_array().unwrap().len(), 1);
    }
}

// ── Annular z-score for small raster windows (Level 2 hook) ───────────────────
//
// Shared utility for SWOT WSE and ECOSTRESS LST scoring: compare the center
// pixel(s) against an annular background ring, exactly as our optical concepts
// do. Produces a signed z-score; |z| maps to the 0.6..0.95 score range.
// This is the math that upgrades coverage-only (0.6) to anomaly-scoring when
// a COG raster window is available.

/// Local annular z-score: center 3×3 vs surrounding ring, NaN/NoData-aware.
/// Returns 0.0 when insufficient data or flat field (no anomaly).
pub fn annular_z_score(view: ndarray::ArrayView2<f32>) -> f64 {
    let (rows, cols) = view.dim();
    if rows < 5 || cols < 5 {
        return 0.0;
    }
    let cr = rows / 2;
    let cc = cols / 2;
    let center = view[[cr, cc]];
    if !center.is_finite() || center == 0.0 || center == -9999.0 {
        return 0.0;
    }
    let mut bg = Vec::with_capacity(rows * cols);
    for r in 0..rows {
        for c in 0..cols {
            let in_core = r >= cr.saturating_sub(1) && r <= cr + 1
                && c >= cc.saturating_sub(1) && c <= cc + 1;
            if !in_core {
                let v = view[[r, c]];
                if v.is_finite() && v != 0.0 && v != -9999.0 {
                    bg.push(v as f64);
                }
            }
        }
    }
    if bg.len() < 4 {
        return 0.0;
    }
    let n = bg.len() as f64;
    let mean = bg.iter().sum::<f64>() / n;
    let std = (bg.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
    if std < 1e-4 {
        return 0.0;
    }
    (center as f64 - mean) / std
}
