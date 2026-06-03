//! Candidate fusion — merge per-concept and temporal signals into ranked candidates.
//!
//! A "Candidate" is a geographic point with a composite anomaly score across
//! all active detection signals (optical concepts, temporal persistence,
//! drift envelope intersection).  This is the common output format shared
//! between the satellite, mag, and BAG pipelines.

use crate::types::{Candidate, ConceptResult};
use chrono::NaiveDate;
use std::collections::HashMap;
use tracing::debug;

// ── Fusion constants ──────────────────────────────────────────────────────────

/// Minimum composite score to emit a candidate.
pub const MIN_EMIT_SCORE: f64 = 3.0;

/// Weight for each signal class in the composite.
pub const W_CONCEPT: f64 = 0.6; // optical concept score (0–10)
pub const W_TEMPORAL: f64 = 0.3; // temporal persistence z-score contribution
pub const W_DRIFT: f64 = 0.1; // drift envelope proximity bonus
pub const W_KNOWN: f64 = 4.0; // bonus (points) when co-located with a known wreck
/// Radius (m) within which a candidate is considered co-located with a known wreck.
pub const KNOWN_PROXIMITY_RADIUS_M: f64 = 300.0;

// ── Per-location signal aggregator ───────────────────────────────────────────

/// Intermediate aggregate: collects all signals for one spatial candidate.
#[derive(Debug, Default, Clone)]
struct SignalBundle {
    concept_scores: Vec<(String, f64)>,
    temporal_z: Option<f64>,
    drift_proximity: Option<f64>, // 0–1 (1 = inside envelope centroid)
    known_proximity: f64,         // 0–1 boost when near a known wreck (0 = none)
    lat: f64,
    lon: f64,
    depth_m: f64,
    wreck_id: String,
    wreck_name: String,
    best_date: Option<NaiveDate>,
}

impl SignalBundle {
    fn composite_score(&self) -> f64 {
        // Best concept score
        let best_concept = self
            .concept_scores
            .iter()
            .map(|(_, s)| *s)
            .fold(0.0_f64, f64::max);

        let temporal_contrib = self
            .temporal_z
            .map(|z| (z.abs() / 4.0).min(1.0) * 10.0) // map z to 0–10
            .unwrap_or(0.0);

        let drift_bonus = self.drift_proximity.unwrap_or(0.0) * 10.0; // 0–10

        // Known-wreck proximity bonus: a candidate co-located with a documented
        // wreck is more likely real (corroboration), so boost its rank.
        let known_bonus = self.known_proximity * W_KNOWN;

        W_CONCEPT * best_concept + W_TEMPORAL * temporal_contrib + W_DRIFT * drift_bonus + known_bonus
    }

    fn best_concept(&self) -> Option<String> {
        self.concept_scores
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(name, _)| name.clone())
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Build a candidate list from concept results.
///
/// Groups results by wreck identity, computes a composite score, and returns
/// candidates ordered by descending composite score.
///
/// `temporal_zscores` — optional map wreck_id → max temporal persistence z-score.
/// `drift_proximity`  — optional map wreck_id → drift proximity (0–1).
pub fn fuse_candidates(
    concept_results: &[ConceptResult],
    temporal_zscores: Option<&HashMap<String, f64>>,
    drift_proximity: Option<&HashMap<String, f64>>,
    min_score: f64,
) -> Vec<Candidate> {
    fuse_candidates_with_known(concept_results, temporal_zscores, drift_proximity, &[], min_score)
}

/// As [`fuse_candidates`], but boosts candidates co-located (within
/// [`KNOWN_PROXIMITY_RADIUS_M`]) with any of `known_wrecks` (lat, lon).
pub fn fuse_candidates_with_known(
    concept_results: &[ConceptResult],
    temporal_zscores: Option<&HashMap<String, f64>>,
    drift_proximity: Option<&HashMap<String, f64>>,
    known_wrecks: &[(f64, f64)],
    min_score: f64,
) -> Vec<Candidate> {
    // Accumulate per-wreck signal bundles
    let mut bundles: HashMap<String, SignalBundle> = HashMap::new();

    for r in concept_results {
        let bundle = bundles
            .entry(r.wreck_id.clone())
            .or_insert_with(|| SignalBundle {
                lat: r.lat,
                lon: r.lon,
                depth_m: r.depth_m,
                wreck_id: r.wreck_id.clone(),
                wreck_name: r.wreck_name.clone(),
                ..Default::default()
            });

        bundle.concept_scores.push((r.concept.clone(), r.score));

        // Track best date across concepts
        if let Some(date) = r.best_date {
            if bundle.best_date.map_or(true, |existing| date > existing) {
                bundle.best_date = Some(date);
            }
        }
    }

    // Inject temporal and drift signals
    for (wreck_id, bundle) in &mut bundles {
        if let Some(tz) = temporal_zscores.and_then(|m| m.get(wreck_id)) {
            bundle.temporal_z = Some(*tz);
        }
        if let Some(dp) = drift_proximity.and_then(|m| m.get(wreck_id)) {
            bundle.drift_proximity = Some(*dp);
        }
    }

    // Build candidates
    let mut candidates: Vec<Candidate> = bundles
        .into_values()
        .map(|b| {
            let composite = b.composite_score();
            let mut signals: HashMap<String, f64> = b
                .concept_scores
                .iter()
                .cloned()
                .collect();
            if let Some(z) = b.temporal_z {
                signals.insert("temporal_persistence_z".into(), z);
            }
            if let Some(dp) = b.drift_proximity {
                signals.insert("drift_proximity".into(), dp);
            }
            Candidate {
                id: b.wreck_id.clone(),
                lat: b.lat,
                lon: b.lon,
                depth_m: b.depth_m,
                composite_score: composite,
                signals,
                best_concept: b.best_concept(),
                best_date: b.best_date,
                notes: b.wreck_name,
            }
        })
        .filter(|c| c.composite_score >= min_score)
        .collect();

    candidates.sort_by(|a, b| b.composite_score.partial_cmp(&a.composite_score).unwrap());
    debug!("Fusion: {} candidates above threshold {}", candidates.len(), min_score);
    candidates
}

// ── Drift proximity helper ────────────────────────────────────────────────────

/// Compute a 0–1 proximity score for a point relative to a drift envelope centroid.
/// 1.0 = at the centroid, 0.0 = outside `max_distance_nm`.
pub fn drift_proximity_score(
    point_lat: f64,
    point_lon: f64,
    centroid_lat: f64,
    centroid_lon: f64,
    max_distance_nm: f64,
) -> f64 {
    let dist = crate::drift::haversine_nm(point_lat, point_lon, centroid_lat, centroid_lon);
    (1.0 - dist / max_distance_nm).max(0.0)
}

// ── Validation comparison ─────────────────────────────────────────────────────

use crate::types::{ValidationEntry, ValidationReport};

/// Compare concept results against ground-truth wrecks.
pub fn validate_against_gt(
    concept_results: &[ConceptResult],
    gt_wrecks: &[crate::types::WreckTarget],
    min_gt_score: f64,
    min_gt_hit_rate: f64,
    mission_id: &str,
) -> ValidationReport {
    use std::collections::HashMap;

    // Best result per wreck name (case-insensitive)
    let mut by_name: HashMap<String, &ConceptResult> = HashMap::new();
    for r in concept_results {
        let key = r.wreck_name.to_lowercase();
        let entry = by_name.entry(key).or_insert(r);
        if r.score > entry.score {
            *entry = r;
        }
    }

    let mut entries = Vec::new();
    let mut n_pass = 0usize;

    for w in gt_wrecks {
        let row = by_name.get(&w.name.to_lowercase());
        let (score, hit_rate, concept, pass) = match row {
            None => (0.0, 0.0, String::new(), false),
            Some(r) => {
                let ok = r.score >= min_gt_score || r.hit_rate >= min_gt_hit_rate;
                (r.score, r.hit_rate, r.concept.clone(), ok)
            }
        };
        if pass {
            n_pass += 1;
        }
        entries.push(ValidationEntry {
            name: w.name.clone(),
            lat: w.lat,
            lon: w.lon,
            score,
            hit_rate,
            concept,
            status: if pass { "pass".into() } else if score == 0.0 { "no_data".into() } else { "weak_signal".into() },
            pass,
        });
    }

    let n_gt = gt_wrecks.len();
    ValidationReport {
        mission_id: mission_id.into(),
        min_gt_score,
        min_gt_hit_rate,
        n_gt,
        n_pass,
        pass_rate: if n_gt > 0 { n_pass as f64 / n_gt as f64 } else { 0.0 },
        wrecks: entries,
    }
}

// ── SAR cluster fusion hook ───────────────────────────────────────────────────

use crate::types::SarCluster;

/// Convert SAR temporal-persistence clusters (`sar.rs`) into fusion candidates.
///
/// This is the hook requested for the SAR DBSCAN persistence stage: each
/// [`SarCluster`] becomes a [`Candidate`] whose composite score is derived from
/// its persistence (0–1 fraction → 0–10) and whose `signals` carry the raw
/// persistence/confidence.  When `fusion_scores` is supplied (e.g. from
/// `nasa_fusion::FusionScorer::score_clusters`), the per-cluster NASA fusion
/// score (0–1) is folded into the composite and recorded as a signal.
///
/// Clusters scoring below `min_score` are dropped; the rest are returned sorted
/// by descending composite score.
pub fn fuse_sar_clusters(
    clusters: &[SarCluster],
    fusion_scores: Option<&[f64]>,
    min_score: f64,
) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = clusters
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let persistence_score = (c.persistence.clamp(0.0, 1.0)) * 10.0;
            let mut signals: HashMap<String, f64> = HashMap::new();
            signals.insert("sar_persistence".into(), c.persistence);
            signals.insert("sar_confidence".into(), c.confidence);

            // Composite: persistence is the base; NASA fusion (if present) lifts it.
            let composite = match fusion_scores.and_then(|s| s.get(i)) {
                Some(&fs) => {
                    signals.insert("nasa_fusion_score".into(), fs);
                    // Average persistence (0–10) with NASA fusion mapped to 0–10.
                    0.5 * persistence_score + 0.5 * (fs.clamp(0.0, 1.0) * 10.0)
                }
                None => persistence_score,
            };

            let name = c
                .props
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("sar_cluster_{}", c.cluster_id));

            Candidate {
                id: format!("sar_{}", c.cluster_id),
                lat: c.lat,
                lon: c.lon,
                depth_m: 0.0,
                composite_score: composite,
                signals,
                best_concept: Some("sar_temporal_persistence".into()),
                best_date: None,
                notes: name,
            }
        })
        .filter(|c| c.composite_score >= min_score)
        .collect();

    candidates.sort_by(|a, b| b.composite_score.partial_cmp(&a.composite_score).unwrap_or(std::cmp::Ordering::Equal));
    debug!("SAR fusion: {} clusters above threshold {}", candidates.len(), min_score);
    candidates
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(id: &str, concept: &str, score: f64, hit_rate: f64) -> ConceptResult {
        ConceptResult {
            wreck_id: id.into(),
            wreck_name: id.into(),
            lat: 42.0,
            lon: -82.0,
            depth_m: 20.0,
            concept: concept.into(),
            n_scenes: 5,
            n_hits: 2,
            hit_rate,
            mean_zscore: 2.0,
            best_zscore: 3.5,
            best_date: None,
            score,
            notes: String::new(),
        }
    }

    #[test]
    fn fusion_ranking() {
        let results = vec![
            make_result("W1", "shadow_roughness", 7.0, 0.6),
            make_result("W1", "zebra_clarity", 5.0, 0.4),
            make_result("W2", "sediment_plume", 3.5, 0.3),
        ];
        let candidates = fuse_candidates(&results, None, None, 0.0);
        assert_eq!(candidates[0].id, "W1", "W1 should rank first");
        assert!(candidates[0].composite_score > candidates[1].composite_score);
    }

    #[test]
    fn drift_proximity_at_centre() {
        let score = drift_proximity_score(42.0, -82.0, 42.0, -82.0, 10.0);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn fuse_sar_clusters_with_and_without_nasa() {
        use crate::types::SarCluster;
        use std::collections::HashMap;
        let clusters = vec![
            SarCluster {
                cluster_id: 1,
                lat: 45.0,
                lon: -81.0,
                persistence: 0.9,
                confidence: 0.85,
                n_points: 9,
                orbit: "ascending".into(),
                props: HashMap::new(),
            },
            SarCluster {
                cluster_id: 2,
                lat: 45.1,
                lon: -81.1,
                persistence: 0.2, // weak → should be filtered at min_score=5
                confidence: 0.3,
                n_points: 2,
                orbit: "ascending".into(),
                props: HashMap::new(),
            },
        ];
        // Without NASA scores: persistence*10 = 9.0 and 2.0; min_score 5 keeps one.
        let cands = fuse_sar_clusters(&clusters, None, 5.0);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].id, "sar_1");
        assert!((cands[0].composite_score - 9.0).abs() < 1e-9);

        // With NASA fusion scores folded in.
        let nasa = vec![0.8, 0.8];
        let cands2 = fuse_sar_clusters(&clusters, Some(&nasa), 0.0);
        assert_eq!(cands2.len(), 2);
        // cluster 1: 0.5*9.0 + 0.5*8.0 = 8.5
        let c1 = cands2.iter().find(|c| c.id == "sar_1").unwrap();
        assert!((c1.composite_score - 8.5).abs() < 1e-9);
        assert!(c1.signals.contains_key("nasa_fusion_score"));
    }
}
