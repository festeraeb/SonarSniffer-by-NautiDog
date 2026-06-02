//! Stage orchestration A->G.
//!
//! Replaces the old monolithic loop in `main.rs`. Each stage is individually
//! selectable via the [`Stage`] enum so callers can run, e.g., only redaction
//! detection. The output is a [`MissionReport`] whose `detections` carry the
//! `signature_type` contract (`physical_wreck` / `masked_redaction_flat`) that
//! `validate_geo*.py` consumes.
//!
//! Stage map:
//!   A Read         — open BAG, read elevation + uncertainty, NoData->NaN
//!   B Geo          — build the WGS84 reprojector (gdal OSR)
//!   C Anomaly      — physical wreck detection (height above floor)
//!   D Redaction    — masking scanner + (optional) elevation signatures
//!   E Orientation  — PCA heading/length/width + compass bearing
//!   F Dedup        — spatial dedup merge
//!   G Report       — assemble + count + serialize

use crate::anomaly;
use crate::bag_io::{self, BagData};
use crate::dedup;
use crate::geo::GeoTransformer;
use crate::orientation;
use crate::redaction_unmask;
use crate::types::{
    signature_type, BagInfo, Knobs, MaskedRegion, MissionReport, ObjectType, RedactionSignature,
    Stage, WreckCandidate, WreckDetection,
};
use gdal::Dataset;
use std::time::Instant;
use tracing::{info, warn};

/// Run the selected stages over a BAG file and produce a [`MissionReport`].
pub fn run(
    path: &str,
    knobs: &Knobs,
    stages: &[Stage],
) -> Result<MissionReport, Box<dyn std::error::Error>> {
    run_with_unmask(path, knobs, stages, None)
}

/// As [`run`], but when `unmask_dir` is `Some`, reconstruct and export
/// georeferenced rasters (recon/diff/hillshade GeoTIFF) for each masked region.
pub fn run_with_unmask(
    path: &str,
    knobs: &Knobs,
    stages: &[Stage],
    unmask_dir: Option<&std::path::Path>,
) -> Result<MissionReport, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let want = |s: Stage| stages.contains(&s);

    // ── Stage A: Read ──
    let BagData {
        elevation,
        uncertainty,
        info,
    } = bag_io::read_bag(path, knobs)?;
    info!(
        "Read {}: {}x{} cells, {} valid, epsg={}",
        path, info.shape.0, info.shape.1, info.valid_cell_count, info.epsg_code
    );

    // ── Stage B: Geo ──
    // Pull the affine geo-transform straight from the dataset for exact mapping.
    let geo_transform = Dataset::open(path).ok().and_then(|d| d.geo_transform().ok());
    let geo = GeoTransformer::new(&info, geo_transform, info.read_step);
    if !geo.has_wgs84() {
        warn!("No WGS84 reprojection available (epsg={}); lat/lon will be projected coords", info.epsg_code);
    }

    let mut detections: Vec<WreckDetection> = Vec::new();
    let mut redaction_signatures: Vec<RedactionSignature> = Vec::new();

    // ── Stage C: Anomaly (physical wrecks) ──
    let mut anomaly_candidates: Vec<(WreckCandidate, Vec<(usize, usize)>)> = Vec::new();
    if want(Stage::Anomaly) {
        anomaly_candidates = anomaly::detect_with_pixels(&elevation, &info, &geo, knobs);
        info!("Anomaly stage: {} candidates", anomaly_candidates.len());
    }

    // ── Stage E: Orientation (annotate candidates) ──
    if want(Stage::Orientation) {
        for (cand, pixels) in anomaly_candidates.iter_mut() {
            orientation::annotate_candidate(cand, pixels, info.resolution_m);
        }
    }

    // Convert physical candidates -> contract detections.
    for (i, (cand, _)) in anomaly_candidates.iter().enumerate() {
        detections.push(physical_to_detection(cand, &info, i));
    }

    // ── Stage D: Redaction / masking ──
    if want(Stage::Redaction) {
        // Masking scanner (elevation-based): nan holes, flattened, texture breaks.
        let mut regions = redaction_unmask::detect_masking(&elevation, &info, &geo, knobs);

        // Uncertainty-based mask detection (bag_mesh.rs::detect_masked_regions).
        if let Some(ref uncert) = uncertainty {
            let umask = redaction_unmask::detect_masked_regions_uncertainty(uncert, &info, &geo, knobs);
            regions.extend(umask);
        }
        redaction_unmask::fusion_rescore_regions(&elevation, uncertainty.as_ref(), &mut regions, knobs);
        info!("Redaction stage: {} masked regions", regions.len());

        // Optional unmask reconstruction + georeferenced export.
        if let Some(dir) = unmask_dir {
            let recons = crate::unmask::unmask_regions(
                path,
                &elevation,
                uncertainty.as_ref(),
                &regions,
                &info,
                &geo,
                knobs,
                Some(dir),
            );
            let relief = recons.iter().filter(|r| r.relief_applied).count();
            info!(
                "Unmask: reconstructed {} regions ({} with uncertainty relief) -> {}",
                recons.len(),
                relief,
                dir.display()
            );
        }

        for (i, region) in regions.iter().enumerate() {
            detections.push(masked_region_to_detection(region, i));
        }

        // Optional heavy elevation signature detectors.
        if knobs.enable_redaction_signatures {
            redaction_signatures = redaction_unmask::analyze_redaction_signatures(
                &elevation,
                uncertainty.as_ref(),
                &geo,
                knobs,
            );
            info!(
                "Redaction signatures: {} (enabled)",
                redaction_signatures.len()
            );
        }
    }

    // ── Stage F: Dedup ──
    if want(Stage::Dedup) {
        detections = dedup::deduplicate(detections, knobs.merge_radius_m);
    }

    // ── Stage G: Report ──
    let physical_wreck_count = detections
        .iter()
        .filter(|d| d.signature_type == signature_type::PHYSICAL_WRECK)
        .count();
    let masked_redaction_count = detections
        .iter()
        .filter(|d| d.signature_type == signature_type::MASKED_REDACTION_FLAT)
        .count();

    let report = MissionReport {
        file: path.to_string(),
        grid_size: info.shape,
        resolution_m: info.resolution_m,
        epsg_code: info.epsg_code,
        nodata_pct: bag_io::nodata_pct(&elevation),
        stages_run: stages.to_vec(),
        knobs: knobs.clone(),
        detections,
        redaction_signatures,
        physical_wreck_count,
        masked_redaction_count,
        process_time_ms: start.elapsed().as_millis(),
    };

    Ok(report)
}

/// Convert a physical [`WreckCandidate`] into the contract [`WreckDetection`]
/// with `signature_type = "physical_wreck"`.
fn physical_to_detection(cand: &WreckCandidate, info: &BagInfo, index: usize) -> WreckDetection {
    WreckDetection {
        id: format!("{}_phys{:03}", info.survey_id, index),
        signature_type: signature_type::PHYSICAL_WRECK.to_string(),
        latitude: cand.latitude,
        longitude: cand.longitude,
        easting: cand.easting,
        northing: cand.northing,
        size_sq_feet: cand.size_sq_feet,
        size_meters: cand.size_meters,
        depth_meters: cand.depth_meters,
        height_above_floor_m: cand.height_above_floor_m,
        long_side_ft: cand.long_side_ft,
        short_side_ft: cand.short_side_ft,
        confidence: cand.confidence,
        object_type: cand.object_type,
        heading_deg: cand.heading_deg,
        heading_alt_deg: cand.heading_alt_deg,
        cell_count: cand.cell_count,
        bag_file: basename(&info.filepath),
        survey_id: info.survey_id.clone(),
        metadata: serde_json::json!({
            "aspect_ratio": cand.aspect_ratio,
            "length_m": cand.length_m,
            "width_m": cand.width_m,
            "size_sq_meters": cand.size_sq_meters,
            "center_row": cand.center_row,
            "center_col": cand.center_col,
        }),
    }
}

/// Convert a [`MaskedRegion`] into the contract [`WreckDetection`] with
/// `signature_type = "masked_redaction_flat"`.
fn masked_region_to_detection(region: &MaskedRegion, index: usize) -> WreckDetection {
    let _ = index;
    WreckDetection {
        id: region.id.clone(),
        signature_type: signature_type::MASKED_REDACTION_FLAT.to_string(),
        latitude: region.center_lat,
        longitude: region.center_lon,
        // MaskedRegion doesn't carry projected coords; dedup will fall back to
        // the lat/lon haversine path since easting==0.
        easting: 0.0,
        northing: 0.0,
        size_sq_feet: region.area_sq_ft,
        size_meters: region.long_side_ft / crate::types::M_TO_FT,
        depth_meters: region.surrounding_depth_ft.abs() / crate::types::M_TO_FT,
        height_above_floor_m: region.depth_anomaly_ft / crate::types::M_TO_FT,
        long_side_ft: region.long_side_ft,
        short_side_ft: region.short_side_ft,
        confidence: region.confidence,
        object_type: ObjectType::from_size_feet(region.long_side_ft),
        heading_deg: 0.0,
        heading_alt_deg: 0.0,
        cell_count: region.cell_count,
        bag_file: region.bag_file.clone(),
        survey_id: region.survey_id.clone(),
        metadata: serde_json::json!({
            "mask_type": region.mask_type,
            "restored_depth_ft": region.restored_depth_ft,
            "depth_anomaly_ft": region.depth_anomaly_ft,
            "surrounding_depth_ft": region.surrounding_depth_ft,
            "bbox_sw": [region.bbox_sw_lat, region.bbox_sw_lon],
            "bbox_ne": [region.bbox_ne_lat, region.bbox_ne_lon],
            "center_row": region.center_row,
            "center_col": region.center_col,
            "bbox_row_min": region.bbox_row_min,
            "bbox_row_max": region.bbox_row_max,
            "bbox_col_min": region.bbox_col_min,
            "bbox_col_max": region.bbox_col_max,
            "tpu_boundary_score": region.tpu_boundary_score,
            "curvelet_proxy_score": region.curvelet_proxy_score,
            "band2_ghost_score": region.band2_ghost_score,
        }),
    }
}

fn basename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}
