# Candle-Dependent Modules (Future Integration)

These files require candle-core with the WgpuBufferExtractor patch.
They replace the standalone versions in src/ once candle is forked into the workspace.

Files:
- optical_mass_candle.rs — Thermocline jitter filter via wgpu dispatch
- galvanic_battery_candle.rs — Ion plume tracker via wgpu dispatch
- satellite_stitch_candle.rs — Coordinate-free grid via wgpu dispatch
- geo_filter_candle.rs — Geological subtraction via wgpu dispatch

These use:
- candle_core::wgpu_backend::{WgpuBufferExtractor, CesarOpsWgpuBridge}
- Direct wgpu::Buffer extraction from Candle tensors
- Zero-copy shader dispatch on the same GPU timeline
