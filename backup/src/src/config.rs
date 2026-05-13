use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The high-level goal of the current mission.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DetectionMode {
    HydrocarbonSheen,   // SWIR dark absorption
    ThermalSink,        // Cold spot in thermal
    ClearWater,         // Unusually clear patch
    SedimentPlume,      // Post-storm turbidity
    SurfaceRipple,      // Persistent ripple
    Glint,              // Specular reflection
    Custom(String),     // AI-defined custom
}

/// Which spectral bands to use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Band {
    /// Sentinel-2 Band indices (1-12)
    Sentinel2(u8),
    /// Landsat-8/9 Band indices (1-11)
    Landsat(u8),
    /// Generic band index for VRT stacks
    Generic(u16),
}

/// How to combine primary and secondary bands.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BandOp {
    Ratio,          // primary / secondary
    Difference,     // primary - secondary
    Index,          // (p - s) / (p + s)
    Single,         // Just primary
    FalseColor,     // RGB composite (handled by slicer, not here)
}

/// Recipe for band math.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BandRecipe {
    pub primary: Band,
    pub secondary: Band,
    pub operation: BandOp,
    pub normalize: bool,
}

/// Sensitivity thresholds for detection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Thresholds {
    /// Minimum value to consider an anomaly (for Index/Diff)
    pub min_value: f32,
    /// Maximum value to consider an anomaly (for Ratio)
    pub max_value: f32,
    /// Confidence threshold (0.0 - 1.0)
    pub confidence: f32,
    /// Minimum area in pixels to filter noise
    pub min_area_px: usize,
}

/// Temporal stacking parameters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StackConfig {
    /// Number of recent tiles to stack for temporal stability
    pub stack_size: usize,
    /// Method: mean, median, or max
    pub method: StackMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StackMethod {
    Mean,
    Median,
    Max,
}

/// The complete configuration for a mission run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionConfig {
    pub mission_id: String,
    pub detection_mode: DetectionMode,
    pub band_recipe: BandRecipe,
    pub thresholds: Thresholds,
    pub stacking: StackConfig,
    /// Additional metadata for the worker to interpret results
    pub metadata: HashMap<String, String>,
}

impl MissionConfig {
    pub fn new(mission_id: String, mode: DetectionMode) -> Self {
        Self {
            mission_id,
            detection_mode: mode,
            band_recipe: BandRecipe::default(),
            thresholds: Thresholds::default(),
            stacking: StackConfig::default(),
            metadata: HashMap::new(),
        }
    }
}

impl Default for BandRecipe {
    fn default() -> Self {
        Self {
            primary: Band::Sentinel2(11), // SWIR 2
            secondary: Band::Sentinel2(8), // NIR
            operation: BandOp::Ratio,
            normalize: true,
        }
    }
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            min_value: -0.1,
            max_value: 0.5,
            confidence: 0.8,
            min_area_px: 5,
        }
    }
}

impl Default for StackConfig {
    fn default() -> Self {
        Self {
            stack_size: 3,
            method: StackMethod::Median,
        }
    }
}

/// Pipeline-specific error type. Uses enum dispatch to avoid trait objects.
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineError {
    DimensionMismatch,
    DivisionByZero,
    InvalidThreshold,
    IoError(String),
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineError::DimensionMismatch => write!(f, "Band dimensions mismatch"),
            PipelineError::DivisionByZero => write!(f, "Division by zero in band math"),
            PipelineError::InvalidThreshold => write!(f, "Invalid threshold configuration"),
            PipelineError::IoError(msg) => write!(f, "IO error: {}", msg),
        }
    }
}

impl std::error::Error for PipelineError {}
