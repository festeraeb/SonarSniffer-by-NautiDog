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
/// `{"swot_score": 0.8}`).  No SWOT client exists in this crate yet — this
/// returns a neutral score so the fusion average stays well-defined.  Wire a
/// real PO.DAAC SWOT fetch here when available (see `fetch_swot_data.py`).
pub fn fetch_swot_score(_bbox: [f64; 4]) -> f64 {
    NEUTRAL_SENSOR_SCORE
}

/// TODO stub: ECOSTRESS thermal score.
///
/// Mirrors `nasa_fusion_test.py::fetch_ecostress_data` (placeholder returning
/// `{"ecostress_score": 0.7}`).  No ECOSTRESS client exists in this crate yet —
/// returns a neutral score.  Wire a real AppEEARS/ECOSTRESS fetch here when
/// available (see `fetch_ecostress_data.py`).
pub fn fetch_ecostress_score(_bbox: [f64; 4]) -> f64 {
    NEUTRAL_SENSOR_SCORE
}

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
            let swot = fetch_swot_score(bbox);
            let ecostress = fetch_ecostress_score(bbox);
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
            let swot = fetch_swot_score(bbox);
            let ecostress = fetch_ecostress_score(bbox);
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
