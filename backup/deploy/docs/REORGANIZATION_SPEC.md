# WreckHunter 2000 - Reorganization Specification

## Overview
This document describes the reorganization of the WreckHunter 2000 project into a clean deployment structure.
The `backup/src/` directory contains the RAW COPY (never modified). The `backup/deploy/` directory is the
clean working structure built from it.

## Directory Structure
```
backup/
├── src/                    ← RAW COPY (DO NOT MODIFY)
├── deploy/                 ← Clean working structure
│   ├── inference/          ← LLM inference engine (cesarops-inference)
│   │   ├── Cargo.toml      ← Inference module manifest
│   │   ├── main.rs         ← Main entry point with CUDA backend dispatch
│   │   ├── matmul.rs       ← Matrix multiplication kernels
│   │   └── matmul_half2.wgsl ← WGSL compute shader for GPU dispatch
│   ├── agent/              ← Orchestration & agent harness (cesarops-agent)
│   │   ├── Cargo.toml      ← Agent module manifest
│   │   ├── lib.rs          ← Agent library root
│   │   ├── main.rs         ← Agent CLI entry point
│   │   ├── dashboard.rs    ← Real-time monitoring dashboard
│   │   ├── harness.rs      ← Agent execution harness
│   │   ├── llm.rs          ← LLM integration layer
│   │   ├── nodes.rs        ← Multi-node cluster management
│   │   ├── router.rs       ← Task routing logic
│   │   ├── workflow.rs     ← Workflow orchestration
│   │   ├── config.rs       ← Agent configuration
│   │   └── telemetry.rs    ← Telemetry collection
│   ├── detection/          ← Wreck detection pipeline (cesarops-detection + adaptive)
│   │   ├── Cargo.toml      ← Detection module manifest
│   │   ├── lib.rs          ← Detection library root
│   │   ├── main.rs         ← Detection CLI entry point
│   │   ├── anomaly.rs      ← Anomaly detection algorithms
│   │   ├── spectral_filter.rs ← Spectral filtering for satellite imagery
│   │   ├── curvelet_transform.rs ← Curvelet-based feature extraction
│   │   ├── adaptive_detect.rs ← Adaptive threshold detection
│   │   ├── sensor_fusion.rs ← Multi-sensor data fusion
│   │   ├── classification.rs ← Wreck classification models
│   │   ├── validation.rs   ← Blind validation pipeline
│   │   └── metrics.rs      ← Performance metrics tracking
│   ├── forge/              ← Web UI & tool execution (cesarops-forge-v2)
│   │   ├── index.html      ← Main web interface
│   │   ├── static/         ← CSS, JS, assets
│   │   ├── templates/      ← HTML templates
│   │   ├── Cargo.toml      ← Forge module manifest
│   │   ├── lib.rs          ← Forge library root
│   │   ├── server.rs       ← HTTP server implementation
│   │   ├── tools_api.rs    ← Tool execution API endpoints
│   │   └── auth.rs         ← Authentication middleware
│   ├── tools/              ← Specialized tools
│   │   ├── aeromagnetic_worker/
│   │   │   ├── Cargo.toml  ← Aeromagnetic worker manifest
│   │   │   └── main.rs     ← Magnetic dipole analysis engine
│   │   ├── gpu_cluster/
│   │   │   ├── cluster_config.toml ← GPU cluster configuration
│   │   │   ├── node_manager.rs ← Node orchestration
│   │   │   └── load_balancer.rs ← Workload distribution
│   │   ├── satellite_stitch.rs   ← Satellite tile stitching
│   │   ├── optical_mass.rs       ← Optical mass processing
│   │   └── curvelet_slicer.rs    ← Curvelet coefficient slicing
│   ├── scripts/            ← Shell scripts, service files, utilities
│   │   ├── deploy.sh           ← Deployment script
│   │   ├── monitor.sh          ← System monitoring
│   │   ├── backup.sh           ← Data backup utility
│   │   └── health_check.sh     ← Service health checks
│   ├── config/             ← All config files (TOML, JSON, YAML)
│   │   ├── Cargo.toml        ← Root workspace manifest
│   │   ├── ai_stack_docker-compose.yml ← Docker compose for AI stack
│   │   ├── db/
│   │   │   └── watchdog_state.json ← Database watchdog state
│   │   └── settings.toml     ← Global application settings
│   ├── shaders/            ← WGSL compute shaders
│   │   ├── matmul_half2.wgsl ← Matrix multiplication shader
│   │   └── spectral_filter.wgsl ← Spectral filtering shader
│   ├── web/                ← HTML/CSS/JS frontend files
│   │   ├── index.html      ← Main forge UI
│   │   ├── static/css/     ← Stylesheets
│   │   ├── static/js/      ← JavaScript modules
│   │   └── templates/      ← Server-rendered templates
│   └── docs/               ← Documentation, specs, roadmaps
│       ├── mission_control_spec.md ← Mission control specification
│       ├── mission_control_impl.md ← Mission control implementation notes
│       ├── full_sensor_scan_spec.md ← Full sensor scan specification
│       ├── blind_validation_plan.md ← Blind validation plan
│       ├── native_search_impl.md ← Native search implementation
│       ├── searxng_deployment.md ← SearXNG deployment guide
│       ├── r1_answers/
│       │   ├── latest.md   ← Latest R1 analysis answers
│       │   └── q1_risks.md ← Risk assessment Q&A
│       └── roadmap.md      ← Project roadmap
├── DELETE_CANDIDATES.md    ← Files that serve no purpose
├── NEEDS_COMPLETION.md     ← Files that are stubs/incomplete but useful
├── NEEDS_WRITING.md        ← Files that SHOULD exist but don't yet
└── REORGANIZATION_SPEC.md  ← This file
```

## Module Descriptions

### cesarops-inference (deploy/inference)
The LLM inference engine with wgpu GPU dispatch and CUDA backends. Handles tensor operations,
matrix multiplication, and model loading for the agent system.

### cesarops-agent (deploy/agent)Orchestration layer that manages autonomous agents. Includes dashboard monitoring, task routing,
workflow execution, multi-node cluster management, and telemetry collection.

### cesarops-detection (deploy/detection)
Wreck detection pipeline combining spectral filtering, curvelet transforms, adaptive thresholding,
sensor fusion, and classification models. Includes blind validation for quality assurance.

### cesarops-forge-v2 (deploy/forge)
Web UI and tool execution framework. Provides HTTP API endpoints for running detection tools,
a real-time dashboard, authentication middleware, and HTML/CSS/JS frontend.

### cesarops-aeromagnetic-worker (deploy/tools/aeromagnetic_worker)
Aeromagnetic anomaly detection using magnetic dipole analysis. Processes aeromagnetic survey data
to identify potential shipwreck locations based on magnetic signature patterns.

### cesarops-gpu-cluster (deploy/tools/gpu_cluster)
Multi-node GPU cluster orchestration managing P100s, 1070s, 1060s, and future M10s. Includes
node management, load balancing, and cluster configuration.

### nautivecs (deploy/agent or deploy/inference)
Core vector search and embedding infrastructure. Used by both inference and agent modules for
semantic search over mission data and sensor results.

## File Categorization Rules
1. Files in `cesarops-*` directories go to their corresponding module folder
2. Config files (.toml, .json, .yaml) go to `config/`
3. WGSL shader files go to `shaders/`
4. Web files (HTML, CSS, JS) go to `web/`
5. Documentation goes to `docs/`
6. Shell scripts go to `scripts/`
7. Specialized standalone tools go to `tools/`
8. If a file belongs to multiple categories, it goes in the PRIMARY category only