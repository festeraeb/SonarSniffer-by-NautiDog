//! Colormap generation for perceptual enhancement.
//!
//! Provides several industry-standard and custom colormaps:
//! - **Viridis**: Perceptually uniform, colorblind-friendly
//! - **Magma**: High contrast variant of viridis
//! - **Jet**: Traditional rainbow (not perceptually linear)
//! - **SonarCustom**: Black→Blue→Cyan→Green→Yellow→White

use serde::{Deserialize, Serialize};

/// Colormap selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Colormap {
    Grayscale,
    Viridis,
    Magma,
    Jet,
    SonarCustom,
}

/// 256-entry RGB lookup table.
pub type ColorLUT = [(u8, u8, u8); 256];

impl Colormap {
    /// Generate the 256-entry color LUT.
    pub fn generate_lut(self) -> ColorLUT {
        match self {
            Colormap::Grayscale => generate_grayscale(),
            Colormap::Viridis => generate_viridis(),
            Colormap::Magma => generate_magma(),
            Colormap::Jet => generate_jet(),
            Colormap::SonarCustom => generate_sonar_custom(),
        }
    }
}

fn generate_grayscale() -> ColorLUT {
    let mut lut = [(0u8, 0u8, 0u8); 256];
    for (i, entry) in lut.iter_mut().enumerate() {
        let v = i as u8;
        *entry = (v, v, v);
    }
    lut
}

/// Viridis colormap (perceptually uniform, colorblind-friendly).
///
/// Key colors (approximate):
/// - 0: Dark purple (68, 1, 84)
/// - 64: Blue (59, 82, 139)
/// - 128: Teal (33, 145, 140)
/// - 192: Green (94, 201, 98)
/// - 255: Yellow (253, 231, 37)
fn generate_viridis() -> ColorLUT {
    const KEYPOINTS: [(u8, (u8, u8, u8)); 5] = [
        (0, (68, 1, 84)),
        (64, (59, 82, 139)),
        (128, (33, 145, 140)),
        (192, (94, 201, 98)),
        (255, (253, 231, 37)),
    ];
    interpolate_colormap(&KEYPOINTS)
}

/// Magma colormap (similar to viridis, higher contrast).
///
/// Key colors:
/// - 0: Black (0, 0, 4)
/// - 64: Purple (60, 15, 90)
/// - 128: Red (160, 55, 90)
/// - 192: Orange (240, 140, 70)
/// - 255: Light yellow (252, 253, 191)
fn generate_magma() -> ColorLUT {
    const KEYPOINTS: [(u8, (u8, u8, u8)); 5] = [
        (0, (0, 0, 4)),
        (64, (60, 15, 90)),
        (128, (160, 55, 90)),
        (192, (240, 140, 70)),
        (255, (252, 253, 191)),
    ];
    interpolate_colormap(&KEYPOINTS)
}

/// Jet colormap (traditional rainbow, not perceptually uniform).
///
/// Blue → Cyan → Green → Yellow → Red
fn generate_jet() -> ColorLUT {
    const KEYPOINTS: [(u8, (u8, u8, u8)); 6] = [
        (0, (0, 0, 128)),      // Dark blue
        (51, (0, 0, 255)),     // Blue
        (102, (0, 255, 255)),  // Cyan
        (153, (0, 255, 0)),    // Green
        (204, (255, 255, 0)),  // Yellow
        (255, (255, 0, 0)),    // Red
    ];
    interpolate_colormap(&KEYPOINTS)
}

/// Custom sonar colormap optimized for underwater acoustics.
///
/// Black (noise floor) → Blue → Cyan → Green → Yellow → Orange → White (strong return)
fn generate_sonar_custom() -> ColorLUT {
    const KEYPOINTS: [(u8, (u8, u8, u8)); 7] = [
        (0, (0, 0, 0)),        // Black (noise floor)
        (32, (0, 0, 128)),     // Dark blue
        (64, (0, 128, 255)),   // Cyan
        (128, (0, 255, 0)),    // Green
        (192, (255, 255, 0)),  // Yellow
        (224, (255, 128, 0)),  // Orange
        (255, (255, 255, 255)), // White (strong return)
    ];
    interpolate_colormap(&KEYPOINTS)
}

/// Linear interpolation between keypoints.
fn interpolate_colormap(keypoints: &[(u8, (u8, u8, u8))]) -> ColorLUT {
    let mut lut = [(0u8, 0u8, 0u8); 256];
    
    for i in 0..256 {
        let idx = i as u8;
        
        // Find bracketing keypoints
        let mut lower = keypoints[0];
        let mut upper = keypoints[keypoints.len() - 1];
        
        for j in 0..keypoints.len() - 1 {
            if idx >= keypoints[j].0 && idx <= keypoints[j + 1].0 {
                lower = keypoints[j];
                upper = keypoints[j + 1];
                break;
            }
        }
        
        // Linear interpolation
        if lower.0 == upper.0 {
            lut[i] = lower.1;
        } else {
            let t = (idx - lower.0) as f32 / (upper.0 - lower.0) as f32;
            let r = lerp_u8(lower.1.0, upper.1.0, t);
            let g = lerp_u8(lower.1.1, upper.1.1, t);
            let b = lerp_u8(lower.1.2, upper.1.2, t);
            lut[i] = (r, g, b);
        }
    }
    
    lut
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_grayscale() {
        let lut = Colormap::Grayscale.generate_lut();
        assert_eq!(lut[0], (0, 0, 0));
        assert_eq!(lut[128], (128, 128, 128));
        assert_eq!(lut[255], (255, 255, 255));
    }
    
    #[test]
    fn test_viridis_keypoints() {
        let lut = Colormap::Viridis.generate_lut();
        // Check approximate keypoint colors
        assert_eq!(lut[0], (68, 1, 84));
        assert_eq!(lut[64], (59, 82, 139));
        assert_eq!(lut[255], (253, 231, 37));
    }
    
    #[test]
    fn test_colormap_length() {
        for cm in &[
            Colormap::Grayscale,
            Colormap::Viridis,
            Colormap::Magma,
            Colormap::Jet,
            Colormap::SonarCustom,
        ] {
            let lut = cm.generate_lut();
            assert_eq!(lut.len(), 256);
        }
    }
}
