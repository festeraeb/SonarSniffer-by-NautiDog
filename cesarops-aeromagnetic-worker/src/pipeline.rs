//! Full detection pipeline: adaptive → dipole (GPU) → curvelet — device-agnostic.

use crate::adaptive::run_adaptive;
use crate::curvelet::{extract_window, score_window_f32};
use crate::discriminator::{cross_reference_candidate, CandidateMatch, KnownWreck, Wellhead};
use crate::dipole_analysis::{analyze_candidate, DipoleAnalysisInput};
use crate::geo::GridMeta;
use crate::gpu::{dipole_scan_grid_try, OutPixel, Params};
use crate::knobs::DetectionLevels;
use crate::scoring::{apply_basin_scoring, evaluate_disposition, BasinScoringInput};
use serde::Serialize;
use std::time::Instant;

#[derive(Debug, Clone, Serialize)]
pub struct DetectedCandidate {
    pub label_id: u32,
    pub center_lat: f64,
    pub center_lon: f64,
    pub score: f32,
    pub adaptive_score: f32,
    pub dipole_score: f32,
    pub curvelet_energy_ratio: f64,
    pub is_dipolar_pull: bool,
    pub lobe_symmetry_ratio: Option<f32>,
    pub dipole_separation_m: f32,
    pub combined_score: f32,
    // ── CPU dipole discriminator (ports pipelines/mag/dipole_analysis.py::analyze_candidate) ──
    pub cpu_is_dipolar: bool,
    pub cpu_lobe_ratio: Option<f32>,
    pub cpu_dipole_separation_m: Option<f32>,
    pub flip_dist_min_m: Option<f32>,
    pub grad_contrast: Option<f32>,
    pub aspect_ratio: Option<f32>,
    /// Long-axis azimuth (deg, 0–180) from dipole_analysis.py PCA.
    pub elongation_azimuth_deg: Option<f32>,
    /// 0–100 man-made likelihood score from the CPU discriminator.
    pub score_manmade: f32,
    pub classification: String,
    // ── Discriminator cross-reference (ports erie_wellhead_discriminator.py) ──
    pub ground_truth: String,
    pub ground_truth_name: String,
    pub well_distance_m: Option<f64>,
    pub nearest_wellhead: Option<String>,
    pub wreck_distance_m: Option<f64>,
    pub nearest_known_wreck: Option<String>,
    pub discriminator_bonus: f64,
    // ── Basin-aware composite scoring (erie_scanner_pipeline.py::_apply_basin_scoring) ──
    /// Lake Erie sub-basin the candidate falls in ("western"/"central"/"eastern"),
    /// or empty when outside all basins.
    pub basin: String,
    /// Human-readable basin/disposition score-adjustment reasons.
    pub basin_reasons: Vec<String>,
    // ── Disposition false-positive filter (mag_data_pipeline.py::stage_cross_reference) ──
    /// True when the nearest matched wreck was raised/scrapped/etc. or tagged a
    /// geological false positive, so the magnetic signature is likely geological.
    pub likely_false_positive: bool,
    pub false_positive_reason: Option<String>,
    pub backends: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PipelineReport {
    pub width: u32,
    pub height: u32,
    pub pixel_size_m: f32,
    pub elapsed_ms: f64,
    pub stages_ms: std::collections::HashMap<String, f64>,
    pub gpu_available: bool,
    pub candidates: Vec<DetectedCandidate>,
}

/// Fuse a base ranking score with the CPU man-made dipole score (0–100).
///
/// Ports the fusion in `pipelines/mag/erie_central_aeromag_orchestrator.py`:
///   `combined = base*(1 - weight) + (manmade/100 * 10) * weight`
pub fn fuse_combined_score(base: f32, score_manmade: f32, dipole_score_weight: f32) -> f32 {
    base * (1.0 - dipole_score_weight) + (score_manmade / 100.0 * 10.0) * dipole_score_weight
}

/// Merge candidates within `merge_radius_m`, keeping the best-scoring one.
///
/// Ports the location-dedupe in `erie_central_aeromag_orchestrator.py`:
///   `if any(haversine_m(...) < merge_m for r in merged): continue`
///
/// The input is assumed sorted by `combined_score` descending so the greedy
/// best-first walk keeps the strongest candidate in each cluster. Distance uses
/// the same haversine metric as the discriminator cross-reference.
pub fn merge_candidates(
    candidates: Vec<DetectedCandidate>,
    merge_radius_m: f64,
) -> Vec<DetectedCandidate> {
    use crate::discriminator::haversine_m;
    if merge_radius_m <= 0.0 {
        return candidates;
    }
    let mut kept: Vec<DetectedCandidate> = Vec::with_capacity(candidates.len());
    for c in candidates {
        let too_close = kept.iter().any(|k| {
            haversine_m(c.center_lat, c.center_lon, k.center_lat, k.center_lon) < merge_radius_m
        });
        if too_close {
            continue;
        }
        kept.push(c);
    }
    kept
}

pub async fn run_detection_pipeline(
    grid: &[f32],
    width: u32,
    height: u32,
    pixel_size_m: f32,
    meta: &GridMeta,
    levels: &DetectionLevels,
    wells: &[Wellhead],
    wrecks: &[KnownWreck],
) -> PipelineReport {
    let t0 = Instant::now();
    let mut stages_ms = std::collections::HashMap::new();
    let mut lv = levels.for_pixel_size_m(pixel_size_m);

    let t1 = Instant::now();
    let adaptive = run_adaptive(grid, width, height, pixel_size_m, meta, &lv);
    stages_ms.insert("adaptive".into(), t1.elapsed().as_secs_f64() * 1000.0);

    let (inner_px, outer_px) = lv.inner_outer_px(pixel_size_m);
    let dipole_params = Params {
        width,
        height,
        inner_radius: inner_px,
        outer_radius: outer_px,
        pixel_size_m,
    };

    let t2 = Instant::now();
    let (dipole_map, gpu_ok) = match dipole_scan_grid_try(grid, dipole_params).await {
        Ok(m) => (m, true),
        Err(e) => {
            log::warn!("WGPU dipole unavailable ({e}) — curvelet+adaptive only");
            (
                vec![
                    OutPixel {
                        bg_mean: 0.0,
                        peak_pos: 0.0,
                        peak_neg: 0.0,
                        dipole_separation_m: 0.0,
                        score: 0.0,
                    };
                    (width * height) as usize
                ],
                false,
            )
        }
    };
    stages_ms.insert("dipole_gpu".into(), t2.elapsed().as_secs_f64() * 1000.0);
    if !gpu_ok {
        lv.require_dipolar_pull = false;
    }

    let t3 = Instant::now();
    let mut final_cands = Vec::new();
    let cwin = lv.curvelet_window_px;
    let scales = lv.curvelet_num_scales;

    // CPU discriminator analysis radii — port of INNER_YD=2000 / OUTER_YD=5000
    // from pipelines/mag/dipole_analysis.py (the core anomaly region and the
    // background annulus). These are intentionally larger than the GPU shader's
    // dipole_inner/outer_yards used for the fast lobe score.
    let yards_to_px = |yd: f32| -> usize {
        let m = yd / 1.0936133;
        (m / pixel_size_m).round().max(3.0) as usize
    };
    let cpu_inner_px = yards_to_px(2000.0);
    let cpu_outer_px = yards_to_px(5000.0).max(cpu_inner_px + 2);

    for (i, ac) in adaptive.iter().enumerate() {
        let idx = (ac.row * width + ac.col) as usize;
        let dp = dipole_map.get(idx).copied().unwrap_or(OutPixel {
            bg_mean: 0.0,
            peak_pos: 0.0,
            peak_neg: 0.0,
            dipole_separation_m: 0.0,
            score: 0.0,
        });

        let gpu_is_pull = dp.peak_pos > 0.05 && dp.peak_neg < -0.05;
        let lobe_ratio = if dp.peak_pos > 0.0 && dp.peak_neg < 0.0 {
            Some(dp.peak_pos.min(dp.peak_neg.abs()) / dp.peak_pos.max(dp.peak_neg.abs()))
        } else {
            None
        };

        // ── CPU dipole discriminator (pipelines/mag/dipole_analysis.py::analyze_candidate) ──
        // Run the rich CPU analysis on the grid window around the candidate to
        // obtain polarity-flip distance, gradient contrast, aspect ratio and the
        // 0–100 man-made score that the thin GPU lobe score cannot provide.
        let cpu = analyze_candidate(&DipoleAnalysisInput {
            grid,
            rows: height as usize,
            cols: width as usize,
            pixel_x_m: pixel_size_m as f64,
            pixel_y_m: pixel_size_m as f64,
            center_row: ac.row as usize,
            center_col: ac.col as usize,
            inner_radius_px: cpu_inner_px,
            outer_radius_px: cpu_outer_px,
        });

        // Fix #2: prefer the CPU analysis `is_dipolar` (relative 0.15*peak_abs
        // rule from dipole_analysis.py) for the dipolar-pull gate when available,
        // falling back to the GPU fixed-nT test only if the CPU analysis is None.
        let is_pull = match &cpu {
            Some(c) => c.is_dipolar,
            None => gpu_is_pull,
        };

        if lv.require_dipolar_pull && !is_pull {
            continue;
        }
        if let Some(lr) = lobe_ratio {
            if lr < lv.min_lobe_ratio {
                continue;
            }
        }
        if dp.score < lv.dipole_min_score {
            continue;
        }

        let patch = extract_window(grid, width, height, ac.row, ac.col, cwin);
        let cs = score_window_f32(&patch, cwin as usize, cwin as usize, scales);
        let curvelet_boost = if cs.energy_ratio >= lv.curvelet_energy_threshold as f64 {
            cs.energy_ratio as f32
        } else {
            0.0
        };

        // Base score = adaptive fused with curvelet (Python orchestrator "score").
        let w_d = 1.0 - lv.curvelet_score_weight;
        let base = ac.score * w_d + curvelet_boost * lv.curvelet_score_weight;

        // ── Fuse the CPU man-made score into the ranking ──
        // Ports erie_central_aeromag_orchestrator.py:
        //   combined = base*(1-dipole_score_weight) + (manmade/100*10)*dipole_score_weight
        let score_manmade = cpu.as_ref().map(|c| c.score_manmade as f32).unwrap_or(0.0);

        // ── Man-made score gate (erie_central_aeromag_orchestrator.py) ──
        // The orchestrator drops candidates whose CPU man-made score is below
        // `min_dipole_manmade_score` (default 20.0). Apply only when the CPU
        // analysis ran (Python reaches this gate only on a successful analysis);
        // candidates without CPU analysis keep the prior fall-through behaviour.
        if cpu.is_some() && score_manmade < lv.min_dipole_manmade_score {
            continue;
        }

        let combined = fuse_combined_score(base, score_manmade, lv.dipole_score_weight);

        let mut backends = vec!["adaptive-cpu".into()];
        if gpu_ok {
            backends.push("dipole-wgpu".into());
        }
        if cpu.is_some() {
            backends.push("dipole-cpu".into());
        }
        backends.push(cs.backend.to_string());

        // ── Discriminator cross-reference (erie_wellhead_discriminator.py) ──
        // Active when wells/wrecks are provided; applies the Loran-C warp and
        // the wellhead/wreck match radii from the knobs.
        let mut xref = CandidateMatch {
            label_id: (i + 1) as u32,
            center_lat: ac.center_lat,
            center_lon: ac.center_lon,
            dipole_score: dp.score,
            dipole_verdict: cpu
                .as_ref()
                .map(|c| c.classification.to_string())
                .unwrap_or_default(),
            ground_truth: String::new(),
            ground_truth_name: String::new(),
            well_distance_m: None,
            nearest_wellhead: None,
            wreck_distance_m: None,
            nearest_known_wreck: None,
            nearest_wreck_salvage_status: String::new(),
            nearest_wreck_magnetic_potential: String::new(),
            bonus_score: 0.0,
            curvelet_energy_ratio: Some(cs.energy_ratio as f32),
        };
        cross_reference_candidate(
            &mut xref,
            wells,
            wrecks,
            lv.wellhead_radius_m,
            lv.wreck_radius_m,
            lv.apply_loran_correction,
        );

        // ── Basin-aware composite scoring (erie_scanner_pipeline.py::_apply_basin_scoring) ──
        // Multiplicative adjustments using the candidate's well/wreck distances
        // (from the cross-reference above) and the CPU dipolar classification
        // computed in Wave 1.
        let cpu_is_dipolar = cpu.as_ref().map(|c| c.is_dipolar).unwrap_or(is_pull);
        let amplitude_peak_abs = cpu.as_ref().map(|c| c.peak_abs).unwrap_or(0.0);
        let basin = crate::scoring::identify_basin(ac.center_lat, ac.center_lon)
            .unwrap_or("")
            .to_string();
        let (basin_score, mut basin_reasons) = apply_basin_scoring(
            combined as f64,
            &BasinScoringInput {
                center_lat: ac.center_lat,
                center_lon: ac.center_lon,
                wellhead_distance_m: xref.well_distance_m,
                wreck_distance_m: xref.wreck_distance_m,
                is_dipolar: cpu_is_dipolar,
                amplitude_peak_abs,
                nearest_known_wreck: xref.nearest_known_wreck.clone(),
            },
        );

        // ── Disposition false-positive filter (mag_data_pipeline.py::stage_cross_reference) ──
        // If the candidate matched a known wreck (within the wreck radius) whose
        // disposition means it is no longer on the bottom, down-rank it.
        let wreck_matched = xref
            .wreck_distance_m
            .map(|d| d <= lv.wreck_radius_m)
            .unwrap_or(false);
        let disposition = evaluate_disposition(
            wreck_matched,
            xref.nearest_known_wreck.as_deref().unwrap_or(""),
            &xref.nearest_wreck_salvage_status,
            &xref.nearest_wreck_magnetic_potential,
        );
        let final_score = basin_score * disposition.score_multiplier;
        if let Some(reason) = &disposition.reason {
            basin_reasons.push(format!("false-positive: {reason}"));
        }
        let final_score_f32 = final_score as f32;

        final_cands.push(DetectedCandidate {
            label_id: (i + 1) as u32,
            center_lat: ac.center_lat,
            center_lon: ac.center_lon,
            score: final_score_f32,
            adaptive_score: ac.score,
            dipole_score: dp.score,
            curvelet_energy_ratio: cs.energy_ratio,
            is_dipolar_pull: is_pull,
            lobe_symmetry_ratio: lobe_ratio,
            dipole_separation_m: dp.dipole_separation_m,
            combined_score: final_score_f32,
            cpu_is_dipolar: cpu.as_ref().map(|c| c.is_dipolar).unwrap_or(false),
            cpu_lobe_ratio: cpu.as_ref().and_then(|c| c.lobe_ratio).map(|v| v as f32),
            cpu_dipole_separation_m: cpu
                .as_ref()
                .and_then(|c| c.dipole_separation_m)
                .map(|v| v as f32),
            flip_dist_min_m: cpu.as_ref().and_then(|c| c.flip_dist_min_m).map(|v| v as f32),
            grad_contrast: cpu.as_ref().and_then(|c| c.grad_contrast).map(|v| v as f32),
            aspect_ratio: cpu.as_ref().and_then(|c| c.aspect_ratio).map(|v| v as f32),
            elongation_azimuth_deg: cpu
                .as_ref()
                .and_then(|c| c.elongation_azimuth_deg)
                .map(|v| v as f32),
            score_manmade,
            classification: cpu
                .as_ref()
                .map(|c| c.classification.to_string())
                .unwrap_or_else(|| "UNKNOWN".to_string()),
            ground_truth: xref.ground_truth,
            ground_truth_name: xref.ground_truth_name,
            well_distance_m: xref.well_distance_m,
            nearest_wellhead: xref.nearest_wellhead,
            wreck_distance_m: xref.wreck_distance_m,
            nearest_known_wreck: xref.nearest_known_wreck,
            discriminator_bonus: xref.bonus_score,
            basin,
            basin_reasons,
            likely_false_positive: disposition.likely_false_positive,
            false_positive_reason: disposition.reason,
            backends,
        });
    }

    final_cands.sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap());
    // ── Candidate merge / NMS (erie_central_aeromag_orchestrator.py) ──
    // Collapse candidates that fall within dipole_merge_radius_m of a
    // higher-scoring one (greedy, best-first), then truncate to top_n.
    final_cands = merge_candidates(final_cands, lv.dipole_merge_radius_m);
    final_cands.truncate(lv.top_n);
    stages_ms.insert("curvelet".into(), t3.elapsed().as_secs_f64() * 1000.0);

    PipelineReport {
        width,
        height,
        pixel_size_m,
        elapsed_ms: t0.elapsed().as_secs_f64() * 1000.0,
        stages_ms,
        gpu_available: gpu_ok,
        candidates: final_cands,
    }
}

pub fn write_candidates_csv(path: &std::path::Path, source_grid: &str, cands: &[DetectedCandidate]) {
    let mut w = String::from(
        "source_grid,label_id,center_lat,center_lon,score,adaptive_score,dipole_score,curvelet_energy_ratio,is_dipolar_pull,lobe_symmetry_ratio,dipole_separation_m,dipole_manmade_score,classification,ground_truth,ground_truth_name,well_distance_m,wreck_distance_m,pull_combined_score,dipole_verdict,dipole_backend\n",
    );
    for c in cands {
        let lr = c
            .lobe_symmetry_ratio
            .map(|v| format!("{v:.3}"))
            .unwrap_or_default();
        let well_d = c.well_distance_m.map(|v| format!("{v:.1}")).unwrap_or_default();
        let wreck_d = c.wreck_distance_m.map(|v| format!("{v:.1}")).unwrap_or_default();
        w.push_str(&format!(
            "{},{},{:.6},{:.6},{:.4},{:.4},{:.4},{:.4},{},{},{:.1},{:.1},{},{},{},{},{},{:.4},RUST_PIPELINE,wgpu+cpu+fdct\n",
            source_grid,
            c.label_id,
            c.center_lat,
            c.center_lon,
            c.combined_score,
            c.adaptive_score,
            c.dipole_score,
            c.curvelet_energy_ratio,
            if c.is_dipolar_pull { "1" } else { "0" },
            lr,
            c.dipole_separation_m,
            c.score_manmade,
            c.classification,
            c.ground_truth,
            c.ground_truth_name,
            well_d,
            wreck_d,
            c.combined_score,
        ));
    }
    std::fs::write(path, w).expect("write csv");
}

// ── Pipeline integration tests ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::GridMeta;
    use crate::knobs::DetectionLevels;

    const N: u32 = 96;

    /// Minimal `DetectedCandidate` builder for merge/NMS tests.
    fn mk_candidate(label: u32, lat: f64, lon: f64, score: f32) -> DetectedCandidate {
        DetectedCandidate {
            label_id: label,
            center_lat: lat,
            center_lon: lon,
            score,
            adaptive_score: score,
            dipole_score: 0.0,
            curvelet_energy_ratio: 0.0,
            is_dipolar_pull: false,
            lobe_symmetry_ratio: None,
            dipole_separation_m: 0.0,
            combined_score: score,
            cpu_is_dipolar: false,
            cpu_lobe_ratio: None,
            cpu_dipole_separation_m: None,
            flip_dist_min_m: None,
            grad_contrast: None,
            aspect_ratio: None,
            elongation_azimuth_deg: None,
            score_manmade: 0.0,
            classification: "AMBIGUOUS".to_string(),
            ground_truth: String::new(),
            ground_truth_name: String::new(),
            well_distance_m: None,
            nearest_wellhead: None,
            wreck_distance_m: None,
            nearest_known_wreck: None,
            discriminator_bonus: 0.0,
            basin: String::new(),
            basin_reasons: Vec::new(),
            likely_false_positive: false,
            false_positive_reason: None,
            backends: Vec::new(),
        }
    }

    /// Candidate merge / NMS (erie_central_aeromag_orchestrator.py dedupe):
    /// candidates within `dipole_merge_radius_m` of a higher-scoring one are
    /// dropped; far-apart candidates are all kept.
    #[test]
    fn test_merge_candidates_nms() {
        // Two clusters near Lake Erie. A & B are ~200 m apart (should merge);
        // C is ~5 km away (should survive).
        // 0.0018 deg lat ≈ 200 m; 0.05 deg lon ≈ ~4 km at this latitude.
        let cands = vec![
            mk_candidate(1, 42.5000, -80.0000, 9.0), // best in cluster 1
            mk_candidate(2, 42.5018, -80.0000, 8.0), // ~200 m from #1 → merged away
            mk_candidate(3, 42.5000, -79.9500, 7.0), // ~4 km east → kept
        ];
        // Input is sorted by score descending (as in the pipeline).
        let merged = merge_candidates(cands, 2500.0);
        assert_eq!(merged.len(), 2, "the ~200 m duplicate must be merged out");
        // The best-scoring candidate of cluster 1 is kept.
        assert_eq!(merged[0].label_id, 1, "best-scoring candidate kept");
        assert!(
            merged.iter().any(|c| c.label_id == 3),
            "the far-apart candidate must survive"
        );
        assert!(
            !merged.iter().any(|c| c.label_id == 2),
            "the close, lower-scoring duplicate must be dropped"
        );
    }

    /// A zero/negative merge radius disables NMS (keeps everything).
    #[test]
    fn test_merge_candidates_disabled() {
        let cands = vec![
            mk_candidate(1, 42.5000, -80.0000, 9.0),
            mk_candidate(2, 42.5000, -80.0000, 8.0),
        ];
        let merged = merge_candidates(cands, 0.0);
        assert_eq!(merged.len(), 2, "zero radius must keep all candidates");
    }

    /// Build a small N×N grid with a synthetic dipole near the centre.
    /// Kept small so the integration test stays fast.
    fn synthetic_dipole_grid() -> Vec<f32> {
        let n = N as usize;
        let mut grid = vec![50_000.0f32; n * n];
        let cy = (N / 2) as i32;
        let cx = (N / 2) as i32;
        for (dy, dx, sign) in [(-6i32, 0, 1.0f32), (6, 0, -1.0)] {
            for r in -4i32..=4 {
                for c in -4i32..=4 {
                    if r * r + c * c > 16 {
                        continue;
                    }
                    let y = (cy + dy + r) as usize;
                    let x = (cx + dx + c) as usize;
                    if y < n && x < n {
                        grid[y * n + x] += sign * 180.0;
                    }
                }
            }
        }
        grid
    }

    fn permissive_levels() -> DetectionLevels {
        let mut lv = DetectionLevels::default();
        // Permissive gates so the synthetic target survives without a GPU.
        lv.z_thresh = 0.15;
        lv.edge_z_thresh = 0.15;
        lv.min_pixels = 1;
        lv.dipole_min_score = 0.0;
        lv.require_dipolar_pull = false;
        // Keep the candidate set (and therefore CPU analysis cost) small.
        lv.top_n = 16;
        // Small curvelet window keeps the FDCT cheap on the tiny grid.
        lv.curvelet_window_px = 32;
        lv
    }

    fn demo_meta() -> GridMeta {
        GridMeta {
            width: N,
            height: N,
            transform: [0.01, 0.0, -82.0, 0.0, -0.01, 42.0],
            source_tif: Some("demo.tif".into()),
        }
    }

    /// The CPU discriminator `analyze_candidate` must actually run inside the
    /// pipeline (it used to be dead code). We prove invocation by checking that
    /// at least one ranked candidate carries a non-zero man-made score and a
    /// real classification (the `UNKNOWN` sentinel is only used when the CPU
    /// analysis returned `None`).
    #[tokio::test]
    async fn test_analyze_candidate_invoked_in_pipeline() {
        let grid = synthetic_dipole_grid();
        let lv = permissive_levels();
        let meta = demo_meta();

        // Large pixel size so the CPU analysis radii (2000yd inner / 5000yd
        // outer, from dipole_analysis.py) fit inside the small test grid.
        let report =
            run_detection_pipeline(&grid, N, N, 100.0, &meta, &lv, &[], &[]).await;

        assert!(
            !report.candidates.is_empty(),
            "pipeline should detect the synthetic dipole"
        );
        let invoked = report
            .candidates
            .iter()
            .any(|c| c.classification != "UNKNOWN");
        assert!(
            invoked,
            "analyze_candidate must be invoked: every candidate has the UNKNOWN sentinel"
        );
        let scored = report
            .candidates
            .iter()
            .any(|c| c.score_manmade > 0.0 && c.cpu_is_dipolar);
        assert!(
            scored,
            "CPU discriminator should report a dipolar, man-made-scoring candidate"
        );
    }

    /// `score_manmade` must influence the fused `combined_score` ranking, using
    /// the exact orchestrator formula from erie_central_aeromag_orchestrator.py.
    #[test]
    fn test_score_manmade_influences_combined_ordering() {
        let weight = 0.5f32;
        let base = 5.0f32;

        let low = fuse_combined_score(base, 10.0, weight);
        let high = fuse_combined_score(base, 90.0, weight);
        assert!(
            high > low,
            "higher man-made score must produce a higher combined score (low={low}, high={high})"
        );

        // Verify the exact Python fusion: base*(1-w) + (manmade/100*10)*w
        let expected = base * (1.0 - weight) + (90.0 / 100.0 * 10.0) * weight;
        assert!(
            (high - expected).abs() < 1e-4,
            "combined score must match the orchestrator fusion formula"
        );

        // With zero weight the man-made score has no effect on ranking.
        let a = fuse_combined_score(base, 10.0, 0.0);
        let b = fuse_combined_score(base, 90.0, 0.0);
        assert!(
            (a - b).abs() < 1e-6,
            "with zero weight the man-made score must not change the result"
        );

        // Sorting two equal-base candidates must order by man-made score.
        let mut scores = vec![("low", low), ("high", high)];
        scores.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap());
        assert_eq!(
            scores[0].0, "high",
            "the higher man-made candidate must rank first"
        );
    }
}
