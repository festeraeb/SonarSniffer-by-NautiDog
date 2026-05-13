# Requirements Document

## Introduction

This document defines the requirements for the Distributed Orchestrator Pipeline — a system that coordinates GPU workers across the CESARops cluster for shipwreck detection via temporal satellite imagery stacking. The system uses dynamic role assignment through nautivecs vector injection, hardware discovery at startup, and enforces a hard 20+ day temporal stacking minimum. Requirements are organized by testability: what can be validated now on cesarops2/cesarops3, and what requires T440 (deferred).

## Glossary

- **Orchestrator**: The coordination service that translates user requests, enforces temporal coverage rules, dispatches work to specialists, and interprets results. Runs on cesarops3.
- **Worker**: A GPU-equipped node running a permanently-loaded LLM that accepts task dispatches and nautivecs context injection. Runs on cesarops2.
- **Compute_Engine**: The wgpu/WGSL shader execution service on T440's dual P100 GPUs. Pure GPU math, no LLM.
- **nautivecs_Engine**: The AST-driven codebase vectorization and semantic retrieval system (v0.1.0, serverless JSON store) used for context injection.
- **Temporal_Stack**: A validated collection of satellite tiles spanning 20+ unique calendar days over a target geographic region.
- **Persistence_Enforcer**: The orchestrator subsystem that guarantees the 20+ day temporal stacking rule is never violated.
- **Sensor_Specialist**: A Worker role injected with band-ratio/threshold detection expertise via nautivecs.
- **Geometry_Specialist**: A Worker role injected with coordinate/grid/drift correction expertise via nautivecs.
- **Detection_Thresholds**: Numeric parameters (glint, hydrocarbon, thermal anomaly) tuned by the Sensor_Specialist, bounded to [0.05, 0.95].
- **Weather_Window**: A classification of satellite acquisition conditions (Calm, PostStorm, LowWater, ThermalContrast, SarTexture).
- **Drift_Correction**: Sub-pixel alignment parameters with a confidence gate of >= 0.8 before compute dispatch.
- **Pipeline_Handle**: A unique identifier returned to the user for tracking a scan request through all stages.
- **Node_Capabilities**: Hardware profile (GPU model, VRAM, compute capability, CPU cores) reported by a worker at startup.

## Requirements

### Requirement 1: Temporal Coverage Enforcement

**User Story:** As a scan operator, I want the orchestrator to enforce a hard minimum of 20 unique days of temporal stacking, so that wreck detection results are reliable and not based on insufficient data.

#### Acceptance Criteria

1. WHEN a scan request is submitted, THE Persistence_Enforcer SHALL verify that the Temporal_Stack contains at least 20 unique calendar days (UTC) of imagery before dispatching compute work
2. IF the available tile inventory contains fewer than 20 unique days, THEN THE Persistence_Enforcer SHALL attempt to download additional tiles to reach the 20-day minimum
3. IF the total achievable day count (existing + downloadable) is fewer than 20, THEN THE Persistence_Enforcer SHALL reject the pipeline with an error containing the current day count, maximum achievable count, and a suggested expanded date range
4. THE Persistence_Enforcer SHALL never allow a pipeline to reach compute dispatch status with fewer than 20 unique days in the Temporal_Stack
5. WHEN counting unique days, THE Persistence_Enforcer SHALL treat two tiles acquired on the same UTC calendar day as a single unique day regardless of sensor type or time-of-day

### Requirement 2: Cloud Cover and Weather Tagging

**User Story:** As a scan operator, I want optical tiles filtered by cloud cover and all tiles tagged with weather conditions, so that the detection pipeline uses only usable imagery with proper context.

#### Acceptance Criteria

1. WHEN building a Temporal_Stack, THE Orchestrator SHALL reject optical sensor tiles (Sentinel-2, Landsat 8, Landsat 9) with cloud cover exceeding 30 percent
2. WHEN building a Temporal_Stack, THE Orchestrator SHALL never reject SAR tiles (Sentinel-1) based on cloud cover percentage
3. WHEN a tile is added to the Temporal_Stack, THE Orchestrator SHALL assign a Weather_Window classification to that tile
4. WHEN assigning tile weights, THE Orchestrator SHALL assign weight 3.0 to PostStorm day-1 tiles, weight 1.0 to Calm tiles, and weight 0.5 to transitional tiles
5. WHEN validating weather diversity, THE Orchestrator SHALL require at least 2 of 4 weather categories (Calm, PostStorm, ThermalContrast, SarTexture) to be present in the Temporal_Stack

### Requirement 3: nautivecs Context Injection

**User Story:** As a system architect, I want workers to receive domain-specific expertise via nautivecs vector injection per-task, so that generic LLMs become specialists without model fine-tuning or permanent role assignment.

#### Acceptance Criteria

1. WHEN the Orchestrator dispatches a task to a Worker, THE nautivecs_Engine SHALL query the JSON vector store for domain-relevant code fragments matching the assigned role
2. WHEN injecting context into a Sensor_Specialist, THE nautivecs_Engine SHALL retrieve fragments related to band ratios, threshold detection, glint analysis, and weather window classification
3. WHEN injecting context into a Geometry_Specialist, THE nautivecs_Engine SHALL retrieve fragments related to sub-pixel grid alignment, drift correction, and coordinate transformations
4. THE nautivecs_Engine SHALL never inject context that exceeds the token budget for the target model (4096 tokens for 7B models, 2048 tokens for 3B models)
5. WHEN building the injection payload, THE nautivecs_Engine SHALL order fragments by relevance with task-specific context first, then domain knowledge
6. IF the nautivecs JSON store is corrupted or missing, THEN THE Worker SHALL fall back to unsteered mode and notify the Orchestrator of degraded specialist quality

### Requirement 4: Detection Threshold Validation

**User Story:** As a pipeline operator, I want all LLM-generated detection thresholds validated against sane bounds, so that hallucinated extreme values never reach the compute engine.

#### Acceptance Criteria

1. WHEN the Sensor_Specialist produces Detection_Thresholds, THE Worker SHALL validate that glint, hydrocarbon, and thermal anomaly thresholds are within the range [0.05, 0.95]
2. WHEN the Sensor_Specialist produces Detection_Thresholds, THE Worker SHALL validate that the confidence floor is within the range [0.1, 0.9]
3. IF threshold validation fails on the first attempt, THEN THE Worker SHALL retry inference with a more constrained prompt containing explicit bounds
4. IF threshold validation fails on the second attempt, THEN THE Worker SHALL fall back to conservative defaults (0.3 for all thresholds) and log the failure

### Requirement 5: Hardware Discovery and Dynamic Scheduling

**User Story:** As a cluster operator, I want workers to probe their own hardware at startup and register capabilities with the orchestrator, so that task scheduling adapts to whatever nodes are actually online.

#### Acceptance Criteria

1. WHEN a Worker starts, THE Worker SHALL probe its GPU hardware (model, VRAM, compute capability) and CPU resources (core count, available memory)
2. WHEN hardware probing completes, THE Worker SHALL register its Node_Capabilities with the Orchestrator via the Transport layer
3. WHEN the Orchestrator receives a Node_Capabilities registration, THE Orchestrator SHALL update its scheduling table to include the new worker
4. WHEN a previously registered Worker fails a health check, THE Orchestrator SHALL mark that worker as unavailable and redistribute its pending tasks to remaining healthy workers
5. WHEN all specialist workers are unavailable, THE Orchestrator SHALL attempt to perform specialist tasks itself using its own GPU and nautivecs injection (overflow mode)

### Requirement 6: Worker Fault Tolerance

**User Story:** As a system operator, I want the pipeline to continue functioning when individual nodes go offline, so that losing a node degrades performance but does not halt the system.

#### Acceptance Criteria

1. WHEN a Worker fails to respond to a health check within 5 seconds, THE Orchestrator SHALL retry once after a 5-second backoff
2. IF a Worker remains unreachable after retry, THEN THE Orchestrator SHALL mark the worker as offline and log the failure with a timestamp
3. WHEN a Worker comes back online, THE Orchestrator SHALL detect it via periodic mDNS/Tailscale discovery and restore it to the scheduling pool
4. WHEN the Compute_Engine (T440) is offline, THE Orchestrator SHALL queue compute work and continue accepting new requests for translation, temporal validation, and specialist tuning
5. IF the Compute_Engine remains offline, THEN THE Orchestrator SHALL attempt to dispatch compute work to alternative wgpu-capable nodes (cesarops2 GTX 1080) with reduced tile count

### Requirement 7: Transport and Inter-Node Communication

**User Story:** As a developer, I want all inter-node communication to flow over the Tailscale mesh with proper timeouts and retries, so that the distributed system is reliable and secure.

#### Acceptance Criteria

1. THE Transport layer SHALL route all inter-node RPC calls over Tailscale WireGuard-encrypted connections
2. WHEN sending a task dispatch to a Worker, THE Transport layer SHALL enforce a timeout of 120 seconds for specialist tasks
3. WHEN sending a compute dispatch to the Compute_Engine, THE Transport layer SHALL enforce a timeout of 600 seconds
4. WHEN a transport call times out, THE Transport layer SHALL return a structured error containing the target node address, elapsed time, and task identifier
5. WHEN a node registers its endpoint, THE Transport layer SHALL use mDNS combined with Tailscale peer discovery for automatic node detection

### Requirement 8: Drift Correction Confidence Gate

**User Story:** As a pipeline operator, I want compute dispatch blocked when drift correction confidence is below threshold, so that misaligned grids never produce garbage detection results.

#### Acceptance Criteria

1. WHEN the Geometry_Specialist produces a Drift_Correction, THE Orchestrator SHALL verify that the confidence value is at least 0.8 before dispatching compute work
2. IF Drift_Correction confidence is below 0.8, THEN THE Orchestrator SHALL reject the compute dispatch and return an error indicating insufficient alignment confidence
3. WHEN Drift_Correction confidence is exactly 0.8, THE Orchestrator SHALL accept the correction and proceed with compute dispatch

### Requirement 9: Pipeline Lifecycle Management

**User Story:** As a scan operator, I want to submit requests, track progress through named stages, and cancel running pipelines, so that I have full visibility and control over scan operations.

#### Acceptance Criteria

1. WHEN a user submits a scan request, THE Orchestrator SHALL return a Pipeline_Handle containing a unique identifier and estimated duration
2. WHEN a pipeline progresses through stages, THE Orchestrator SHALL update the Pipeline_Status to reflect the current stage (Queued, Translating, EnforcingTemporal, DownloadingTiles, TuningParameters, ValidatingGeometry, Computing, Interpreting, Complete, Failed)
3. WHEN a user requests pipeline cancellation, THE Orchestrator SHALL halt all in-progress work for that pipeline and release associated resources
4. IF a pipeline fails at any stage, THEN THE Orchestrator SHALL record the failure reason and the stage at which failure occurred

### Requirement 10: Compute Engine Execution (T440 — Deferred Testing)

**User Story:** As a pipeline operator, I want the T440 compute engine to execute multi-pass wgpu/WGSL anomaly detection across the full tile stack, so that the heavy GPU math runs on dedicated hardware.

#### Acceptance Criteria

1. WHEN the Compute_Engine receives a ComputeWorkPackage, THE Compute_Engine SHALL execute all specified passes (Scout, SyntheticTiling, Analyst, TemporalStitch) in sequence
2. WHEN executing a compute pass, THE Compute_Engine SHALL compile and dispatch WGSL shaders via wgpu 29.x on the P100 GPUs
3. WHEN a compute pipeline completes, THE Compute_Engine SHALL release all GPU memory allocated for shader buffers
4. THE Compute_Engine SHALL return one PassResult per pass with anomaly_confidence values in the range [0.0, 1.0]
5. IF a GPU encounters an out-of-memory condition during compute, THEN THE Compute_Engine SHALL abort the current pass and return a structured error

### Requirement 11: Unique Day Counting

**User Story:** As a developer, I want a deterministic function for counting unique days in a tile collection, so that temporal coverage calculations are consistent and testable.

#### Acceptance Criteria

1. THE count_unique_days function SHALL return the number of distinct UTC calendar days across all tiles in the input collection
2. WHEN two tiles have acquisition timestamps on the same UTC calendar day, THE count_unique_days function SHALL count them as one unique day
3. WHEN the input collection is empty, THE count_unique_days function SHALL return zero
4. THE count_unique_days function SHALL return a value less than or equal to the number of tiles in the input collection

### Requirement 12: Stack Validation and Construction

**User Story:** As a developer, I want stack construction to enforce all validation rules atomically, so that no invalid stack ever reaches downstream components.

#### Acceptance Criteria

1. WHEN building a Temporal_Stack, THE build_validated_stack function SHALL filter out optical tiles exceeding the cloud cover maximum while preserving all SAR tiles
2. IF filtering reduces the unique day count below 20, THEN THE build_validated_stack function SHALL return an error rather than producing an invalid stack
3. WHEN constructing a Temporal_Stack, THE build_validated_stack function SHALL compute WeatherDiversity from the final filtered tile set
4. THE build_validated_stack function SHALL assign a weight to every tile based on its Weather_Window classification before returning the stack
