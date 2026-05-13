# Sonar Sniffer — Dual-Track Commercial Plan

## Architecture: Single crate, two deployment tracks

### sonar-core (standalone Rust crate)
- Garmin binary parser (gstd format)
- Zero external deps, compiles anywhere
- Outputs: SonarPingFrame { lat, lon, depth, bottom_hardness, signal_intensity }

### Free Track: SAR Deployment
- Integrated as MCP tool in cesarops-inference
- AI calls it autonomously during search operations
- Real-time dipole & anomaly tracking
- No cost to operate (runs on our cluster)

### Paid Track: Habitat & Structure App
- Standalone desktop GUI (egui or slint)
- Runs on customer's hardware (zero hosting cost)
- Features: side-scan mosaic, KML export, bottom-hardness mapping
- Target: fishermen, wreck hunters, marine surveyors

## Revenue → Hardware Scaling
- 10-15 licenses: Used V100 (32GB) → unlocks cuda-backend
- 30-50 licenses: M10 / multi-GPU chassis
- Enterprise: H100 cloud for massive MoE inference

## Differentiators
- AI-powered anomaly detection (no competitor has this)
- bottom_hardness metric (structure fishing)
- KML/Google Earth export
- Community tier: crowdsourced lakebed mapping feeds the AI

## Risks
- Garmin binary format is proprietary (firmware updates can break parser)
- Competition: ReefMaster, Dr.Depth (but no AI integration)

## Integration with cesarops-inference
- sonar-core lives at crates/sonar-core/
- MCP tool: parse_sonar_file(path) → Vec<SonarPingFrame>
- geo_filter.rs uses ping data for dipole correlation
- satellite_stitch.rs uses GPS anchors from sonar for coordinate alignment

## Status: FUTURE WORK — build after inference engine is generating tokens
