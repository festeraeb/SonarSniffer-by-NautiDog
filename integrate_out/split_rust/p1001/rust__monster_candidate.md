# integrate/unmapped/laptopdump_wreckhunter_build/monster_candidate.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/monster_candidate.rs

## Rust source
```rust
//! Monster Candidate Analysis Module
//! Analyzes thermal mass anomalies for large steel freighter candidates
//! Integrates with CESAROPS inference pipeline for wreck identification

use std::fmt;
use cesarops_common::geo::{LatLon, Distance};
use cesarops_common::units::{Length, Mass, Temperature};

/// Monster candidate specifications
#[derive(Debug, Clone)]
pub struct MonsterSpecs {
    pub latitude: f64,
    pub longitude: f64,
    pub length_ft: f64,
    pub mass_tons: f64,
    pub year_established: Option<i32>,
}

impl Default for MonsterSpecs {
    fn default() -> Self {
        Self {
            latitude: 42.4180,
            longitude: -87.2350,
            length_ft: 343.0,
            mass_tons: 14474.0,
            year_established: Some(1929),
        }
    }
}

impl MonsterSpecs {
    /// Create monster specs from configuration
    pub fn new(
        latitude: f64,
        longitude: f64,
        length_ft: f64,
        mass_tons: f64,
        year_established: Option<i32>,
    ) -> Self {
        Self {
            latitude,
            longitude,
            length_ft,
            mass_tons,
            year_established,
        }
    }

    /// Convert length to meters
    pub fn length_m(&self) -> f64 {
        self.length_ft * 0.3048
    }

    /// Convert mass to kilograms
    pub fn mass_kg(&self) -> f64 {
        self.mass_tons * 1000.0
    }

    /// Calculate expected pixel area for Landsat-8 B10 (30m/pixel)
    pub fn expected_pixel_count(&self) -> u64 {
        let pixels_per_ft = 1.0 / 98.4; // 30m = 98.4ft
        let target_pixels = (self.length_ft * pixels_per_ft).powi(2);
        target_pixels as u64
    }
}

/// Candidate anomaly record
#[derive(Debug, Clone)]
pub struct Candidate {
    pub rank: u32,
    pub anomaly_id: u32,
    pub row: u32,
    pub col: u32,
    pub pixel_count: u32,
    pub z_score: f64,
    pub notes: String,
    pub is_best_match: bool,
}

impl Candidate {
    /// Create a new candidate
    pub fn new(
        rank: u32,
        anomaly_id: u32,
        row: u32,
        col: u32,
        pixel_count: u32,
        z_score: f64,
        notes: String,
    ) -> Self {
        Self {
            rank,
            anomaly_id,
            row,
            col,
            pixel_count,
            z_score,
            notes,
            is_best_match: false,
        }
    }

    /// Mark as best match if pixel count is in target range
    pub fn mark_best_match(&mut self, target_min: u32, target_max: u32) {
        self.is_best_match = self.pixel_count >= target_min && self.pixel_count <= target_max;
    }
}

/// Monster candidate analysis result
#[derive(Debug)]
pub struct MonsterAnalysisResult {
    pub monster_specs: MonsterSpecs,
    pub expected_pixel_count: u64,
    pub candidates: Vec<Candidate>,
    pub best_match: Option<Candidate>,
    pub verification_steps: Vec<String>,
}

impl MonsterAnalysisResult {
    /// Create analysis result from monster specs and candidates
    pub fn new(
        monster_specs: MonsterSpecs,
        candidates: Vec<Candidate>,
    ) -> Self {
        let expected_pixel_count = monster_specs.expected_pixel_count();
        
        let mut result = Self {
            monster_specs,
            expected_pixel_count,
            candidates,
            best_match: None,
            verification_steps: vec![
                "Cross-check with 2025 Sentinel-2 tile".to_string(),
                "Apply 1.47x Zion Constant depth correction".to_string(),
                "Verify stationary (not school of fish)".to_string(),
                "Check for 14,474 ton mass signature".to_string(),
            ],
        };

        // Find best match
        let target_min = 100;
        let target_max = 500;
        for candidate in &mut result.candidates {
            candidate.mark_best_match(target_min, target_max);
        }

        // Sort by pixel count and z-score
        result.candidates.sort_by(|a, b| {
            b.pixel_count.cmp(&a.pixel_count)
                .then_with(|| b.z_score.partial_cmp(&a.z_score).unwrap_or(std::cmp::Ordering::Equal))
        });

        // Mark best match
        if let Some(best) = result.candidates.first() {
            if best.is_best_match {
                result.best_match = Some(best.clone());
            }
        }

        result
    }

    /// Get the best match candidate
    pub fn best_match(&self) -> Option<&Candidate> {
        self.best_match.as_ref()
    }

    /// Get candidates in monster-sized range
    pub fn monster_sized_candidates(&self) -> Vec<&Candidate> {
        self.candidates
            .iter()
            .filter(|c| 100 <= c.pixel_count && c.pixel_count <= 500)
            .collect()
    }
}

/// Side-by-side comparison data
#[derive(Debug, Clone)]
pub struct ComparisonData {
    pub target_name: String,
    pub length_ft: f64,
    pub mass_tons: f64,
    pub coordinates: LatLon,
    pub pixel_target: u64,
    pub best_anomaly_id: u32,
    pub best_anomaly_pixels: u32,
    pub z_score: f64,
    pub position: (u32, u32),
}

impl ComparisonData {
    /// Create comparison data for a target
    pub fn new(
        name: &str,
        length_ft: f64,
        mass_tons: f64,
        lat: f64,
        lon: f64,
        pixel_target: u64,
        best_anomaly_id: u32,
        best_anomaly_pixels: u32,
        z_score: f64,
        position: (u32, u32),
    ) -> Self {
        Self {
            target_name: name.to_string(),
            length_ft,
            mass_tons,
            coordinates: LatLon::new(lat, lon),
            pixel_target,
            best_anomaly_id,
            best_anomaly_pixels,
            z_score,
            position,
        }
    }
}

/// Monster candidate analysis module
pub mod monster_candidate {
    use super::*;

    /// Analyze monster candidates from milled data
    pub fn analyze_monster_candidates(
        monster_specs: MonsterSpecs,
        candidates: Vec<(u32, u32, u32, u32, u32, f64, String)>,
    ) -> MonsterAnalysisResult {
        // Convert raw candidate data to Candidate structs
        let mut candidates_struct: Vec<Candidate> = candidates
            .iter()
            .enumerate()
            .map(|(i, &(rank, anomaly_id, row, col, pixels, zscore, notes)| {
                Candidate::new(
                    (i + 1) as u32,
                    anomaly_id,
                    row,
                    col,
                    pixels,
                    zscore,
                    notes,
                )
            })
            .collect();

        MonsterAnalysisResult::new(monster_specs, candidates_struct)
    }

    /// Generate comparison data for multiple targets
    pub fn generate_comparison_data(
        targets: Vec<(
            &str,
            f64,
            f64,
            f64,
            f64,
            u64,
            u32,
            u32,
            f64,
            (u32, u32),
        )>,
    ) -> Vec<ComparisonData> {
        targets
            .iter()
            .map(|&(
                name,
                length_ft,
                mass_tons,
                lat,
                lon,
                pixel_target,
                best_anomaly_id,
                best_anomaly_pixels,
                z_score,
                position,
            )| {
                ComparisonData::new(
                    name,
                    length_ft,
                    mass_tons,
                    lat,
                    lon,
                    pixel_target,
                    best_anomaly_id,
                    best_anomaly_pixels,
                    z_score,
                    position,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expected_pixel_count() {
        let specs = MonsterSpecs::default();
        let expected = specs.expected_pixel_count();
        assert!(expected > 0);
    }

    #[test]
    fn test_candidate_best_match() {
        let mut candidate = Candidate::new(
            1,
            61,
            2368,
            419,
            422,
            2.71,
            "Possible wreck - S of Monster".to_string(),
        );
        candidate.mark_best_match(100, 500);
        assert!(candidate.is_best_match);
    }

    #[test]
    fn test_monster_sized_candidates() {
        let mut candidates = vec![
            Candidate::new(1, 1, 1, 1, 50, 2.0, "small".to_string()),
            Candidate::new(2, 2, 2, 2, 200, 2.5, "medium".to_string()),
            Candidate::new(3, 3, 3, 3, 600, 3.0, "large".to_string()),
        ];
        let result = MonsterAnalysisResult::new(MonsterSpecs::default(), candidates);
        let monster_sized = result.monster_sized_candidates();
        assert_eq!(monster_sized.len(), 1);
        assert_eq!(monster_sized[0].pixel_count, 200);
    }
}
```

## Forge wire
- Pipeline calls `monster_candidate::analyze_monster_candidates()` to process milled anomaly data
- Returns `MonsterAnalysisResult` with ranked candidates and best match identification
- Comparison data is generated via `generate_comparison_data()` for multi-target analysis
- Results are serialized to JSON for downstream verification workflows

## Risks
- Hardcoded Landsat-8 resolution (30m/pixel) needs configuration for different satellite sources
- Z-score thresholds are magic numbers that should be parameterized
- Coordinate system assumptions (WGS84) need explicit documentation
- Pixel count calculations assume rectangular bounding boxes; irregular shapes need area correction
