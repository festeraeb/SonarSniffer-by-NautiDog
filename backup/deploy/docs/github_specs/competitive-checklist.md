# Competitive Capability Checklist

Reference targets: SeaView Mosaic, SonarWiz Sidescan, SonarTRX.

## Priority Gaps To Close

- Multi-format parser coverage beyond Garmin (`XTF`, `JSF`, `SDF`, `S7K`, `SL2/SL3`, `Humminbird`, etc.).
- Navigation correction and editing: layback, heading smoothing, CMG filtering, split/trim, manual range edits.
- Backscatter processing stack: AGC, TVG, de-striping, beam-angle correction, slant-range correction, spike removal.
- High-fidelity mosaic engine preserving waterfall resolution with blending/feathering controls.
- Scalable data model for very large projects and processing history with undo/redo.
- Contact database + picking workflow + report generation.
- Survey planning workflows (parallel lines in polygon, timing/coverage estimates).
- Real-time/near-real-time quality checks and coverage confirmation.

## Already Implemented In This Repo Pass

- Garmin-first resilient parser with record resynchronization.
- Rust output pipeline:
  - Waterfall PNG
  - Mosaic PNG
  - MBTiles
  - KML/KMZ
  - ArcGIS sidecar JSON
  - Free static web viewer bundle (MapLibre)
- Feature-gated GStreamer Rust integration entrypoint.

## Next Rust-First Milestones

1. Parser architecture
- Introduce `SonarParser` trait and plugin registry.
- Add parser fixtures and corpus tests for each format.
- Add firmware-map support (field dictionaries + sync signatures).

2. Processing graph
- Build deterministic processing pipeline with stage metadata.
- Persist stage parameters and output hashes for undo/redo and reproducibility.

3. Geospatial output quality
- Add true georeferenced raster outputs (`GeoTIFF`, tiled pyramids).
- Add mosaic feathering and seam leveling.
- Add trackline and coverage polygon generation.

4. Viewer and chart overlay
- Add ENC/S57 and NOAA vector chart overlays in bundled viewer.
- Add layer controls for sonar, charts, contacts, annotations.

5. Video and presentation
- Build GStreamer timeline renderer with keyframes, text overlay, logo watermark.
- Export MP4/WebM with project presets.

6. Performance and scale
- Chunked IO + memory-mapped reading for large files.
- Parallel tile generation + background task queue.
- Benchmarks for load time, render speed, and export throughput.

## Quality Gates

- Golden image tests for waterfall/mosaic output stability.
- Fuzz tests for parser self-healing and malformed records.
- End-to-end regression tests for MBTiles/KML/KMZ/ArcGIS export integrity.
- Performance baselines tracked in CI.
