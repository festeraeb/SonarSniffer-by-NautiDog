//! Area-tunable detection thresholds only — hardware/runtime is auto-discovered.

use serde::{Deserialize, Serialize};

/// Detection sensitivity knobs (per basin / mission). No device or path settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionLevels {
    pub window_yards: f32,
    pub z_thresh: f32,
    pub edge_z_thresh: f32,
    pub min_pixels: u32,
    pub max_pixels: u32,
    pub top_n: usize,
    pub dipole_inner_yards: f32,
    pub dipole_outer_yards: f32,
    pub dipole_min_score: f32,
    pub require_dipolar_pull: bool,
    pub min_lobe_ratio: f32,
    pub curvelet_window_px: u32,
    pub curvelet_num_scales: usize,
    pub curvelet_energy_threshold: f32,
    pub curvelet_score_weight: f32,
    /// Weight of the CPU man-made dipole score in the fused ranking.
    /// Ports `dipole_score_weight` from erie_central_aeromag_orchestrator.py.
    #[serde(default = "default_dipole_score_weight")]
    pub dipole_score_weight: f32,
    /// Apply the Loran-C systematic warp before well/wreck cross-referencing.
    /// Ports `apply_loran_correction` from erie_central_aeromag_orchestrator.py.
    #[serde(default = "default_apply_loran_correction")]
    pub apply_loran_correction: bool,
    /// Match radius (m) for tagging a candidate as a suspected wellhead.
    /// Ports `wellhead_radius_m` from erie_central_aeromag_orchestrator.py.
    #[serde(default = "default_wellhead_radius_m")]
    pub wellhead_radius_m: f64,
    /// Match radius (m) for tagging a candidate as a known wreck.
    /// Ports `wreck_radius_m` from erie_wellhead_discriminator.py::cross_reference_candidates.
    #[serde(default = "default_wreck_radius_m")]
    pub wreck_radius_m: f64,
    #[serde(default)]
    pub use_vertical_derivative: bool,
    /// Minimum CPU man-made dipole score (0–100) for a candidate to survive.
    /// Ports `min_dipole_manmade_score` from erie_central_aeromag_orchestrator.py.
    #[serde(default = "default_min_dipole_manmade_score")]
    pub min_dipole_manmade_score: f32,
    /// Merge/NMS radius (m): candidates within this distance are collapsed,
    /// keeping the best. Ports `dipole_merge_radius_m` from
    /// erie_central_aeromag_orchestrator.py.
    #[serde(default = "default_dipole_merge_radius_m")]
    pub dipole_merge_radius_m: f64,
    /// Target metres-per-pixel for scaling dipole windows. Ports
    /// `dipole_target_m_px` from erie_central_aeromag_orchestrator.py.
    #[serde(default = "default_dipole_target_m_px")]
    pub dipole_target_m_px: f32,
    /// Curvelet patch upscale factor. Ports `curvelet_patch_upscale` from
    /// erie_central_aeromag_orchestrator.py.
    #[serde(default = "default_curvelet_patch_upscale")]
    pub curvelet_patch_upscale: u32,
    /// Validation z-threshold. Ports `validation_z_thresh` from
    /// erie_central_aeromag_orchestrator.py.
    #[serde(default = "default_validation_z_thresh")]
    pub validation_z_thresh: f32,
    /// Auto-applied when pixel size exceeds this (meters).
    #[serde(default = "default_coarse_m_px")]
    pub coarse_grid_m_px_threshold: f32,
    #[serde(default)]
    pub coarse: Option<Box<DetectionLevels>>,
}

fn default_coarse_m_px() -> f32 {
    400.0
}

fn default_min_dipole_manmade_score() -> f32 {
    20.0
}

fn default_dipole_merge_radius_m() -> f64 {
    2500.0
}

fn default_dipole_target_m_px() -> f32 {
    100.0
}

fn default_curvelet_patch_upscale() -> u32 {
    1
}

fn default_validation_z_thresh() -> f32 {
    0.35
}

fn default_dipole_score_weight() -> f32 {
    0.5
}

fn default_apply_loran_correction() -> bool {
    true
}

fn default_wellhead_radius_m() -> f64 {
    2000.0
}

fn default_wreck_radius_m() -> f64 {
    5000.0
}

impl Default for DetectionLevels {
    fn default() -> Self {
        Self {
            window_yards: 750.0,
            z_thresh: 0.38,
            edge_z_thresh: 0.42,
            min_pixels: 4,
            max_pixels: 350,
            top_n: 500,
            dipole_inner_yards: 650.0,
            dipole_outer_yards: 1900.0,
            dipole_min_score: 0.35,
            require_dipolar_pull: true,
            min_lobe_ratio: 0.22,
            curvelet_window_px: 128,
            curvelet_num_scales: 6,
            curvelet_energy_threshold: 2.0,
            curvelet_score_weight: 0.45,
            dipole_score_weight: 0.5,
            apply_loran_correction: true,
            wellhead_radius_m: 2000.0,
            wreck_radius_m: 5000.0,
            use_vertical_derivative: false,
            min_dipole_manmade_score: 20.0,
            dipole_merge_radius_m: 2500.0,
            dipole_target_m_px: 100.0,
            curvelet_patch_upscale: 1,
            validation_z_thresh: 0.35,
            coarse_grid_m_px_threshold: 400.0,
            coarse: None,
        }
    }
}

impl DetectionLevels {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let t = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&t).map_err(|e| e.to_string())
    }

    pub fn for_pixel_size_m(&self, m_per_px: f32) -> DetectionLevels {
        if m_per_px > self.coarse_grid_m_px_threshold {
            if let Some(ref coarse) = self.coarse {
                return (**coarse).clone();
            }
        }
        self.clone()
    }

    pub fn inner_outer_px(&self, m_per_px: f32) -> (u32, u32) {
        let yards_to_px = |yd: f32| -> u32 {
            let m = yd / 1.0936133;
            (m / m_per_px).round().max(3.0) as u32
        };
        let inner = yards_to_px(self.dipole_inner_yards);
        let outer = yards_to_px(self.dipole_outer_yards).max(inner + 2);
        (inner, outer)
    }

    pub fn window_px(&self, m_per_px: f32) -> (u32, u32) {
        let m = self.window_yards / 1.0936133;
        let w = (m / m_per_px).round().max(3.0) as u32;
        let wy = if w % 2 == 0 { w + 1 } else { w };
        (wy, wy)
    }
}
