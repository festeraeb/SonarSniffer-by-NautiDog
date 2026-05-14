# Files That Should Be Created

## Assessment Criteria
These files are missing from the backup but are needed for a complete, functional deployment.
Priority levels: High (blocks core functionality), Medium (important for operations), Low (nice-to-have).

| Suggested Path | Purpose | Why It's Needed | Priority |
|---------------|---------|-----------------|----------|
| deploy/tools/curvelet_slicer.rs | FFT-based curvelet transform for satellite tile drift correction | Coordinate drift in satellite tiles causes misalignment; critical for accurate wreck detection | High |
| deploy/tools/spectral_unmixer.rs | Spectral unmixing of multispectral imagery | Separates water, sediment, metal signatures for wreck identification | High |
| deploy/config/prometheus.yml | Monitoring configuration for GPU cluster metrics | Enables real-time P100/M10 performance tracking and alerting | Medium |
| deploy/config/grafana_dashboards.json | Visualization dashboards for fleet health | Operators need visual monitoring of multi-node GPU status | Medium |
| deploy/scripts/deploy.sh | One-command deployment script | Automates build, test, and service startup across all modules | Medium |
| deploy/scripts/cleanup.sh | Resource cleanup (GPU memory, temp files) | Prevents OOM on long-running inference sessions | Medium |
| deploy/docs/ARCHITECTURE.md | System architecture overview | New team members need to understand module relationships | Medium |
| deploy/docs/API_REFERENCE.md | API documentation for forge-v2 endpoints | External integrations require documented endpoints | Medium |
| deploy/inference/src/shader_compiler.rs | WGSL shader compilation from file system | Shaders are stored in shaders/ but no compiler bridges them to wgpu | High |
| deploy/detection/src/wreck_classifier.rs | ML-based wreck classification model loader | Detection pipeline needs trained models to classify anomalies | High |
| deploy/agent/src/memory_manager.rs | Persistent agent memory/state store | Agents need to maintain context across mission phases | Medium |
| deploy/mcp_steered/src/tool_registry.rs | Dynamic tool registration and discovery | MCP tools should be discoverable at runtime without restart | Low |
| deploy/web/src/store.ts | Frontend state management | Forge web UI needs centralized state for task results and config | Medium |
