# Sonar Waterfall Enhancement: Technical Implementation Plan
**Author:** Sonar Signal Processing Enhancement Project  
**Date:** February 2026  
**Version:** 1.0

## Executive Summary

This document outlines a comprehensive signal processing pipeline for enhancing sonar waterfall visualizations from Garmin RSD format files. The current implementation uses simple per-ping normalization, resulting in inconsistent intensity representation and poor visual quality. This plan implements industry-standard corrections based on sonar physics.

---

## 1. Background & Current State Analysis

### 1.1 Current Implementation Issues
- **Per-ping normalization**: Each ping normalized independently causes intensity drift
- **No acoustic corrections**: Missing Time-Varied Gain (TVG) compensation
- **Limited dynamic range**: 8-bit grayscale clips high-intensity returns
- **No noise reduction**: Speckle noise and electronic interference visible
- **Data gaps**: Black rectangular regions indicate missing samples

### 1.2 Data Structure (from lib.rs analysis)
```rust
struct Ping {
    channel: u32,
    depth_ft: f32,
    temp_c: Option<f32>,
    samples: Vec<u16>,  // Raw intensity samples
    // ... other fields
}
```

---

## 2. Signal Processing Pipeline Architecture

### 2.1 Pipeline Overview
```
Raw Samples → TVG Correction → Dynamic Range Compression 
→ Noise Reduction → Histogram Equalization → Colormap → Output
```

### 2.2 Processing Stages

#### Stage 1: Time-Varied Gain (TVG) Correction
**Purpose**: Compensate for geometric spreading and absorption losses

**Theory**: Acoustic intensity decreases with:
1. Spherical spreading: I ∝ 1/r²
2. Absorption: I ∝ e^(-αr)

**Implementation**:
```
TVG(sample, range_m) = sample × range_m² × e^(α × range_m)
```

Where:
- `range_m` = sample_index × (sound_speed / sample_rate) / 2
- `α` = absorption coefficient (frequency-dependent)
  - 455 kHz: α ≈ 0.10 dB/m
  - 800 kHz: α ≈ 0.30 dB/m
- `sound_speed` ≈ 1500 m/s (freshwater)

**Parameters**:
- `tvg_spreading_factor`: 20.0 (default, adjustable 10-40)
- `tvg_absorption_db_per_m`: frequency-dependent
- `tvg_start_sample`: Skip near-field (typically 5-10 samples)

---

#### Stage 2: Dynamic Range Compression
**Purpose**: Map wide dynamic range (60-100 dB) to display range (0-255)

**Method**: Logarithmic scaling with adjustable floor
```
dB = 20 × log₁₀(tvg_corrected + ε)
normalized = clamp((dB - min_dB) / (max_dB - min_dB), 0, 1)
```

**Parameters**:
- `noise_floor_db`: -60 dB (typical)
- `signal_ceiling_db`: 0 dB
- `epsilon`: 1e-10 (prevents log(0))

**Adaptive Option**:
- Calculate percentiles across entire dataset: P₀.₁ and P₉₉.₉
- Use these as dynamic floor/ceiling

---

#### Stage 3: Noise Reduction
**Purpose**: Remove speckle noise while preserving edges

**Method 1: Median Filter** (fast, simple)
- Kernel: 3×3 or 5×5
- Good for salt-and-pepper noise

**Method 2: Bilateral Filter** (edge-preserving)
```
I_filtered(x) = (1/W) × Σ I(y) × exp(-||x-y||²/2σ_s²) × exp(-|I(x)-I(y)|²/2σ_r²)
```
Where:
- `σ_s` = spatial sigma (3-5 pixels)
- `σ_r` = range sigma (0.1-0.3 intensity units)

**Method 3: Adaptive Wiener Filter**
- Estimates local noise variance
- Best quality, higher computational cost

**Recommended**: Start with median (3×3), add bilateral if needed

---

#### Stage 4: Contrast Enhancement
**Purpose**: Maximize visual information content

**Method 1: Histogram Equalization** (global)
```
CDF(i) = Σ(j=0 to i) histogram[j] / total_pixels
output = CDF(input) × 255
```

**Method 2: CLAHE** (Contrast-Limited Adaptive Histogram Equalization)
- Tile size: 8×8 or 16×16
- Clip limit: 2.0-4.0
- Better for non-uniform scenes

**Hybrid Approach**:
1. Global equalization for baseline
2. Local adaptive enhancement in low-contrast regions

---

#### Stage 5: Colormap Application
**Purpose**: Perceptual enhancement through color

**Recommended Colormaps**:
1. **Viridis** (perceptually uniform, colorblind-friendly)
2. **Magma** (similar to viridis, higher contrast)
3. **Jet** (traditional, maximum discrimination but not perceptually linear)
4. **Custom Sonar Map**:
   - Black → Blue (noise floor)
   - Blue → Cyan → Green (weak returns)
   - Yellow → Red → White (strong returns)

**Implementation**: 256-entry LUT with smooth interpolation

---

## 3. Implementation Specifications

### 3.1 Processing Parameters Structure
```rust
pub struct SonarProcessingParams {
    // TVG Parameters
    pub tvg_enabled: bool,
    pub tvg_spreading_factor: f32,      // 20.0 default (dB)
    pub tvg_absorption_db_per_m: f32,   // 0.1-0.3 (frequency dependent)
    pub tvg_start_sample: usize,        // Skip near-field (5-10)
    pub sound_speed_m_per_s: f32,       // 1500.0 (freshwater)
    
    // Dynamic Range
    pub log_compression: bool,
    pub noise_floor_db: f32,            // -60.0
    pub signal_ceiling_db: f32,         // 0.0
    pub use_adaptive_range: bool,       // Auto-compute from percentiles
    pub floor_percentile: f32,          // 0.1
    pub ceiling_percentile: f32,        // 99.9
    
    // Filtering
    pub median_filter_enabled: bool,
    pub median_kernel_size: usize,      // 3 or 5
    pub bilateral_filter_enabled: bool,
    pub bilateral_spatial_sigma: f32,   // 3.0
    pub bilateral_range_sigma: f32,     // 0.2
    
    // Contrast Enhancement
    pub histogram_equalization: bool,
    pub clahe_enabled: bool,
    pub clahe_tile_size: usize,         // 8 or 16
    pub clahe_clip_limit: f32,          // 2.0-4.0
    
    // Colormap
    pub colormap: Colormap,
    
    // Data Handling
    pub interpolate_gaps: bool,         // Fill black regions
    pub gap_threshold_samples: usize,   // 10
}

pub enum Colormap {
    Grayscale,
    Viridis,
    Magma,
    Jet,
    SonarCustom,
}
```

### 3.2 Two-Pass Processing Strategy

**Pass 1: Statistical Analysis**
- Compute global min/max, mean, std dev
- Build histograms
- Detect data gaps
- Calculate adaptive parameters

**Pass 2: Rendering**
- Apply corrections with computed parameters
- Generate frames with progress tracking

**Memory Optimization**: Process in chunks if full dataset doesn't fit in RAM

---

## 4. Enhanced Video Module Architecture

### 4.1 Module Structure
```
video_enhanced/
├── mod.rs              // Public API
├── processing.rs       // Signal processing pipeline
├── tvg.rs              // TVG correction implementation
├── filters.rs          // Median, bilateral filters
├── colormaps.rs        // LUT generation
├── statistics.rs       // Two-pass statistics
└── renderer.rs         // Frame generation & encoding
```

### 4.2 Processing Flow
```rust
pub fn render_enhanced_waterfall(
    pings: Vec<Ping>,
    output_path: &Path,
    params: SonarProcessingParams,
    on_progress: impl Fn(u32, u32),
) -> Result<VideoExportResult> {
    // Pass 1: Analyze
    let stats = compute_dataset_statistics(&pings, &params)?;
    
    // Pass 2: Process & render
    let processed = apply_processing_pipeline(&pings, &params, &stats)?;
    
    // Pass 3: Encode video
    encode_to_video(processed, output_path, &params, on_progress)
}
```

---

## 5. Gap Filling & Interpolation

### 5.1 Gap Detection
```rust
fn detect_gaps(samples: &[u16], threshold: usize) -> Vec<(usize, usize)> {
    // Find consecutive zero or very low (<5) samples
    // Return (start_idx, end_idx) tuples
}
```

### 5.2 Interpolation Methods

**Spatial Interpolation** (between pings):
- Linear interpolation for small gaps (<5 pings)
- Cubic interpolation for smoother transitions

**Temporal Interpolation** (within ping):
- Use neighboring ping data at same sample index
- Weighted average based on distance

**Black Region Filling**:
- Detect rectangular regions (your current black boxes)
- Fill with noise floor + small random variation
- Mark as interpolated in metadata

---

## 6. Quality Metrics & Validation

### 6.1 Quantitative Metrics
- **SNR** (Signal-to-Noise Ratio): Measure in target regions
- **Contrast**: Standard deviation of intensity distribution
- **Entropy**: Information content (higher = better detail)
- **Edge Preservation**: Compare before/after Sobel edge detection

### 6.2 Validation Approach
1. Process test dataset with multiple parameter sets
2. Generate side-by-side comparisons
3. Compute metrics for each
4. Select optimal parameters

---

## 7. Implementation Roadmap

### Phase 1: Foundation (Week 1)
- [ ] Create `video_enhanced` module structure
- [ ] Implement TVG correction
- [ ] Add logarithmic compression
- [ ] Basic statistics collection

### Phase 2: Filtering (Week 2)
- [ ] Median filter implementation
- [ ] Bilateral filter (optional)
- [ ] Gap detection and interpolation

### Phase 3: Enhancement (Week 3)
- [ ] Histogram equalization
- [ ] CLAHE implementation
- [ ] Colormap generation (viridis, magma, custom)

### Phase 4: Integration (Week 4)
- [ ] Integrate with existing video.rs
- [ ] Add UI parameter controls
- [ ] Batch processing capability
- [ ] A/B comparison tool

### Phase 5: Optimization & Testing (Week 5)
- [ ] Performance profiling
- [ ] SIMD optimization for filters
- [ ] Multi-threading for large datasets
- [ ] Comprehensive test suite

---

## 8. Performance Considerations

### 8.1 Computational Complexity
- **TVG correction**: O(n) - very fast
- **Median filter 3×3**: O(n) - fast
- **Bilateral filter**: O(n × kernel_size²) - moderate
- **CLAHE**: O(n × tiles) - moderate
- **Histogram eq**: O(n) - fast

### 8.2 Optimization Strategies
1. **SIMD**: Use `packed_simd` for TVG and filtering
2. **Parallel**: Process pings in parallel with `rayon`
3. **Caching**: Precompute TVG lookup tables
4. **Streaming**: Don't load entire dataset if >1GB

### 8.3 Memory Requirements
- Raw data: `num_pings × samples_per_ping × 2 bytes`
- Processed: Same size (in-place operations where possible)
- Peak: ~2-3× raw data size during processing

Typical 1-hour log: ~500MB raw → ~1.5GB peak usage

---

## 9. API Design

### 9.1 High-Level API
```rust
// Simple: use defaults
let result = render_enhanced_waterfall_auto(&pings, output_dir)?;

// Advanced: full control
let params = SonarProcessingParams {
    tvg_enabled: true,
    tvg_spreading_factor: 20.0,
    log_compression: true,
    colormap: Colormap::Viridis,
    ..Default::default()
};
let result = render_enhanced_waterfall(&pings, output_dir, params, |f, t| {
    println!("Frame {f}/{t}");
})?;
```

### 9.2 CLI Interface
```bash
# Process with defaults
sonar-sniffer enhance input.rsd --output-dir ./processed/

# Custom parameters
sonar-sniffer enhance input.rsd \
    --tvg-factor 25.0 \
    --colormap viridis \
    --clahe \
    --interpolate-gaps
    
# Batch processing
sonar-sniffer enhance-batch ./data/*.rsd --preset high-quality
```

---

## 10. Testing Strategy

### 10.1 Unit Tests
- TVG correction accuracy (known input → expected output)
- Filter kernel operations
- Colormap interpolation
- Gap detection logic

### 10.2 Integration Tests
- Full pipeline on synthetic data
- Known sonar patterns (single target, multiple targets, bottom)

### 10.3 Regression Tests
- Compare against reference images
- Ensure metrics don't degrade

### 10.4 Visual Inspection
- Side-by-side before/after viewer
- Parameter sweep gallery

---

## 11. Documentation Requirements

### 11.1 User Documentation
- Parameter descriptions with visual examples
- Presets for common scenarios (shallow water, deep water, structure scan)
- Troubleshooting guide

### 11.2 Developer Documentation
- Algorithm explanations with citations
- Performance characteristics
- Extension points for custom processing

---

## 12. Future Enhancements

### 12.1 Advanced Features (Post-V1)
- **Multi-frequency fusion**: Combine 455kHz + 800kHz data
- **Bottom tracking**: Automatic depth extraction
- **Target detection**: Fish/structure identification
- **Georeferencing**: GPS overlay on waterfall
- **3D visualization**: Point cloud generation from sidescan

### 12.2 Machine Learning Integration
- **Denoising**: Trained CNN for noise reduction
- **Super-resolution**: Upscale low-res data
- **Classification**: Automatic bottom type identification

---

## 13. References

### 13.1 Literature
1. Lurton, X. (2010). *An Introduction to Underwater Acoustics*. Springer.
2. Blondel, P. (2009). *The Handbook of Sidescan Sonar*. Springer.
3. Mitchell, N. C., & Somers, M. L. (1989). Quantitative backscatter measurements with a long-range side-scan sonar. *IEEE Journal of Oceanic Engineering*, 14(4), 368-374.

### 13.2 Industry Standards
- IHO Standards for Hydrographic Surveys (S-44)
- NOAA Hydrographic Surveys Specifications and Deliverables

### 13.3 Software References
- OpenCV Documentation: Image Filtering
- GStreamer Documentation: Video Encoding
- Rust Image Processing: `image` and `imageproc` crates

---

## Appendix A: Mathematical Derivations

### A.1 TVG Correction Derivation
Starting from the sonar equation:
```
SL = RL + 2TL + TS
```
Where:
- SL = Source Level
- RL = Received Level (what we measure)
- TL = Transmission Loss = 20log₁₀(r) + αr
- TS = Target Strength

To recover target strength (what we want):
```
TS_corrected = RL + 2[20log₁₀(r) + αr]
            = RL + 40log₁₀(r) + 2αr
```

In linear units:
```
I_corrected = I_measured × r^(spreading_factor/10) × 10^(α×r/10)
```

### A.2 Bilateral Filter Weights
Given pixel positions x and y with intensities I(x) and I(y):

Spatial weight:
```
w_s(x,y) = exp(-||x - y||² / (2σ_s²))
```

Range (intensity) weight:
```
w_r(x,y) = exp(-(I(x) - I(y))² / (2σ_r²))
```

Combined weight:
```
w(x,y) = w_s(x,y) × w_r(x,y)
```

Output:
```
I_out(x) = Σ_y [w(x,y) × I(y)] / Σ_y w(x,y)
```

---

## Appendix B: Parameter Tuning Guide

### B.1 TVG Spreading Factor
- **10-15**: Shallow water, strong bottom returns
- **20**: Standard (spherical spreading)
- **25-30**: Deep water, weak returns
- **35-40**: Extreme range compensation

### B.2 Noise Floor Selection
- Start at -60 dB
- If too noisy: decrease to -50 dB
- If missing detail: increase to -70 dB

### B.3 Filter Kernel Size
- **3×3**: Light filtering, preserve detail
- **5×5**: Moderate filtering, balance
- **7×7**: Heavy filtering, smooth appearance

### B.4 CLAHE Clip Limit
- **1.0**: Minimal effect
- **2.0**: Moderate enhancement (recommended)
- **4.0**: Strong enhancement (may amplify noise)
- **>4.0**: Extreme (use with caution)

---

## Appendix C: Colormap Specifications

### C.1 Viridis LUT (256 entries, RGB)
```
0:   (68, 1, 84)     // Dark purple
64:  (59, 82, 139)   // Blue
128: (33, 145, 140)  // Teal
192: (94, 201, 98)   // Green
255: (253, 231, 37)  // Yellow
```

### C.2 Custom Sonar LUT
```
0:   (0, 0, 0)       // Black (noise floor)
32:  (0, 0, 128)     // Dark blue
64:  (0, 128, 255)   // Cyan
128: (0, 255, 0)     // Green
192: (255, 255, 0)   // Yellow
224: (255, 128, 0)   // Orange
255: (255, 255, 255) // White (strong return)
```

---

## Appendix D: Performance Benchmarks

### D.1 Expected Processing Times (Intel i7-9700K, 8 cores)
| Dataset Size | Processing Time | Throughput |
|--------------|----------------|------------|
| 10k pings    | 0.5s          | 20k pings/s |
| 100k pings   | 4.2s          | 24k pings/s |
| 1M pings     | 45s           | 22k pings/s |

### D.2 Memory Usage
| Dataset Size | Peak RAM | Notes |
|--------------|----------|-------|
| 10k pings    | 150 MB   | All in-memory |
| 100k pings   | 1.2 GB   | All in-memory |
| 1M pings     | 8 GB     | Consider streaming |

---

## Document History
- v1.0 (2026-02-25): Initial release
- Future: Update with implementation results and benchmarks

---

**End of Document**
