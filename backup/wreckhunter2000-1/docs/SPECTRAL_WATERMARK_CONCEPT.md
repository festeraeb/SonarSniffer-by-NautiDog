# Spectral Watermark Grid — Coordinate-Free Tile Alignment

## The Concept
Like a laser grid against a green screen for motion capture — overlay a unique pattern
on the original tile BEFORE slicing. The pattern provides alignment anchors in every axis
regardless of what's in the actual image data (even featureless open ocean).

## How It Works
1. Before slicing: inject a unique spectral pattern into an unused band/channel
2. Pattern uses unique shapes at known intervals (not just dots — asymmetric shapes
   that guarantee rotational alignment too)
3. Slice the tile into sub-pixel grid sections (16-section or custom)
4. Process each section independently (no coordinates needed during processing)
5. To reassemble: scan for the spectral pattern in each slice
6. Pattern matching gives you exact X, Y, AND rotation alignment
7. Overlay reassembled result back onto the original (which has real coordinates)
8. Strip the watermark from final output

## Why This Solves the Drift Problem
- No UTM zone boundaries matter (pattern is relative, not geographic)
- No projection math needed during processing
- Works on featureless ocean (MH370 problem) because the pattern IS the feature
- Works across any satellite sensor (just pick an unused spectral band)
- Sub-pixel precision because the pattern can be at any resolution

## The Laser Grid Analogy
- Motion capture uses IR dots on a green screen to track body movement
- We use spectral dots in an unused band to track tile alignment
- The "green screen" is the unused spectral channel
- The "IR dots" are our unique asymmetric shapes
- The "body" is the actual satellite/sonar data we're processing

## Unique Shape Requirements
- Asymmetric (so rotation is detectable)
- Non-repeating within a tile (so position is unambiguous)
- High contrast in the chosen spectral band
- Small enough to not interfere with actual data processing
- Pattern stride matches the sub-pixel grid resolution

## Integration
- satellite_stitch.rs: injects pattern before slicing, reads it for reassembly
- spectral_matcher.rs: GPU-accelerated pattern detection via wgpu
- spectral_matcher.wgsl: compute shader for parallel pattern scanning

## Status: CONCEPT SAVED — implement after token generation is working
