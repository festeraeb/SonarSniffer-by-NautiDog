//! Wreck Classifier — Anomaly classification using multi-signal fusion.
//!
//! Takes the outputs from the detection pipeline (temporal stack anomaly scores,
//! curvelet filter responses, spectral unmixing fractions) and classifies each
//! anomaly candidate into categories:
//!
//! - Wreck (confirmed structural anomaly)
//! - Debris field (scattered material)
//! - Geological feature (natural formation, false positive)
//! - Vessel (active ship, not a wreck)
//! - Unknown (needs more data or field verification)
//!
//! Classification uses a weighted scoring system that can be calibrated against
//! known wreck sites. Future: plug in a trained ML model for higher accuracy.

use std::collections::HashMap;

/// Classification categories for detected anomalies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnomalyClass {
    /// Confirmed structural anomaly consistent with a wreck
    Wreck,
    /// Scattered material pattern (debris field, cargo spill)
    DebrisField,
    /// Natural geological feature (reef, rock outcrop, sandbar)
    Geological,
    /// Active or recently active vessel (not a wreck)
    Vessel,
    /// Insufficient data for classification
    Unknown,
}

impl AnomalyClass {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Wreck => "wreck",
            Self::DebrisField => "debris_field",
            Self::Geological => "geological",
            Self::Vessel => "vessel",
            Self::Unknown => "unknown",
        }
    }
}

/// Input features for classification (one per anomaly candidate).
#[derive(Clone, Debug)]
pub struct AnomalyFeatures {
    /// Temporal stack anomaly score (higher = more anomalous vs baseline)
    pub temporal_score: f32,
    /// Curvelet filter response (higher = more structural/linear features)
    pub curvelet_response: f32,
    /// Metal fraction from spectral unmixing (0.0-1.0)
    pub metal_fraction: f32,
    /// Vegetation fraction (high = likely natural)
    pub vegetation_fraction: f32,
    /// Water depth estimate in meters (if bathymetry available)
    pub depth_m: Option<f32>,
    /// Size of anomaly in pixels
    pub area_pixels: usize,
    /// Aspect ratio (length/width) — wrecks tend to be elongated
    pub aspect_ratio: f32,
    /// Persistence across temporal acquisitions (0.0-1.0, higher = more persistent)
    pub temporal_persistence: f32,
    /// Distance to nearest shipping lane in meters
    pub distance_to_shipping_lane_m: Option<f32>,
    /// Magnetic anomaly strength (if aeromagnetic data available)
    pub magnetic_anomaly_nt: Option<f32>,
    /// SAR coherence loss (if SAR data available, 0.0-1.0)
    pub sar_coherence_loss: Option<f32>,
}

/// Classification result for a single anomaly.
#[derive(Clone, Debug)]
pub struct ClassificationResult {
    /// Primary classification
    pub class: AnomalyClass,
    /// Confidence in the classification (0.0-1.0)
    pub confidence: f32,
    /// Scores for each class (for ranking alternatives)
    pub class_scores: HashMap<AnomalyClass, f32>,
    /// Human-readable explanation of why this classification was chosen
    pub explanation: String,
    /// Recommended action
    pub action: RecommendedAction,
}

/// What to do with this anomaly.
#[derive(Clone, Debug)]
pub enum RecommendedAction {
    /// High confidence — add to wreck database
    AddToDatabase,
    /// Medium confidence — schedule field verification
    FieldVerification,
    /// Needs more satellite passes for temporal confirmation
    AcquireMoreImagery,
    /// Likely false positive — dismiss but log
    Dismiss,
    /// Active vessel — ignore for wreck search
    IgnoreVessel,
}

/// Configuration for the classifier's scoring weights.
#[derive(Clone, Debug)]
pub struct ClassifierConfig {
    // Wreck indicators (positive weights)
    pub w_temporal_score: f32,
    pub w_curvelet: f32,
    pub w_metal: f32,
    pub w_persistence: f32,
    pub w_aspect_ratio: f32,
    pub w_magnetic: f32,
    pub w_sar_coherence: f32,

    // Anti-wreck indicators (negative weights)
    pub w_vegetation: f32,      // high vegetation = not a wreck
    pub w_shallow_depth: f32,   // very shallow = likely geological

    // Thresholds
    pub wreck_threshold: f32,
    pub debris_threshold: f32,
    pub geological_threshold: f32,
    pub vessel_threshold: f32,
    pub confidence_min: f32,
}

impl Default for ClassifierConfig {
    fn default() -> Self {
        Self {
            w_temporal_score: 0.20,
            w_curvelet: 0.15,
            w_metal: 0.25,
            w_persistence: 0.15,
            w_aspect_ratio: 0.05,
            w_magnetic: 0.10,
            w_sar_coherence: 0.10,
            w_vegetation: -0.30,
            w_shallow_depth: -0.15,
            wreck_threshold: 0.65,
            debris_threshold: 0.45,
            geological_threshold: 0.50,
            vessel_threshold: 0.40,
            confidence_min: 0.30,
        }
    }
}

/// The wreck classifier.
pub struct WreckClassifier {
    pub config: ClassifierConfig,
}

impl WreckClassifier {
    pub fn new(config: ClassifierConfig) -> Self {
        Self { config }
    }

    /// Classify a single anomaly based on its features.
    pub fn classify(&self, features: &AnomalyFeatures) -> ClassificationResult {
        let mut scores: HashMap<AnomalyClass, f32> = HashMap::new();

        // ── Wreck Score ──
        let mut wreck_score = 0.0f32;
        wreck_score += self.config.w_temporal_score * normalize_score(features.temporal_score, 2.0, 8.0);
        wreck_score += self.config.w_curvelet * normalize_score(features.curvelet_response, 0.5, 5.0);
        wreck_score += self.config.w_metal * features.metal_fraction;
        wreck_score += self.config.w_persistence * features.temporal_persistence;
        wreck_score += self.config.w_aspect_ratio * normalize_score(features.aspect_ratio, 1.5, 5.0);

        if let Some(mag) = features.magnetic_anomaly_nt {
            wreck_score += self.config.w_magnetic * normalize_score(mag, 10.0, 200.0);
        }
        if let Some(sar) = features.sar_coherence_loss {
            wreck_score += self.config.w_sar_coherence * sar;
        }

        // Anti-indicators
        wreck_score += self.config.w_vegetation * features.vegetation_fraction;
        if let Some(depth) = features.depth_m {
            if depth < 2.0 {
                wreck_score += self.config.w_shallow_depth;
            }
        }

        scores.insert(AnomalyClass::Wreck, wreck_score.clamp(0.0, 1.0));

        // ── Debris Score ──
        // Debris: high temporal score but low structure (no curvelet), scattered
        let debris_score = (
            normalize_score(features.temporal_score, 1.5, 5.0) * 0.3
            + features.metal_fraction * 0.2
            + (1.0 - normalize_score(features.aspect_ratio, 2.0, 5.0)) * 0.2 // NOT elongated
            + features.temporal_persistence * 0.15
            + normalize_score(features.area_pixels as f32, 50.0, 500.0) * 0.15
        ).clamp(0.0, 1.0);
        scores.insert(AnomalyClass::DebrisField, debris_score);

        // ── Geological Score ──
        // Geological: persistent, no metal, often shallow, vegetation nearby
        let geo_score = (
            features.temporal_persistence * 0.3 // very persistent = natural
            + features.vegetation_fraction * 0.25
            + (1.0 - features.metal_fraction) * 0.2
            + if features.depth_m.unwrap_or(10.0) < 3.0 { 0.25 } else { 0.0 }
        ).clamp(0.0, 1.0);
        scores.insert(AnomalyClass::Geological, geo_score);

        // ── Vessel Score ──
        // Vessel: appears in recent imagery but NOT in older, moves between passes
        let vessel_score = (
            normalize_score(features.temporal_score, 2.0, 6.0) * 0.3
            + (1.0 - features.temporal_persistence) * 0.4 // NOT persistent = moving
            + normalize_score(features.aspect_ratio, 2.0, 6.0) * 0.15
            + if features.distance_to_shipping_lane_m.unwrap_or(f32::MAX) < 5000.0 { 0.15 } else { 0.0 }
        ).clamp(0.0, 1.0);
        scores.insert(AnomalyClass::Vessel, vessel_score);

        // ── Select Winner ──
        let (best_class, best_score) = scores.iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(c, s)| (*c, *s))
            .unwrap_or((AnomalyClass::Unknown, 0.0));

        let (class, confidence) = if best_score < self.config.confidence_min {
            (AnomalyClass::Unknown, best_score)
        } else {
            (best_class, best_score)
        };

        let explanation = self.explain(&class, features, &scores);
        let action = self.recommend_action(&class, confidence);

        ClassificationResult {
            class,
            confidence,
            class_scores: scores,
            explanation,
            action,
        }
    }

    /// Classify a batch of anomalies.
    pub fn classify_batch(&self, features: &[AnomalyFeatures]) -> Vec<ClassificationResult> {
        features.iter().map(|f| self.classify(f)).collect()
    }

    /// Generate human-readable explanation.
    fn explain(&self, class: &AnomalyClass, features: &AnomalyFeatures, scores: &HashMap<AnomalyClass, f32>) -> String {
        let mut reasons = Vec::new();

        match class {
            AnomalyClass::Wreck => {
                if features.metal_fraction > 0.15 { reasons.push(format!("metal signature {:.0}%", features.metal_fraction * 100.0)); }
                if features.temporal_persistence > 0.7 { reasons.push("persistent across time".to_string()); }
                if features.aspect_ratio > 2.0 { reasons.push(format!("elongated shape ({:.1}:1)", features.aspect_ratio)); }
                if features.magnetic_anomaly_nt.unwrap_or(0.0) > 20.0 { reasons.push("magnetic anomaly detected".to_string()); }
            }
            AnomalyClass::Geological => {
                if features.vegetation_fraction > 0.2 { reasons.push("vegetation present".to_string()); }
                if features.depth_m.unwrap_or(10.0) < 3.0 { reasons.push("very shallow".to_string()); }
                reasons.push("highly persistent (natural feature)".to_string());
            }
            AnomalyClass::Vessel => {
                if features.temporal_persistence < 0.3 { reasons.push("not persistent (moved between passes)".to_string()); }
                if features.distance_to_shipping_lane_m.unwrap_or(f32::MAX) < 5000.0 { reasons.push("near shipping lane".to_string()); }
            }
            _ => {}
        }

        if reasons.is_empty() {
            format!("Classified as {} (score: {:.2})", class.label(), scores.get(class).unwrap_or(&0.0))
        } else {
            format!("{}: {}", class.label(), reasons.join(", "))
        }
    }

    /// Determine recommended action based on classification and confidence.
    fn recommend_action(&self, class: &AnomalyClass, confidence: f32) -> RecommendedAction {
        match class {
            AnomalyClass::Wreck => {
                if confidence > 0.8 { RecommendedAction::AddToDatabase }
                else { RecommendedAction::FieldVerification }
            }
            AnomalyClass::DebrisField => RecommendedAction::FieldVerification,
            AnomalyClass::Geological => RecommendedAction::Dismiss,
            AnomalyClass::Vessel => RecommendedAction::IgnoreVessel,
            AnomalyClass::Unknown => RecommendedAction::AcquireMoreImagery,
        }
    }
}

/// Normalize a value to 0.0-1.0 range given expected min/max.
fn normalize_score(value: f32, expected_min: f32, expected_max: f32) -> f32 {
    ((value - expected_min) / (expected_max - expected_min)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_obvious_wreck() {
        let classifier = WreckClassifier::new(ClassifierConfig::default());

        let features = AnomalyFeatures {
            temporal_score: 5.0,
            curvelet_response: 3.0,
            metal_fraction: 0.35,
            vegetation_fraction: 0.0,
            depth_m: Some(15.0),
            area_pixels: 200,
            aspect_ratio: 3.5,
            temporal_persistence: 0.9,
            distance_to_shipping_lane_m: Some(2000.0),
            magnetic_anomaly_nt: Some(80.0),
            sar_coherence_loss: Some(0.6),
        };

        let result = classifier.classify(&features);
        assert_eq!(result.class, AnomalyClass::Wreck);
        assert!(result.confidence > 0.6, "Should be high confidence: {}", result.confidence);
    }

    #[test]
    fn test_classify_geological() {
        let classifier = WreckClassifier::new(ClassifierConfig::default());

        let features = AnomalyFeatures {
            temporal_score: 2.0,
            curvelet_response: 0.5,
            metal_fraction: 0.0,
            vegetation_fraction: 0.4,
            depth_m: Some(1.5),
            area_pixels: 500,
            aspect_ratio: 1.2,
            temporal_persistence: 0.95,
            distance_to_shipping_lane_m: None,
            magnetic_anomaly_nt: None,
            sar_coherence_loss: None,
        };

        let result = classifier.classify(&features);
        assert_eq!(result.class, AnomalyClass::Geological);
    }

    #[test]
    fn test_classify_vessel() {
        let classifier = WreckClassifier::new(ClassifierConfig::default());

        let features = AnomalyFeatures {
            temporal_score: 4.0,
            curvelet_response: 2.0,
            metal_fraction: 0.3,
            vegetation_fraction: 0.0,
            depth_m: None,
            area_pixels: 100,
            aspect_ratio: 4.0,
            temporal_persistence: 0.1, // appeared once, gone next pass
            distance_to_shipping_lane_m: Some(500.0),
            magnetic_anomaly_nt: None,
            sar_coherence_loss: None,
        };

        let result = classifier.classify(&features);
        assert_eq!(result.class, AnomalyClass::Vessel);
    }
}
