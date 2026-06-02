//! Core data types and tunable knobs for the BAG wreck/redaction scanner.
//!
//! Ported from the Python reference pipeline at `pipelines/bag/`:
//!   * `bag_wreck_detector.py`        -> BAGInfo, WreckDetection, ObjectType
//!   * `advanced_bag_scanner.py`      -> RedactionSignature, WreckCandidate, default config
//!   * `masking_scanner.py`           -> MaskedRegion
//!   * `standalone_bag_scanner.py`    -> anomaly height knobs
//!
//! # Output contract (CRITICAL)
//!
//! `pipelines/bag/wreckhunter2000/validate_geo.py` and `validate_geo2.py` consume
//! the Rust detector output and branch on `signature_type`:
//!
//! ```python
//! if item["type"] == "physical_wreck":        ...
//! elif item["type"] == "masked_redaction_flat": ...
//! ```
//!
//! and read `r.size_sq_feet`, `r.latitude`, `r.longitude`. Therefore every
//! [`WreckDetection`] MUST serialize `signature_type`, `size_sq_feet`,
//! `latitude`, and `longitude`, and `signature_type` MUST be one of the two
//! literals defined in [`SignatureType`].

use serde::{Deserialize, Serialize};

/// Meters -> feet (matches masking_scanner.py `M_TO_FT = 3.28084`).
pub const M_TO_FT: f64 = 3.28084;
/// Square meters -> square feet.
pub const SQM_TO_SQFT: f64 = M_TO_FT * M_TO_FT;
/// BAG NoData sentinel (`BAGReader.NODATA = 1_000_000.0`).
pub const NODATA: f64 = 1_000_000.0;

/// The two `signature_type` string literals the Python validators branch on.
///
/// These strings are the public output contract — do not rename them.
pub mod signature_type {
    /// A real protruding object (wreck/debris/obstruction) found in the
    /// elevation channel. From [`crate::anomaly`].
    pub const PHYSICAL_WRECK: &str = "physical_wreck";
    /// A region whose bathymetry was deliberately flattened / masked / redacted.
    /// From [`crate::redaction_unmask`].
    pub const MASKED_REDACTION_FLAT: &str = "masked_redaction_flat";
}

/// Object classification, ported from `bag_wreck_detector.py::ObjectType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectType {
    Wreck,
    Debris,
    Obstruction,
    Unknown,
}

impl ObjectType {
    /// Size-based classification matching `_cluster_to_detection`:
    ///   size_ft >= 50 -> WRECK, >= 15 -> DEBRIS, else OBSTRUCTION.
    pub fn from_size_feet(size_ft: f64) -> Self {
        if size_ft >= 50.0 {
            ObjectType::Wreck
        } else if size_ft >= 15.0 {
            ObjectType::Debris
        } else {
            ObjectType::Obstruction
        }
    }
}

/// Pipeline stages A..G. A subset can be selected from the CLI via `--stages`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    /// A: open the BAG, read bands, build the elevation/uncertainty grids.
    Read,
    /// B: WGS84 reprojection set-up (geo transformer).
    Geo,
    /// C: physical anomaly (wreck) detection.
    Anomaly,
    /// D: redaction / masking signature detection (the marquee IP).
    Redaction,
    /// E: PCA orientation (heading/length/width/compass).
    Orientation,
    /// F: spatial deduplication.
    Dedup,
    /// G: assemble + emit the MissionReport.
    Report,
}

impl Stage {
    /// The full ordered pipeline A->G.
    pub fn all() -> Vec<Stage> {
        vec![
            Stage::Read,
            Stage::Geo,
            Stage::Anomaly,
            Stage::Redaction,
            Stage::Orientation,
            Stage::Dedup,
            Stage::Report,
        ]
    }

    /// Parse a single stage token (used by the CLI `--stages a,c,d` form).
    pub fn parse_token(tok: &str) -> Option<Stage> {
        match tok.trim().to_ascii_lowercase().as_str() {
            "a" | "read" => Some(Stage::Read),
            "b" | "geo" => Some(Stage::Geo),
            "c" | "anomaly" => Some(Stage::Anomaly),
            "d" | "redaction" => Some(Stage::Redaction),
            "e" | "orientation" => Some(Stage::Orientation),
            "f" | "dedup" => Some(Stage::Dedup),
            "g" | "report" => Some(Stage::Report),
            _ => None,
        }
    }
}

/// All tunable parameters, gathered from the Python defaults.
///
/// Sources for each default are cited inline. Serde gives every field a default
/// so a partial `--knobs '{"min_height_m": 1.0}'` JSON merges cleanly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Knobs {
    // ── Anomaly detector (bag_wreck_detector.py / standalone_bag_scanner.py) ──
    /// Z-score threshold for global anomaly gate
    /// (`advanced_bag_scanner` `anomaly_threshold=2.5`,
    ///  `standalone` uses `2.0 * global_std`).
    pub anomaly_threshold: f64,
    /// Multi-resolution scan skip factors (`advanced` `skip_pattern=[5,10,15]`).
    pub skip_pattern: Vec<usize>,
    /// Minimum height above seafloor to count as an anomaly, meters
    /// (`AnomalyDetector.min_height_m = 1.8`).
    pub min_height_m: f64,
    /// Minimum cells in a cluster (`AnomalyDetector.min_cluster_cells = 8`).
    pub min_cluster_cells: usize,
    /// Spatial clustering radius, meters (`AnomalyDetector.cluster_radius_m = 100`).
    pub cluster_radius_m: f64,
    /// Maximum bounding-box aspect ratio (`AnomalyDetector.MAX_ASPECT_RATIO = 6.0`).
    pub max_aspect_ratio: f64,

    // ── Size gates ──
    /// Minimum wreck size, sq ft (`advanced` `min_wreck_size_sq_ft = 25`).
    pub min_wreck_size_sq_ft: f64,
    /// Maximum wreck size, sq ft (`advanced` `max_wreck_size_sq_ft = 50000`).
    pub max_wreck_size_sq_ft: f64,
    /// Minimum long-side length, ft (`MIN_LONG_SIDE_FT = 36`).
    pub min_long_side_ft: f64,
    /// Minimum short-side length, ft (`MIN_SHORT_SIDE_FT = 10`).
    pub min_short_side_ft: f64,

    // ── Confidence / geo gates ──
    /// Minimum confidence to keep a candidate (`advanced` `min_confidence = 0.3`).
    pub min_confidence: f64,
    /// Great Lakes bounding box (`AnomalyDetector.GL_*`).
    pub gl_lat_min: f64,
    pub gl_lat_max: f64,
    pub gl_lon_min: f64,
    pub gl_lon_max: f64,
    /// Enforce the Great-Lakes bbox filter. Disabled outside the lakes domain.
    pub enforce_great_lakes_bbox: bool,

    // ── Redaction / masking ──
    /// Redaction signature sensitivity 0..1 (`advanced` `redaction_sensitivity = 0.7`).
    pub redaction_sensitivity: f64,
    /// Run the heavy elevation-only signature detectors
    /// (`advanced` `enable_redaction_signatures = false`).
    pub enable_redaction_signatures: bool,
    /// Percentile (of valid uncertainty) used to threshold masked regions.
    /// `detect_masked_regions` / `calc_orientation.py` use ~p5/p4.
    pub mask_uncertainty_pct: f64,
    /// Edge erosion (px) for masked-region interiors (`detect_masked_regions`).
    pub mask_erosion_px: usize,
    /// Enable fused rescoring for masked regions:
    ///   Route 2 (TPU boundary step), Route 1 proxy (curvelet-like edge coherence),
    ///   Route 3 (raw Band2 ghost contrast).
    pub enable_tpu_fusion: bool,
    /// Weight for TPU boundary discontinuity score (Route 2).
    pub tpu_boundary_weight: f64,
    /// Weight for curvelet-like coherence score (Route 1 proxy).
    pub curvelet_proxy_weight: f64,
    /// Weight for Band2 ghost-contrast score (Route 3).
    pub band2_ghost_weight: f64,
    /// Radius in cells used for boundary-ring sampling around masked regions.
    pub tpu_ring_px: usize,

    // ── Unmask reconstruction (visual rebuild of hidden surface) ──
    /// Margin (cells) around a masked region's bbox for the reconstruction window.
    pub unmask_margin_px: usize,
    /// IDW distance power for the baseline fill (2.0 = inverse-square).
    pub unmask_idw_power: f64,
    /// Cap on donor points sampled for IDW (keeps cost bounded on big windows).
    pub unmask_max_donors: usize,
    /// Apply uncertainty-guided relief on top of the IDW baseline.
    pub unmask_uncertainty_relief: bool,
    /// Relief budget as a multiple of donor-depth std (meters at 1 sigma).
    pub unmask_relief_gain: f64,
    /// Hillshade sun azimuth (deg from north, clockwise).
    pub unmask_sun_az_deg: f64,
    /// Hillshade sun altitude (deg above horizon).
    pub unmask_sun_alt_deg: f64,
    /// Max long-side (ft) of a region to attempt unmask reconstruction on.
    /// Larger regions are coverage-edge / whole-tile artifacts, not hides.
    pub unmask_max_region_ft: f64,
    /// Probe threshold: (tpu_boundary + band2_ghost) / 2 must exceed this
    /// for auto-reconstruction. Below this = skip (or flag for optional rebuild).
    pub unmask_probe_threshold: f64,
    /// Depth-anomaly threshold (ft): if a region's restored depth anomaly
    /// exceeds this absolute value, treat as object-evidence regardless of
    /// the uncertainty probe score.
    pub unmask_anomaly_threshold_ft: f64,
    /// Force reconstruction of ALL masked regions (including oversized /
    /// probe-failing). Set via `--knobs '{"unmask_force_all":true}'`.
    pub unmask_force_all: bool,

    // ── Dedup ──
    /// Spatial dedup merge radius, meters (`SpatialDeduplicator.merge_radius_m = 200`).
    pub merge_radius_m: f64,

    // ── IO ──
    /// NoData sentinel value (`BAGReader.NODATA = 1_000_000`).
    pub nodata: f64,
    /// Max cells before downsampling on read (`BAGReader.MAX_CELLS = 10_000_000`).
    pub max_cells: usize,
}

impl Default for Knobs {
    fn default() -> Self {
        Knobs {
            anomaly_threshold: 2.5,
            skip_pattern: vec![5, 10, 15],
            min_height_m: 1.8,
            min_cluster_cells: 8,
            cluster_radius_m: 100.0,
            max_aspect_ratio: 6.0,

            min_wreck_size_sq_ft: 25.0,
            max_wreck_size_sq_ft: 50_000.0,
            min_long_side_ft: 36.0,
            min_short_side_ft: 10.0,

            min_confidence: 0.45,
            gl_lat_min: 41.3,
            gl_lat_max: 49.0,
            gl_lon_min: -92.2,
            gl_lon_max: -76.0,
            enforce_great_lakes_bbox: true,

            redaction_sensitivity: 0.82,
            enable_redaction_signatures: false,
            mask_uncertainty_pct: 4.0,
            mask_erosion_px: 3,
            enable_tpu_fusion: true,
            tpu_boundary_weight: 0.50,
            curvelet_proxy_weight: 0.15,
            band2_ghost_weight: 0.35,
            tpu_ring_px: 4,

            unmask_margin_px: 30,
            unmask_idw_power: 2.0,
            unmask_max_donors: 512,
            unmask_uncertainty_relief: true,
            unmask_relief_gain: 1.0,
            unmask_sun_az_deg: 315.0,
            unmask_sun_alt_deg: 45.0,
            unmask_max_region_ft: 2000.0,
            unmask_probe_threshold: 0.25,
            unmask_anomaly_threshold_ft: 3.0,
            unmask_force_all: false,

            merge_radius_m: 200.0,

            nodata: NODATA,
            max_cells: 10_000_000,
        }
    }
}

impl Knobs {
    /// Merge a partial JSON object onto the defaults. Unknown keys are ignored
    /// by serde(default); provided keys override.
    pub fn from_json_overlay(json: &str) -> Result<Knobs, serde_json::Error> {
        // serde(default) means any omitted field falls back to Default.
        serde_json::from_str(json)
    }
}

/// Georeferencing info extracted from a BAG file.
/// Ported from `bag_wreck_detector.py::BAGInfo`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BagInfo {
    pub filepath: String,
    pub survey_id: String,
    /// (rows, cols) after any read-time downsampling.
    pub shape: (usize, usize),
    /// Southwest corner easting/northing (projected meters).
    pub sw_easting: f64,
    pub sw_northing: f64,
    pub ne_easting: f64,
    pub ne_northing: f64,
    /// Cell size, meters (scaled if downsampled on read).
    pub resolution_m: f64,
    /// CRS WKT string (from the dataset projection).
    pub crs_wkt: String,
    pub epsg_code: i32,
    pub vertical_datum: String,
    pub nodata_value: f64,
    pub valid_cell_count: usize,
    pub total_cell_count: usize,
    pub depth_min: f64,
    pub depth_max: f64,
    /// True if a usable uncertainty band (band 2) was present.
    pub has_uncertainty: bool,
    /// Downsample factor applied at read time (1 = none).
    pub read_step: usize,
}

/// A detected redaction signature.
/// Ported from `advanced_bag_scanner.py::RedactionSignature`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionSignature {
    /// 'smoothing' | 'removal' | 'alteration' | 'pattern_overlay'
    pub signature_type: String,
    pub confidence: f64,
    /// (lat, lon) in WGS84.
    pub location: (f64, f64),
    /// (min_lon, min_lat, max_lon, max_lat).
    pub bounding_box: (f64, f64, f64, f64),
    pub size_pixels: usize,
    pub size_meters_sq: f64,
    /// Assigned by `_identify_redactors`.
    pub redactor_id: Option<String>,
    pub technique_used: String,
    /// Free-form evidence dictionary.
    pub evidence: serde_json::Value,
}

/// A detected masked/redacted area.
/// Ported from `masking_scanner.py::MaskedRegion`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaskedRegion {
    pub id: String,
    pub bag_file: String,
    pub survey_id: String,
    /// "nan_hole" | "flattened" | "texture_break"
    pub mask_type: String,
    pub center_lat: f64,
    pub center_lon: f64,
    pub center_row: usize,
    pub center_col: usize,
    pub bbox_sw_lat: f64,
    pub bbox_sw_lon: f64,
    pub bbox_ne_lat: f64,
    pub bbox_ne_lon: f64,
    pub bbox_row_min: usize,
    pub bbox_row_max: usize,
    pub bbox_col_min: usize,
    pub bbox_col_max: usize,
    pub long_side_ft: f64,
    pub short_side_ft: f64,
    pub area_sq_ft: f64,
    pub cell_count: usize,
    pub surrounding_depth_ft: f64,
    pub depth_variance_ft: f64,
    pub restored_depth_ft: f64,
    pub depth_anomaly_ft: f64,
    pub confidence: f64,
    /// Route 2 fused score component.
    pub tpu_boundary_score: f64,
    /// Route 1 proxy score component.
    pub curvelet_proxy_score: f64,
    /// Route 3 fused score component.
    pub band2_ghost_score: f64,
    pub resolution_ft: f64,
    pub epsg: i32,
}

/// Intermediate physical-anomaly candidate before final contract conversion.
/// Mirrors fields used across `advanced_bag_scanner.py::WreckCandidate` and
/// `standalone_bag_scanner.py` detection dicts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WreckCandidate {
    pub center_row: usize,
    pub center_col: usize,
    pub easting: f64,
    pub northing: f64,
    pub latitude: f64,
    pub longitude: f64,
    pub depth_meters: f64,
    pub height_above_floor_m: f64,
    pub size_meters: f64,
    pub size_sq_meters: f64,
    pub size_sq_feet: f64,
    pub long_side_ft: f64,
    pub short_side_ft: f64,
    pub length_m: f64,
    pub width_m: f64,
    pub aspect_ratio: f64,
    pub cell_count: usize,
    pub confidence: f64,
    pub object_type: ObjectType,
    /// PCA heading (deg from north, clockwise), filled by orientation stage.
    pub heading_deg: f64,
    /// 180-degree ambiguity partner of `heading_deg`.
    pub heading_alt_deg: f64,
}

/// The final, contract-bearing detection emitted in the MissionReport.
///
/// Field names mirror what `validate_geo*.py` reads off the Rust result objects:
/// `signature_type`, `size_sq_feet`, `latitude`, `longitude`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WreckDetection {
    pub id: String,
    /// CONTRACT: "physical_wreck" or "masked_redaction_flat".
    pub signature_type: String,
    pub latitude: f64,
    pub longitude: f64,
    pub easting: f64,
    pub northing: f64,
    /// CONTRACT: size in square feet (validate_geo reads `r.size_sq_feet`).
    pub size_sq_feet: f64,
    pub size_meters: f64,
    pub depth_meters: f64,
    pub height_above_floor_m: f64,
    pub long_side_ft: f64,
    pub short_side_ft: f64,
    pub confidence: f64,
    pub object_type: ObjectType,
    pub heading_deg: f64,
    pub heading_alt_deg: f64,
    pub cell_count: usize,
    pub bag_file: String,
    pub survey_id: String,
    /// Extra provenance (redaction technique, mask type, merge count, etc.).
    pub metadata: serde_json::Value,
}

/// Top-level JSON output document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionReport {
    pub file: String,
    pub grid_size: (usize, usize),
    pub resolution_m: f64,
    pub epsg_code: i32,
    pub nodata_pct: f64,
    pub stages_run: Vec<Stage>,
    pub knobs: Knobs,
    /// All detections (physical + masked) carrying the `signature_type` contract.
    pub detections: Vec<WreckDetection>,
    /// Raw redaction signatures (diagnostic; only when enabled).
    pub redaction_signatures: Vec<RedactionSignature>,
    /// Counts for quick validation.
    pub physical_wreck_count: usize,
    pub masked_redaction_count: usize,
    pub process_time_ms: u128,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knobs_defaults_match_strict_profile() {
        let k = Knobs::default();
        assert_eq!(k.anomaly_threshold, 2.5);
        assert_eq!(k.skip_pattern, vec![5, 10, 15]);
        assert_eq!(k.min_height_m, 1.8);
        assert_eq!(k.min_cluster_cells, 8);
        assert_eq!(k.max_aspect_ratio, 6.0);
        assert_eq!(k.min_wreck_size_sq_ft, 25.0);
        assert_eq!(k.max_wreck_size_sq_ft, 50_000.0);
        assert_eq!(k.min_long_side_ft, 36.0);
        assert_eq!(k.min_short_side_ft, 10.0);
        assert_eq!(k.redaction_sensitivity, 0.82);
        assert_eq!(k.min_confidence, 0.45);
        assert_eq!(k.mask_uncertainty_pct, 4.0);
        assert_eq!(k.mask_erosion_px, 3);
        assert_eq!(k.tpu_boundary_weight, 0.50);
        assert_eq!(k.curvelet_proxy_weight, 0.15);
        assert_eq!(k.band2_ghost_weight, 0.35);
        assert_eq!(k.tpu_ring_px, 4);
        assert!(!k.enable_redaction_signatures);
        assert_eq!(k.merge_radius_m, 200.0);
        assert_eq!(k.nodata, 1_000_000.0);
    }

    #[test]
    fn knobs_json_overlay_merges_onto_defaults() {
        let k = Knobs::from_json_overlay(r#"{"min_height_m": 1.0, "merge_radius_m": 50}"#).unwrap();
        assert_eq!(k.min_height_m, 1.0); // overridden
        assert_eq!(k.merge_radius_m, 50.0); // overridden
        assert_eq!(k.anomaly_threshold, 2.5); // default preserved
    }

    #[test]
    fn object_type_size_thresholds() {
        assert_eq!(ObjectType::from_size_feet(60.0), ObjectType::Wreck);
        assert_eq!(ObjectType::from_size_feet(20.0), ObjectType::Debris);
        assert_eq!(ObjectType::from_size_feet(5.0), ObjectType::Obstruction);
    }

    #[test]
    fn signature_type_contract_literals() {
        // These exact strings are what validate_geo*.py branch on.
        assert_eq!(signature_type::PHYSICAL_WRECK, "physical_wreck");
        assert_eq!(signature_type::MASKED_REDACTION_FLAT, "masked_redaction_flat");
    }

    #[test]
    fn stage_parsing() {
        assert_eq!(Stage::parse_token("a"), Some(Stage::Read));
        assert_eq!(Stage::parse_token("redaction"), Some(Stage::Redaction));
        assert_eq!(Stage::parse_token("zzz"), None);
        assert_eq!(Stage::all().len(), 7);
    }
}
