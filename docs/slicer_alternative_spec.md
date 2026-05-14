# Slicer Alternative: Dual-GPU Temporal Stack with Cross-Verification

## Problem Statement

Traditional GeoTIFF analysis requires slicing large raster tiles into chunks that fit in VRAM, processing each chunk independently, then stitching results back together. This introduces:
- Coordinate drift at slice boundaries
- Seam artifacts from imperfect overlap alignment
- Reassembly complexity
- Lost context at edges (anomalies split across slices get missed or double-counted)

## Solution: Full-Tile Temporal Stacking on Dual P100s

Load entire tiles (no slicing) into each P100's 16GB HBM2. Run two independent temporal stacks in parallel, then cross-verify results for high-confidence detection.

## Architecture

```
                    ┌─────────────────────────────────┐
                    │         CPU Orchestrator         │
                    │   (Xeon 4110 — traffic cop)      │
                    └──────────┬──────────┬───────────┘
                               │          │
                    ┌──────────▼──┐  ┌────▼──────────┐
                    │   P100 #0   │  │   P100 #1     │
                    │   16GB HBM2 │  │   16GB HBM2   │
                    │             │  │               │
                    │ Tiles 0-9   │  │ Tiles 10-19   │
                    │ (temporal   │  │ (temporal     │
                    │  window A)  │  │  window B)    │
                    │             │  │               │
                    │ → anomaly_A │  │ → anomaly_B   │
                    └──────────┬──┘  └────┬──────────┘
                               │          │
                    ┌──────────▼──────────▼───────────┐
                    │     Cross-Verification Pass      │
                    │  (GPU or CPU — compare A vs B)   │
                    │                                   │
                    │  Agreement = HIGH CONFIDENCE      │
                    │  A only    = REVIEW               │
                    │  B only    = REVIEW               │
                    │  Neither   = CLEAR                │
                    └─────────────────────────────────┘
```

## Memory Budget

- Sentinel-2 tile at 10m: 10980 × 10980 pixels
- Single band f32: ~460 MB
- 5 bands (RGB + NIR + SWIR): ~2.3 GB per tile
- 10 tiles × 2.3 GB = ~23 GB → too much for 16GB

### Practical fit (per P100, 16GB):
- **3 bands per tile** (NIR + SWIR + one visible): ~1.4 GB/tile → 10 tiles = 14 GB ✓
- **Single-band analysis** (e.g. NDWI ratio pre-computed on CPU): ~460 MB/tile → 10 tiles = 4.6 GB, leaves 11 GB for intermediate buffers ✓
- **f16 storage** with f32 compute: ~0.7 GB/tile for 3 bands → 10 tiles = 7 GB ✓

**Recommended approach**: Pre-compute band ratios on CPU (NDWI, NDVI, glint index) into single f32 channels, then stack 10 ratio tiles per GPU. This gives maximum temporal depth with minimum VRAM.

## Temporal Window Strategy

- **GPU 0 (Window A)**: Most recent 10 acquisitions (e.g. last 30-60 days)
- **GPU 1 (Window B)**: Next 10 acquisitions (e.g. 60-180 days ago)

This gives temporal diversity in the verification:
- If both windows flag the same location → persistent anomaly (wreck, structure, debris field)
- If only Window A flags it → recent event (new debris, vessel, oil slick)
- If only Window B flags it → historical anomaly now resolved (seasonal, transient)

### The 3-tile minimum

Even within a 10-tile stack, as few as 3 well-chosen tiles can produce reliable detection IF:
1. One calm-water day (baseline reflectance)
2. One post-storm day (debris/disturbance visible)
3. One seasonal opposite (eliminates biological false positives)

The other 7 tiles add confidence scoring and reduce noise floor. More temporal samples = tighter anomaly thresholds = fewer false positives.

## Processing Pipeline

### Phase 1: CPU Prep (Xeon)
```rust
// Pre-compute band ratios on CPU (embarrassingly parallel across tiles)
fn prepare_tiles(raw_tiles: &[GeoTiff]) -> Vec<AnalysisTile> {
    raw_tiles.par_iter().map(|tile| {
        AnalysisTile {
            ndwi: compute_ndwi(&tile.nir, &tile.green),      // (Green - NIR) / (Green + NIR)
            glint: compute_glint(&tile.nir, &tile.swir),     // NIR / SWIR ratio
            timestamp: tile.acquisition_time,
            bbox: tile.geo_bounds,
        }
    }).collect()
}
```

### Phase 2: GPU Upload & Dispatch
```rust
// Upload 10 tiles to each GPU — no slicing
fn dispatch_dual_stack(
    gpu0: &GpuContext,
    gpu1: &GpuContext,
    tiles: &[AnalysisTile; 20],
) -> (AnomalyMap, AnomalyMap) {
    let window_a = &tiles[0..10];   // Recent
    let window_b = &tiles[10..20];  // Historical

    // Upload full tiles to GPU VRAM (PCIe transfer, ~2-3s total)
    let buf_a = gpu0.upload_tile_stack(window_a);
    let buf_b = gpu1.upload_tile_stack(window_b);

    // Dispatch temporal stacking shader on both GPUs simultaneously
    let handle_a = gpu0.dispatch_temporal_stack(&buf_a);
    let handle_b = gpu1.dispatch_temporal_stack(&buf_b);

    // Wait for both (GPU compute, ~100-500ms depending on shader complexity)
    let anomaly_a = gpu0.readback(&handle_a);
    let anomaly_b = gpu1.readback(&handle_b);

    (anomaly_a, anomaly_b)
}
```

### Phase 3: Cross-Verification
```rust
// Compare two independent anomaly maps
fn cross_verify(
    anomaly_a: &AnomalyMap,
    anomaly_b: &AnomalyMap,
    agreement_threshold: f32,
) -> VerifiedResults {
    let width = anomaly_a.width;
    let height = anomaly_a.height;
    let mut results = VerifiedResults::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let score_a = anomaly_a.get(x, y);
            let score_b = anomaly_b.get(x, y);

            let classification = if score_a > agreement_threshold && score_b > agreement_threshold {
                Confidence::High  // Both windows agree — persistent anomaly
            } else if score_a > agreement_threshold {
                Confidence::Review  // Recent only — new event?
            } else if score_b > agreement_threshold {
                Confidence::Historical  // Old only — resolved
            } else {
                Confidence::Clear
            };

            results.set(x, y, classification, score_a, score_b);
        }
    }

    results
}
```

### Phase 4: Result Extraction
```rust
// Extract georeferenced hits from verified results
fn extract_hits(
    results: &VerifiedResults,
    geo_transform: &GeoTransform,
    min_confidence: Confidence,
) -> Vec<ScanHit> {
    let mut hits = Vec::new();

    for region in results.connected_regions(min_confidence) {
        let center_pixel = region.centroid();
        let (lat, lon) = geo_transform.pixel_to_latlon(center_pixel.x, center_pixel.y);

        hits.push(ScanHit {
            lat, lon,
            confidence: region.max_confidence,
            score_recent: region.avg_score_a,
            score_historical: region.avg_score_b,
            area_pixels: region.pixel_count,
            classification: if region.max_confidence == Confidence::High {
                "persistent_anomaly"
            } else {
                "recent_event"
            },
        });
    }

    hits
}
```

## WGSL Shader: Temporal Stack Analysis

```wgsl
// temporal_stack.wgsl — runs on each P100 independently
// Input: 10 tiles as a 3D texture (width × height × 10 temporal layers)
// Output: anomaly score per pixel

struct Params {
    width: u32,
    height: u32,
    n_tiles: u32,
    recency_decay: f32,  // weight recent tiles higher
}

@group(0) @binding(0) var<storage, read> tiles: array<f32>;      // flattened [n_tiles][height][width]
@group(0) @binding(1) var<storage, read_write> output: array<f32>; // [height][width]
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;
    if (x >= params.width || y >= params.height) { return; }

    let pixel_idx = y * params.width + x;
    let stride = params.width * params.height;

    // Compute weighted mean across temporal stack
    var weighted_sum: f32 = 0.0;
    var weight_total: f32 = 0.0;

    for (var t: u32 = 0u; t < params.n_tiles; t++) {
        let val = tiles[t * stride + pixel_idx];
        // More recent tiles (higher t index) get more weight
        let weight = pow(params.recency_decay, f32(params.n_tiles - 1u - t));
        weighted_sum += val * weight;
        weight_total += weight;
    }

    let baseline = weighted_sum / weight_total;

    // Compute anomaly: how much does the most recent tile deviate from baseline?
    let current = tiles[(params.n_tiles - 1u) * stride + pixel_idx];
    let deviation = abs(current - baseline);

    // Compute variance for adaptive thresholding
    var variance_sum: f32 = 0.0;
    for (var t: u32 = 0u; t < params.n_tiles; t++) {
        let val = tiles[t * stride + pixel_idx];
        let diff = val - baseline;
        variance_sum += diff * diff;
    }
    let std_dev = sqrt(variance_sum / f32(params.n_tiles));

    // Anomaly score: number of standard deviations from baseline
    // Higher = more anomalous
    let anomaly_score = select(deviation / std_dev, 0.0, std_dev < 0.001);

    output[pixel_idx] = anomaly_score;
}
```

## CPU Orchestrator Cost

| Step | Where | Time |
|------|-------|------|
| Load 20 tiles from SSD | CPU/DMA | ~1-2s |
| Pre-compute band ratios | CPU (AVX2, 16 threads) | ~500ms |
| PCIe upload to 2 GPUs | DMA | ~2-3s |
| GPU temporal stack (both parallel) | P100 × 2 | ~200-500ms |
| PCIe readback (2 anomaly maps) | DMA | ~500ms |
| Cross-verification | CPU single pass | ~200ms |
| Region extraction | CPU | ~50ms |
| **Total** | | **~5-7 seconds per 20-tile scan** |

CPU compute is <1 second of that. The rest is I/O and GPU kernel time.

## Integration with Forge

The cluster panel's CESAROPS preset would configure this as:
```json
{
    "p100_mode": "split",
    "gpu0_role": "temporal-stack-recent",
    "gpu1_role": "temporal-stack-historical",
    "analysis_pipeline": "dual_verify",
    "tiles_per_gpu": 10,
    "band_mode": "precomputed_ratios",
    "verification_threshold": 2.5
}
```

When CESAROPS is active, the P100s run this pipeline on a loop:
1. Check for new tile acquisitions
2. Load latest 20 tiles
3. Run dual-stack analysis
4. Cross-verify
5. Push high-confidence hits to the scan database
6. Sleep until next acquisition window

## Why This Eliminates Slicing

- **No spatial subdivision**: Each tile is loaded whole. Pixel (0,0) to pixel (10979,10979) all in VRAM.
- **No boundary artifacts**: There are no boundaries. One tile = one contiguous memory block.
- **No stitching**: Nothing to stitch. The output anomaly map has the same dimensions as the input tile.
- **No coordinate drift**: The geo-transform is applied once at the end, to the final hit coordinates. No intermediate reprojections.
- **Temporal stacking replaces spatial tiling**: Instead of cutting space into pieces, we stack time into layers. The GPU processes all of space at once, across multiple time steps.

## Future: MI50 Addition

When the MI50 32GB arrives:
- Move the analysis pipeline to MI50 (1 TB/s bandwidth, 32GB = 15-18 tiles without band reduction)
- P100s become dedicated LLM inference
- MI50 runs the full 20-tile stack on ONE card (no split needed)
- Cross-verification becomes: run the same stack twice with different weighting schemes on the same card, or compare against a CPU baseline
