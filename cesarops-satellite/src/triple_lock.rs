//! Triple-lock multi-sensor fusion — tunable port of `triple_lock_fusion.py`.
//!
//! The original Python tool hand-coded three per-sensor z-score thresholds
//! (thermal cold-sink, SAR steel, optical glint) and a spatial fuse tolerance.
//! Those values were tuned offline (originally with the LightGBM wreck
//! classifier in `wreck_ml_trainer.py`) and then frozen into the script.
//!
//! This port keeps the same fusion semantics but reads every threshold from
//! [`Knobs`], so the operator (or an ML retrain) can re-tune them per mission
//! instead of editing source.  The defaults equal the frozen Python values
//! (thermal/sar/optical = 2.5 z, tolerance = 300 m, min_locks = 3).
//!
//! Detection philosophy (operator's "triple-lock"): a candidate is only
//! trustworthy when **independent sensor families** agree at the same place.
//! A wreck hasn't moved in 100 years, so the families need NOT agree on date —
//! thermal from one year + optical from another + SAR from a third still lock.

use crate::types::{Candidate, ConceptResult, Knobs};
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Independent sensor families. A "lock" is one distinct family clearing its
/// threshold at a location; `min_locks` of these must co-locate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorFamily {
    Thermal,
    Sar,
    Optical,
    Glint,
    Temporal,
}

impl SensorFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            SensorFamily::Thermal => "thermal",
            SensorFamily::Sar => "sar",
            SensorFamily::Optical => "optical",
            SensorFamily::Glint => "glint",
            SensorFamily::Temporal => "temporal",
        }
    }

    /// Classify a concept/signal name into its independent sensor family.
    /// Unknown names default to Optical (the broadest passive-optical bucket).
    pub fn classify(concept: &str) -> SensorFamily {
        let c = concept.to_lowercase();
        if c.contains("thermal") || c.contains("cold_sink") || c.contains("coldsink")
            || c.contains("heat_sink") || c.contains("heatsink") || c.contains("ecostress")
        {
            SensorFamily::Thermal
        } else if c.contains("sar") || c.contains("backscatter") || c.contains("coherence") {
            SensorFamily::Sar
        } else if c.contains("temporal") || c.contains("persistence") {
            SensorFamily::Temporal
        } else if c.contains("glint") || c.contains("roughness") {
            // Glint/roughness is an independent family from clarity — a wreck
            // modulates surface current → glint texture, which is a different
            // physics path than the water-column clarity ratio. Operator considers
            // clarity + glint + thermal = genuine triple-lock.
            SensorFamily::Glint
        } else {
            // clarity, zebra, shadow, plume, blue_green, sdb ...
            SensorFamily::Optical
        }
    }

    /// Per-family threshold from the tunable knobs.
    pub fn threshold(self, knobs: &Knobs) -> f64 {
        match self {
            SensorFamily::Thermal => knobs.triple_lock_thermal_z,
            SensorFamily::Sar => knobs.triple_lock_sar_z,
            SensorFamily::Optical => knobs.triple_lock_optical_z,
            SensorFamily::Glint => knobs.triple_lock_optical_z, // same physics as optical
            SensorFamily::Temporal => knobs.triple_lock_temporal_z,
        }
    }
}

/// A single sensor anomaly at a point (one family's evidence).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorHit {
    pub lat: f64,
    pub lon: f64,
    pub family: SensorFamily,
    /// Signed z-score (sign carries direction; magnitude is what's thresholded).
    pub zscore: f64,
    /// Human-readable source (concept name, cluster id, ...).
    pub source: String,
}

/// A fused multi-sensor detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TripleLockTarget {
    pub lat: f64,
    pub lon: f64,
    /// Number of distinct sensor families that locked here.
    pub lock_level: u8,
    pub families: Vec<String>,
    /// Best |z| seen at this location across all contributing hits.
    pub max_zscore: f64,
    /// Mean |z| across contributing hits.
    pub avg_zscore: f64,
    /// avg_zscore * lock_level (matches Python `confidence`).
    pub confidence: f64,
    pub n_anomalies: usize,
    pub sources: Vec<String>,
}

/// Approximate metres-per-degree-latitude (good enough for a tolerance cluster).
const M_PER_DEG: f64 = 111_320.0;

/// Fuse single-sensor hits into multi-sensor locks.
///
/// Greedy spatial clustering identical in spirit to the Python
/// `fuse_triple_lock`: walk hits in order, seed a cluster, absorb any
/// not-yet-used hit within `tolerance_m`, then count distinct families.
/// Only clusters with `>= min_locks` distinct families are returned.
pub fn fuse_triple_lock(hits: &[SensorHit], tolerance_m: f64, min_locks: u8) -> Vec<TripleLockTarget> {
    // Tolerance in degrees (Euclidean in lat/lon — matches Python's tol_deg).
    let tol_deg = tolerance_m / M_PER_DEG;
    let tol_deg2 = tol_deg * tol_deg;

    let mut used = vec![false; hits.len()];
    let mut out: Vec<TripleLockTarget> = Vec::new();

    for i in 0..hits.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut cluster: Vec<&SensorHit> = vec![&hits[i]];

        for j in (i + 1)..hits.len() {
            if used[j] {
                continue;
            }
            let dlat = hits[i].lat - hits[j].lat;
            let dlon = hits[i].lon - hits[j].lon;
            if dlat * dlat + dlon * dlon < tol_deg2 {
                cluster.push(&hits[j]);
                used[j] = true;
            }
        }

        // Distinct families present in this cluster.
        let mut families: Vec<SensorFamily> = Vec::new();
        for h in &cluster {
            if !families.contains(&h.family) {
                families.push(h.family);
            }
        }
        let lock_level = families.len() as u8;
        if lock_level < min_locks {
            continue;
        }

        let n = cluster.len() as f64;
        let avg_lat = cluster.iter().map(|h| h.lat).sum::<f64>() / n;
        let avg_lon = cluster.iter().map(|h| h.lon).sum::<f64>() / n;
        let abs_z: Vec<f64> = cluster.iter().map(|h| h.zscore.abs()).collect();
        let avg_z = abs_z.iter().sum::<f64>() / n;
        let max_z = abs_z.iter().cloned().fold(0.0_f64, f64::max);

        let mut sources: Vec<String> = cluster.iter().map(|h| h.source.clone()).collect();
        sources.sort();
        sources.dedup();

        out.push(TripleLockTarget {
            lat: avg_lat,
            lon: avg_lon,
            lock_level,
            families: families.iter().map(|f| f.as_str().to_string()).collect(),
            max_zscore: max_z,
            avg_zscore: avg_z,
            confidence: avg_z * lock_level as f64,
            n_anomalies: cluster.len(),
            sources,
        });
    }

    // Triple locks first, then by confidence (matches Python sort key).
    out.sort_by(|a, b| {
        b.lock_level
            .cmp(&a.lock_level)
            .then(b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal))
    });
    debug!(
        "Triple-lock: {} locks (>= {} families) from {} hits",
        out.len(),
        min_locks,
        hits.len()
    );
    out
}

/// Build sensor hits from the pipeline's concept results, keeping only those
/// whose |z| clears their family threshold.  This is the bridge from the
/// optical/SAR/temporal stages into the triple-lock gate.
pub fn hits_from_concept_results(results: &[ConceptResult], knobs: &Knobs) -> Vec<SensorHit> {
    results
        .iter()
        .filter_map(|r| {
            let family = SensorFamily::classify(&r.concept);
            // Use best_zscore as the anomaly strength (already direction-aware).
            let z = if r.best_zscore.abs() > 0.0 { r.best_zscore } else { r.mean_zscore };
            if z.abs() >= family.threshold(knobs) {
                Some(SensorHit {
                    lat: r.lat,
                    lon: r.lon,
                    family,
                    zscore: z,
                    source: r.concept.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Build sensor hits from fused candidates (used when concept-level z-scores
/// aren't available, e.g. POC-derived candidates carry per-signal scores).
/// Each candidate signal becomes a hit if its score clears the family threshold.
pub fn hits_from_candidates(candidates: &[Candidate], knobs: &Knobs) -> Vec<SensorHit> {
    let mut hits = Vec::new();
    for c in candidates {
        for (signal, &score) in &c.signals {
            let family = SensorFamily::classify(signal);
            if score.abs() >= family.threshold(knobs) {
                hits.push(SensorHit {
                    lat: c.lat,
                    lon: c.lon,
                    family,
                    zscore: score,
                    source: signal.clone(),
                });
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(lat: f64, lon: f64, fam: SensorFamily, z: f64) -> SensorHit {
        SensorHit { lat, lon, family: fam, zscore: z, source: fam.as_str().into() }
    }

    #[test]
    fn classify_families() {
        assert_eq!(SensorFamily::classify("thermal_coldsink"), SensorFamily::Thermal);
        assert_eq!(SensorFamily::classify("sar_temporal_persistence"), SensorFamily::Sar);
        assert_eq!(SensorFamily::classify("temporal_persistence_z"), SensorFamily::Temporal);
        assert_eq!(SensorFamily::classify("blue_green_clarity"), SensorFamily::Optical);
        assert_eq!(SensorFamily::classify("glint_roughness"), SensorFamily::Glint);
    }

    #[test]
    fn three_families_lock() {
        let hits = vec![
            hit(45.5, -84.5, SensorFamily::Thermal, -3.0),
            hit(45.5, -84.5, SensorFamily::Sar, 2.8),
            hit(45.5001, -84.5001, SensorFamily::Optical, 2.6),
        ];
        let locks = fuse_triple_lock(&hits, 300.0, 3);
        assert_eq!(locks.len(), 1);
        assert_eq!(locks[0].lock_level, 3);
        assert!(locks[0].confidence > 0.0);
    }

    #[test]
    fn two_families_no_triple() {
        let hits = vec![
            hit(45.5, -84.5, SensorFamily::Thermal, -3.0),
            hit(45.5, -84.5, SensorFamily::Optical, 2.6),
        ];
        // Same family twice should NOT raise the lock level.
        let mut more = hits.clone();
        more.push(hit(45.5, -84.5, SensorFamily::Optical, 4.0));
        assert!(fuse_triple_lock(&more, 300.0, 3).is_empty());
        // But min_locks=2 should pass with two distinct families.
        let locks = fuse_triple_lock(&hits, 300.0, 2);
        assert_eq!(locks.len(), 1);
        assert_eq!(locks[0].lock_level, 2);
    }

    #[test]
    fn far_apart_not_fused() {
        let hits = vec![
            hit(45.5, -84.5, SensorFamily::Thermal, -3.0),
            hit(45.9, -84.9, SensorFamily::Sar, 2.8),
            hit(45.1, -84.1, SensorFamily::Optical, 2.6),
        ];
        assert!(fuse_triple_lock(&hits, 300.0, 3).is_empty());
    }
}
