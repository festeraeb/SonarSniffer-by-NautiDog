# Aeromagnetic/Sonar Detection Improvement — Aspect Ratio Normalization

## Problem
At depth (300-500ft), the inverse cube law makes wrecks and wellheads look like identical blobs.
Need to distinguish:
- Shipwreck: elongated horizontal structure (length > width)
- Wellhead/pipes: vertical structures appearing as columns

## Solution: Aspect Ratio Normalization via Synthetic Grid

### Step 1: Calculate True Aspect Ratio
AR = Horizontal Swath Width / Vertical Beam Height
If sonar has 100m swath and 10m beam height, one horizontal pixel = 10x more distance than vertical.

### Step 2: Stretch the Synthetic Grid
Apply aspect ratio to grid dimensions before curvelet analysis.

```rust
pub struct AsymmetricTile {
    pub grid_data: Vec<f32>,
    pub aspect_ratio: f64,  // Horizontal / Vertical
}

impl AsymmetricTile {
    pub fn stretch_for_analysis(&self) -> Array2<f32> {
        // Scale horizontal dimension by aspect ratio
        // Makes vertical features appear "shorter" relative to horizontal
    }
}
```

### Step 3: Curvelet Directional Classification
After stretching to true physical proportions:
- Shipwrecks: strong horizontal curvelet coefficients
- Wellheads: strong vertical curvelet coefficients

```rust
pub fn classify_target(curvelet_coeffs: &CurveletTransform) -> TargetType {
    let horizontal_energy = curvelet_coeffs.horizontal_direction_energy();
    let vertical_energy = curvelet_coeffs.vertical_direction_energy();

    if horizontal_energy > vertical_energy * 2.0 {
        TargetType::ElongatedHorizontal // Likely a wreck
    } else if vertical_energy > horizontal_energy * 2.0 {
        TargetType::VerticalStructure   // Likely wellhead/pipes
    } else {
        TargetType::UnknownBlob
    }
}
```

## Why This Works
1. Physical accuracy — comparing meters not pixels
2. Curvelet sensitivity to edges/directions enhanced by correct aspect ratio
3. Inverse cube law affects both dimensions equally — correction preserves shape

## Implementation
1. Add aspect_ratio to SyntheticTile
2. Stretch before curvelet transform
3. Directional thresholding after decomposition
4. Test on known Erie targets (wrecks vs wellheads)

## Integration with cesarops-inference
This runs on the P100s via the matmul_half2.wgsl shader during SAR scan mode.
The curvelet forward/inverse stays on Xeon (f64 precision).
Only the pixel sweep (directional energy calculation) goes to GPU.

---

## Geological Subtraction — Filter the Noise, Find the Signal

### The Problem with Lake Erie
- Glacial till: random low-amplitude magnetic noise
- Basalt flows: mimic ship hull signatures
- Need to subtract geology to find anthropogenic anomalies

### Solution: Bandpass Filter → Residual → Dipole Detection

```
Raw Data → FFT → Bandpass Filter → IFFT → Residual
                                              ↓
                              Subtract geology (low + mid freq)
                                              ↓
                              What remains = anthropogenic targets
```

#### Frequency Bands:
- Low-frequency: large geology (bedrock, faults) → REMOVE
- Mid-frequency: intermediate (drumlins, sediment) → REMOVE
- High-frequency: small targets (ships, pipes) → KEEP
- Very-high-frequency: sensor noise → REMOVE

#### geo_filter.rs Module:

```rust
pub struct GeologicalFilter {
    pub low_cutoff_hz: f64,
    pub high_cutoff_hz: f64,
}

impl GeologicalFilter {
    pub fn filter_geology(&self, raw_grid: &SyntheticTile) -> SyntheticTile {
        // FFT → bandpass mask → IFFT → residual
    }

    pub fn detect_dipole(&self, residual: &SyntheticTile) -> Vec<DipoleCandidate> {
        // Find positive-negative pairs (5-200m separation = man-made)
    }
}

pub struct DipoleCandidate {
    pub center: Coordinates,
    pub separation_meters: f64,
    pub confidence: f64,
}
```

### Anthropogenic Signatures (what survives the filter):
1. Dipole symmetry (positive-negative pair) — ships have this, geology rarely does
2. Sharp edges / straight lines — wrecks have right angles
3. Consistent amplitude — steel is uniform, geology varies

### The Galvanic Cell Angle (Rossa-specific):
After geological subtraction, LOWER the detection threshold.
The lead keel + steel bolts galvanic cell creates a WEAK but PERSISTENT dipole.
With geology removed, even a millivolt-level signal becomes detectable.

### Full Pipeline:
1. Filter geology (bandpass removes basalt + glacial till)
2. Lower threshold on residual (cleaner signal = more sensitivity)
3. Detect dipoles (galvanic cell signature from lead/steel)
4. Elongate/classify with aspect ratio (wreck vs wellhead)
5. Cross-reference with thermal + blue spectrum + current ripples

### Integration:
- geo_filter.rs goes into nauticuvs crate
- FFT/IFFT uses f64 precision on Xeon (AVX-512)
- Dipole sweep uses matmul_half2.wgsl on P100 (FP16 2:1)
- Expose as MCP tool: filter_geology, detect_dipole
