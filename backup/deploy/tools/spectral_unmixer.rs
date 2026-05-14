//! Spectral Unmixer — Separates mixed pixel signatures into pure endmember fractions.
//!
//! A single 10m Sentinel-2 pixel over water may contain:
//! - Water (absorption in NIR/SWIR)
//! - Sediment (high reflectance in red/NIR)
//! - Metal/paint (specular peaks in specific bands)
//! - Vegetation (red edge, high NIR)
//! - Oil/hydrocarbon (absorption in SWIR)
//!
//! This module performs linear spectral unmixing to determine what fraction of each
//! endmember is present in every pixel. The output is a set of fraction maps —
//! one per endmember — that feed into the detection pipeline.
//!
//! For wreck detection: high metal fraction + low vegetation + underwater context = candidate.
//! For aircraft search: high paint fraction matching target color profile = candidate.

/// Spectral band identifiers matching Sentinel-2 / Landsat HLS bands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Band {
    Blue,       // B02 ~490nm
    Green,      // B03 ~560nm
    Red,        // B04 ~665nm
    RedEdge1,   // B05 ~705nm
    RedEdge2,   // B06 ~740nm
    RedEdge3,   // B07 ~783nm
    NIR,        // B08 ~842nm
    NIRNarrow,  // B08A ~865nm
    SWIR1,      // B11 ~1610nm
    SWIR2,      // B12 ~2190nm
}

impl Band {
    /// Central wavelength in nanometers.
    pub fn wavelength_nm(&self) -> f32 {
        match self {
            Self::Blue => 490.0,
            Self::Green => 560.0,
            Self::Red => 665.0,
            Self::RedEdge1 => 705.0,
            Self::RedEdge2 => 740.0,
            Self::RedEdge3 => 783.0,
            Self::NIR => 842.0,
            Self::NIRNarrow => 865.0,
            Self::SWIR1 => 1610.0,
            Self::SWIR2 => 2190.0,
        }
    }
}

/// A spectral endmember — the pure signature of a material across bands.
#[derive(Clone, Debug)]
pub struct Endmember {
    pub name: String,
    /// Reflectance values for each band (same order as bands in UnmixConfig)
    pub spectrum: Vec<f32>,
}

/// Configuration for the unmixing operation.
#[derive(Clone, Debug)]
pub struct UnmixConfig {
    /// Which bands are available in the input data
    pub bands: Vec<Band>,
    /// Library of endmembers to unmix against
    pub endmembers: Vec<Endmember>,
    /// Enforce sum-to-one constraint (fractions must sum to 1.0)
    pub sum_to_one: bool,
    /// Enforce non-negativity (no negative fractions)
    pub non_negative: bool,
    /// Minimum fraction to report (below this = zero)
    pub min_fraction: f32,
}

impl Default for UnmixConfig {
    fn default() -> Self {
        Self {
            bands: vec![Band::Blue, Band::Green, Band::Red, Band::NIR, Band::SWIR1, Band::SWIR2],
            endmembers: default_endmember_library(),
            sum_to_one: true,
            non_negative: true,
            min_fraction: 0.01,
        }
    }
}

/// Result of unmixing a single pixel.
#[derive(Clone, Debug)]
pub struct UnmixResult {
    /// Fraction of each endmember (same order as config.endmembers)
    pub fractions: Vec<f32>,
    /// Residual error (lower = better fit)
    pub rmse: f32,
}

/// Result of unmixing an entire tile.
pub struct TileUnmixResult {
    pub width: usize,
    pub height: usize,
    /// One fraction map per endmember, each width×height pixels
    pub fraction_maps: Vec<Vec<f32>>,
    /// RMSE map showing fit quality per pixel
    pub rmse_map: Vec<f32>,
    /// Endmember names (for labeling)
    pub endmember_names: Vec<String>,
}

/// The spectral unmixer.
pub struct SpectralUnmixer {
    pub config: UnmixConfig,
    /// Pre-computed endmember matrix (n_bands × n_endmembers) for fast unmixing
    endmember_matrix: Vec<f32>,
    /// Pre-computed (E^T × E)^-1 × E^T for unconstrained least squares
    unmix_matrix: Vec<f32>,
    n_bands: usize,
    n_endmembers: usize,
}

impl SpectralUnmixer {
    /// Create a new unmixer with the given configuration.
    /// Pre-computes the unmixing matrix for fast per-pixel operation.
    pub fn new(config: UnmixConfig) -> Self {
        let n_bands = config.bands.len();
        let n_endmembers = config.endmembers.len();

        // Build endmember matrix E [n_bands × n_endmembers]
        let mut e_matrix = vec![0.0f32; n_bands * n_endmembers];
        for (j, em) in config.endmembers.iter().enumerate() {
            for i in 0..n_bands {
                e_matrix[i * n_endmembers + j] = em.spectrum[i.min(em.spectrum.len() - 1)];
            }
        }

        // Compute pseudo-inverse: (E^T × E)^-1 × E^T
        // This gives us the unconstrained least-squares solution matrix
        let unmix_matrix = compute_pseudoinverse(&e_matrix, n_bands, n_endmembers);

        Self {
            config,
            endmember_matrix: e_matrix,
            unmix_matrix,
            n_bands,
            n_endmembers,
        }
    }

    /// Unmix a single pixel (multi-band reflectance values).
    pub fn unmix_pixel(&self, pixel_bands: &[f32]) -> UnmixResult {
        assert!(pixel_bands.len() >= self.n_bands);

        // Unconstrained solution: fractions = unmix_matrix × pixel
        let mut fractions = vec![0.0f32; self.n_endmembers];
        for j in 0..self.n_endmembers {
            let mut sum = 0.0f32;
            for i in 0..self.n_bands {
                sum += self.unmix_matrix[j * self.n_bands + i] * pixel_bands[i];
            }
            fractions[j] = sum;
        }

        // Apply constraints
        if self.config.non_negative {
            for f in fractions.iter_mut() {
                if *f < 0.0 { *f = 0.0; }
            }
        }

        if self.config.sum_to_one {
            let sum: f32 = fractions.iter().sum();
            if sum > 0.0 {
                for f in fractions.iter_mut() {
                    *f /= sum;
                }
            }
        }

        // Apply minimum threshold
        for f in fractions.iter_mut() {
            if *f < self.config.min_fraction {
                *f = 0.0;
            }
        }

        // Calculate RMSE (reconstruction error)
        let rmse = self.reconstruction_error(pixel_bands, &fractions);

        UnmixResult { fractions, rmse }
    }

    /// Unmix an entire tile. Input: one Vec<f32> per band, each width×height pixels.
    pub fn unmix_tile(&self, band_data: &[Vec<f32>], width: usize, height: usize) -> TileUnmixResult {
        assert_eq!(band_data.len(), self.n_bands);
        let n_pixels = width * height;

        let mut fraction_maps: Vec<Vec<f32>> = (0..self.n_endmembers)
            .map(|_| vec![0.0f32; n_pixels])
            .collect();
        let mut rmse_map = vec![0.0f32; n_pixels];

        // Process each pixel
        let mut pixel_buf = vec![0.0f32; self.n_bands];
        for px in 0..n_pixels {
            // Gather bands for this pixel
            for b in 0..self.n_bands {
                pixel_buf[b] = band_data[b][px];
            }

            let result = self.unmix_pixel(&pixel_buf);

            // Scatter fractions into maps
            for (j, &frac) in result.fractions.iter().enumerate() {
                fraction_maps[j][px] = frac;
            }
            rmse_map[px] = result.rmse;
        }

        TileUnmixResult {
            width,
            height,
            fraction_maps,
            rmse_map,
            endmember_names: self.config.endmembers.iter().map(|e| e.name.clone()).collect(),
        }
    }

    /// Get the fraction map for a specific endmember by name.
    pub fn get_fraction_map<'a>(&self, result: &'a TileUnmixResult, name: &str) -> Option<&'a [f32]> {
        result.endmember_names.iter()
            .position(|n| n == name)
            .map(|idx| result.fraction_maps[idx].as_slice())
    }

    /// Calculate reconstruction error (RMSE) for a pixel given fractions.
    fn reconstruction_error(&self, observed: &[f32], fractions: &[f32]) -> f32 {
        let mut sum_sq = 0.0f32;
        for i in 0..self.n_bands {
            let mut reconstructed = 0.0f32;
            for j in 0..self.n_endmembers {
                reconstructed += self.endmember_matrix[i * self.n_endmembers + j] * fractions[j];
            }
            let diff = observed[i] - reconstructed;
            sum_sq += diff * diff;
        }
        (sum_sq / self.n_bands as f32).sqrt()
    }
}

// ─── Default Endmember Library ───────────────────────────────────────────────

/// Default endmember library for maritime/freshwater SAR.
/// Reflectance values are approximate and should be calibrated per region.
/// Order: [Blue, Green, Red, NIR, SWIR1, SWIR2]
fn default_endmember_library() -> Vec<Endmember> {
    vec![
        Endmember {
            name: "deep_water".to_string(),
            // Water absorbs strongly in NIR/SWIR, reflects slightly in blue/green
            spectrum: vec![0.04, 0.03, 0.02, 0.005, 0.001, 0.001],
        },
        Endmember {
            name: "shallow_water_sand".to_string(),
            // Shallow water over sand — some bottom reflectance in visible
            spectrum: vec![0.06, 0.07, 0.06, 0.02, 0.005, 0.002],
        },
        Endmember {
            name: "sediment_plume".to_string(),
            // Turbid water with suspended sediment — high in red/NIR
            spectrum: vec![0.08, 0.10, 0.12, 0.06, 0.02, 0.01],
        },
        Endmember {
            name: "vegetation".to_string(),
            // Classic vegetation signature — red edge, high NIR
            spectrum: vec![0.03, 0.06, 0.03, 0.40, 0.20, 0.08],
        },
        Endmember {
            name: "metal_rust".to_string(),
            // Rusted iron/steel — reddish, moderate NIR, low SWIR
            spectrum: vec![0.05, 0.06, 0.12, 0.15, 0.10, 0.06],
        },
        Endmember {
            name: "metal_clean".to_string(),
            // Clean aluminum/steel — high specular, relatively flat spectrum
            spectrum: vec![0.20, 0.22, 0.23, 0.25, 0.20, 0.18],
        },
        Endmember {
            name: "paint_white".to_string(),
            // White paint — high reflectance across visible, drops in SWIR
            spectrum: vec![0.70, 0.72, 0.71, 0.65, 0.30, 0.15],
        },
        Endmember {
            name: "paint_red".to_string(),
            // Red paint — low blue/green, high red, moderate NIR
            spectrum: vec![0.05, 0.05, 0.35, 0.20, 0.10, 0.05],
        },
        Endmember {
            name: "oil_slick".to_string(),
            // Oil/hydrocarbon on water — absorption in SWIR, slight sheen in visible
            spectrum: vec![0.06, 0.05, 0.04, 0.03, 0.008, 0.004],
        },
        Endmember {
            name: "concrete_rock".to_string(),
            // Concrete/rock — moderate flat reflectance
            spectrum: vec![0.15, 0.16, 0.17, 0.18, 0.20, 0.18],
        },
    ]
}

// ─── Linear Algebra Helpers ──────────────────────────────────────────────────

/// Compute the Moore-Penrose pseudo-inverse: (E^T × E)^-1 × E^T
/// Input: E [n_rows × n_cols], Output: E_pinv [n_cols × n_rows]
fn compute_pseudoinverse(e: &[f32], n_rows: usize, n_cols: usize) -> Vec<f32> {
    // E^T × E [n_cols × n_cols]
    let mut ete = vec![0.0f32; n_cols * n_cols];
    for i in 0..n_cols {
        for j in 0..n_cols {
            let mut sum = 0.0f32;
            for k in 0..n_rows {
                sum += e[k * n_cols + i] * e[k * n_cols + j];
            }
            ete[i * n_cols + j] = sum;
        }
    }

    // Invert (E^T × E) using Gauss-Jordan elimination
    let mut inv = vec![0.0f32; n_cols * n_cols];
    for i in 0..n_cols {
        inv[i * n_cols + i] = 1.0; // identity
    }

    let mut aug = ete.clone();
    for col in 0..n_cols {
        // Find pivot
        let mut max_row = col;
        let mut max_val = aug[col * n_cols + col].abs();
        for row in (col + 1)..n_cols {
            let val = aug[row * n_cols + col].abs();
            if val > max_val {
                max_val = val;
                max_row = row;
            }
        }

        // Swap rows
        if max_row != col {
            for k in 0..n_cols {
                aug.swap(col * n_cols + k, max_row * n_cols + k);
                inv.swap(col * n_cols + k, max_row * n_cols + k);
            }
        }

        let pivot = aug[col * n_cols + col];
        if pivot.abs() < 1e-10 {
            // Singular — add regularization
            aug[col * n_cols + col] = 1e-6;
            continue;
        }

        // Scale pivot row
        for k in 0..n_cols {
            aug[col * n_cols + k] /= pivot;
            inv[col * n_cols + k] /= pivot;
        }

        // Eliminate column
        for row in 0..n_cols {
            if row == col { continue; }
            let factor = aug[row * n_cols + col];
            for k in 0..n_cols {
                aug[row * n_cols + k] -= factor * aug[col * n_cols + k];
                inv[row * n_cols + k] -= factor * inv[col * n_cols + k];
            }
        }
    }

    // Now compute (E^T × E)^-1 × E^T [n_cols × n_rows]
    let mut pinv = vec![0.0f32; n_cols * n_rows];
    for i in 0..n_cols {
        for j in 0..n_rows {
            let mut sum = 0.0f32;
            for k in 0..n_cols {
                sum += inv[i * n_cols + k] * e[j * n_cols + k];
            }
            pinv[i * n_rows + j] = sum;
        }
    }

    pinv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unmix_pure_water() {
        let unmixer = SpectralUnmixer::new(UnmixConfig::default());

        // Pure deep water pixel
        let pixel = vec![0.04, 0.03, 0.02, 0.005, 0.001, 0.001];
        let result = unmixer.unmix_pixel(&pixel);

        // Should be mostly deep_water
        assert!(result.fractions[0] > 0.5, "Deep water fraction should dominate: {:?}", result.fractions);
        assert!(result.rmse < 0.05, "RMSE should be low for pure endmember: {}", result.rmse);
    }

    #[test]
    fn test_unmix_metal_in_water() {
        let unmixer = SpectralUnmixer::new(UnmixConfig::default());

        // Mixed pixel: 70% water + 30% rusted metal (submerged wreck)
        let water = &[0.04f32, 0.03, 0.02, 0.005, 0.001, 0.001];
        let metal = &[0.05f32, 0.06, 0.12, 0.15, 0.10, 0.06];
        let pixel: Vec<f32> = (0..6).map(|i| water[i] * 0.7 + metal[i] * 0.3).collect();

        let result = unmixer.unmix_pixel(&pixel);

        // Should detect both water and metal
        let water_frac = result.fractions[0]; // deep_water
        let metal_frac = result.fractions[4]; // metal_rust
        assert!(water_frac > 0.3, "Should detect water: {}", water_frac);
        assert!(metal_frac > 0.1, "Should detect metal: {}", metal_frac);
    }

    #[test]
    fn test_unmix_tile() {
        let unmixer = SpectralUnmixer::new(UnmixConfig::default());

        // 4x4 tile, 6 bands, all deep water
        let n_pixels = 16;
        let band_data: Vec<Vec<f32>> = vec![
            vec![0.04; n_pixels], // Blue
            vec![0.03; n_pixels], // Green
            vec![0.02; n_pixels], // Red
            vec![0.005; n_pixels], // NIR
            vec![0.001; n_pixels], // SWIR1
            vec![0.001; n_pixels], // SWIR2
        ];

        let result = unmixer.unmix_tile(&band_data, 4, 4);
        assert_eq!(result.fraction_maps.len(), 10); // 10 endmembers
        assert_eq!(result.fraction_maps[0].len(), 16); // 4x4 pixels

        // All pixels should be mostly water
        let water_map = unmixer.get_fraction_map(&result, "deep_water").unwrap();
        for &frac in water_map {
            assert!(frac > 0.3, "All pixels should have water fraction > 0.3");
        }
    }
}
