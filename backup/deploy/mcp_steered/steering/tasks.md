# Implementation Plan: Segmented Context Manager (SCM)

## Overview

Extend the existing SCM module (`cesarops-mcp-steered/src/scm/`) from its current foundation (drift monitoring, feedback dispatch, context pruning, basic executor) into the full closed-loop pipeline described in the design. The existing 6 modules and 24 passing tests remain intact; new modules are added alongside them, and existing types are expanded to support JSON-LD schema compliance, input/output guardrails, cross-segment validation, and pipeline coordination.

## Tasks

- [x] 1. Expand RSU schema for full JSON-LD compliance
  - [x] 1.1 Extend `rsu.rs` with JSON-LD fields and full design types
    - Add `@context` and `@type` serde-renamed fields to `RsuTask`
    - Add `RsuMeta` with `steering_ref`, `steering_policy`, `context_ttl_seconds`, `created_at` fields
    - Add `RsuExecution` struct with `action` and `parameters` fields
    - Add `dependencies: Vec<String>` field
    - Add `SegmentMode` enum (Precision/Exploratory) alongside existing `SteeringMode`
    - Implement `From<SegmentMode>` for `SteeringMode` for backward compatibility
    - Ensure existing tests still pass after expansion
    - _Requirements: 1.1, 1.2, 1.5, 1.6_

  - [x] 1.2 Implement RSU JSON-LD parsing and validation
    - Write `Rsu::from_json_ld(value: &serde_json::Value) -> Result<Rsu>` that validates required fields
    - Reject payloads with unexpected fields (Requirement 1.3) using `#[serde(deny_unknown_fields)]` or manual validation
    - Validate `thinking_budget` is one of low/medium/high
    - Return descriptive errors for missing required fields (id, meta.parent_goal, phases.observation, phases.reasoning, phases.accuracy_check)
    - _Requirements: 1.1, 1.3, 1.5, 1.6_

  - [x] 1.3 Implement RSU serialization to JSON-LD
    - Write `Rsu::to_json_ld(&self) -> serde_json::Value` that produces conformant output
    - Ensure `@context` is set to `https://kiro.ai` and `@type` is set to `SegmentTask`
    - _Requirements: 1.2, 1.4_

  - [x] 1.4 Write unit tests for RSU round-trip and validation
    - Test parse → serialize → parse produces equivalent struct (Requirement 1.4)
    - Test rejection of unknown fields
    - Test rejection of missing required fields
    - Test thinking_budget validation
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6_

- [x] 2. Implement FidelityCheck schema parsing
  - [x] 2.1 Create `validator.rs` with FidelityCheck types
    - Define `RuleSeverity` enum (Blocker, Warning)
    - Define `ValidationRule` struct (rule, description, severity)
    - Define `MonitoringConfig` struct (threshold, rolling_window, escalation_threshold)
    - Define `FidelityCheck` struct with `@context`, `@type`, validation_rules, monitoring
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.6_

  - [x] 2.2 Implement FidelityCheck parsing and validation
    - Write `FidelityCheck::from_json_ld(value: &serde_json::Value) -> Result<FidelityCheck>`
    - Validate severity is only `blocker` or `warning`
    - Validate monitoring.threshold is 0.0–1.0 inclusive
    - Return descriptive errors for incomplete rules (missing rule/description/severity)
    - _Requirements: 7.1, 7.3, 7.4, 7.6_

  - [x] 2.3 Implement FidelityCheck serialization
    - Write `FidelityCheck::to_json_ld(&self) -> serde_json::Value`
    - Ensure `@context` = `https://kiro.ai`, `@type` = `FidelityCheck`
    - _Requirements: 7.2, 7.5_

  - [x] 2.4 Write unit tests for FidelityCheck round-trip
    - Test parse → serialize → parse equivalence (Requirement 7.5)
    - Test rejection of invalid severity values
    - Test rejection of threshold outside 0.0–1.0
    - Test rejection of incomplete rules
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6_

- [x] 3. Implement the Segmenter (objective decomposition)
  - [x] 3.1 Create `segmenter.rs` with decomposition logic
    - Define `Segmenter` struct that holds a reference to `LlmClient`
    - Implement `decompose(objective: &str, llm: &LlmClient) -> Result<Vec<Rsu>>` that calls the LLM to break an objective into 2–10 RSUs
    - Assign unique IDs following `segment_NNN` pattern (zero-padded)
    - Set `parent_goal` on every RSU to the original objective
    - Assign `thinking_budget` based on estimated complexity
    - Populate `phases.observation` with context dependencies from prior segments
    - Ensure no duplicate IDs in the sequence (Requirement 2.6)
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

  - [x] 3.2 Write unit tests for Segmenter
    - Test ID uniqueness across generated RSUs
    - Test parent_goal propagation
    - Test RSU count is within 2–10 range
    - Test segment_NNN ID format
    - _Requirements: 2.1, 2.2, 2.3, 2.6_

- [x] 4. Implement the Steering Controller
  - [x] 4.1 Create `steering_ctrl.rs` with think-prefix injection
    - Define `SteeringController` struct
    - Implement `steer_and_execute()` that builds the steered prompt for an RSU
    - Prepend think-prefix referencing `phases.reasoning` content (Requirement 3.1)
    - Include `phases.accuracy_check` as explicit constraint in prefix (Requirement 3.2)
    - Call `SteeringEngine::build_context()` with RSU's reasoning as query and domain as `role_hint` (Requirements 3.3, 10.1, 10.4)
    - Enforce non-bypassable guarantee: all LLM calls must go through this controller (Requirement 3.7)
    - _Requirements: 3.1, 3.2, 3.3, 3.7, 10.1, 10.4_

  - [x] 4.2 Implement budget enforcement in SteeringController
    - Set max_tokens based on `thinking_budget`: low=512, medium=2048, high=4096 for reasoning phase (Requirements 3.4, 3.5, 3.6)
    - Enforce total token budget across all phases: low=1024, medium=4096, high=8192 (Requirements 9.1, 9.2, 9.3)
    - Implement truncation and budget-exhausted marking when limit is hit (Requirement 9.4)
    - _Requirements: 3.4, 3.5, 3.6, 9.1, 9.2, 9.3, 9.4, 9.5_

  - [x] 4.3 Implement SteeringEngine integration details
    - Handle `needs_human_approval` flag from SteeringContext — pause and emit low-confidence warning (Requirement 10.2)
    - Include corrections from correction store with highest priority in think-prefix (Requirement 10.3)
    - Respect SteeringEngine's context_budget limit (Requirement 10.5)
    - _Requirements: 10.2, 10.3, 10.5_

  - [x] 4.4 Write unit tests for SteeringController
    - Test think-prefix contains reasoning and accuracy_check content
    - Test max_tokens is set correctly per budget tier
    - Test non-bypassable guarantee (no direct LLM path)
    - _Requirements: 3.1, 3.2, 3.4, 3.5, 3.6, 3.7_

- [x] 5. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 6. Implement Input and Output Guardrails
  - [x] 6.1 Create `guardrail.rs` with InputGuardrail
    - Define `InputGuardrail` struct with `permitted_actions: HashSet<String>` initialized to writeFile, readFile, runCommand, queryNautivecs
    - Implement `validate(rsu: &Rsu, prior_outputs: &[SegmentOutput]) -> Result<()>`
    - Compare RSU's `phases.observation` against `parent_goal` for semantic drift (Requirement 4.1)
    - Reject RSUs referencing goals/contexts not in parent_goal or prior outputs (Requirement 4.2)
    - Verify `execution.action` is in permitted set (Requirement 4.3)
    - Reject unauthorized actions with descriptive error (Requirement 4.4)
    - Target <50ms for RSUs under 1000 chars (Requirement 4.5)
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [x] 6.2 Implement OutputGuardrail
    - Define `OutputGuardrail` struct
    - Implement `validate(output: &str, rsu: &Rsu, fidelity_check: &FidelityCheck, monitor: &dyn DriftMonitor) -> Result<ValidationResult>`
    - Load FidelityCheck rules from `steering_ref` path (Requirement 5.1)
    - Enforce `strict_schema_enforcement` rule — reject output with undefined fields (Requirement 5.2)
    - Enforce `canonical_serialization` rule — verify tool calls use trusted serializer (Requirement 5.3)
    - Compute Logical_Drift score 0.0–1.0 (Requirement 5.4)
    - Reject output if drift exceeds threshold (0.05) and flag for reprocessing (Requirement 5.5)
    - Halt pipeline on blocker severity failure (Requirement 5.6)
    - Log warning severity failures but continue (Requirement 5.7)
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 5.7_

  - [x] 6.3 Write unit tests for guardrails
    - Test InputGuardrail rejects unauthorized actions
    - Test InputGuardrail detects observation drift from parent_goal
    - Test OutputGuardrail halts on blocker failure
    - Test OutputGuardrail logs but continues on warning failure
    - Test drift score computation returns 0.0–1.0
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 5.4, 5.5, 5.6, 5.7_

- [x] 7. Implement Cross-Segment Validator
  - [x] 7.1 Add `CrossSegmentValidator` to `validator.rs`
    - Implement `validate(segment_output: &str, original_objective: &str, prior_outputs: &[SegmentOutput]) -> Result<CrossValidationResult>`
    - Compare Segment N's output against original objective constraints (Requirement 6.1)
    - Verify output does not contradict any prior segment output (Requirement 6.5)
    - Return `CrossValidationResult::Pass` or `CrossValidationResult::Fail` with violation detail
    - _Requirements: 6.1, 6.4, 6.5_

  - [x] 7.2 Write unit tests for CrossSegmentValidator
    - Test pass case with consistent output
    - Test fail case with contradicting prior segment
    - Test fail case with output deviating from objective
    - _Requirements: 6.1, 6.5_

- [x] 8. Implement the Monitor (metrics and rolling drift)
  - [x] 8.1 Create `monitor.rs` with metrics tracking
    - Define `Monitor` struct with `drift_window: VecDeque<f32>` and `metrics: Vec<SegmentMetrics>`
    - Define `SegmentMetrics` struct (rsu_id, drift_score, tokens_used, retries, elapsed_ms, segment_mode, status, timestamp)
    - Define `MonitoringSummary` struct (total_segments_processed, average_drift, total_retries, budget_exhaustion_count, segments_by_mode)
    - Implement `record(metrics: SegmentMetrics)` to log segment completion (Requirement 11.1)
    - Implement `rolling_drift_average(window: usize) -> f32` over last 5 segments (Requirement 11.2)
    - Implement `should_increase_steering() -> bool` when rolling avg > 0.03 (Requirement 11.3)
    - Flag domains requiring >1 retry as potential weaknesses (Requirement 11.4)
    - Implement `query_metrics() -> MonitoringSummary` for structured query interface (Requirement 11.5)
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5_

  - [x] 8.2 Write unit tests for Monitor
    - Test rolling average computation over window
    - Test should_increase_steering triggers at 0.03 threshold
    - Test query_metrics returns correct summary
    - _Requirements: 11.1, 11.2, 11.3, 11.5_

- [x] 9. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 10. Implement Pipeline Coordinator (phase execution and orchestration)
  - [x] 10.1 Create `pipeline.rs` with PipelineCoordinator struct
    - Define `PipelineCoordinator` struct holding Segmenter, SteeringController, InputGuardrail, OutputGuardrail, CrossSegmentValidator, Monitor
    - Define `PipelineResult`, `PipelineState`, `SegmentOutput`, `SegmentStatus` types
    - Define `Phase` enum (Observation, Reasoning, AccuracyCheck)
    - Define `PhaseOutput` struct (phase, content, tokens_used, elapsed_ms)
    - _Requirements: 8.1, 8.5, 6.3, 12.3_

  - [x] 10.2 Implement phase sequence execution
    - Implement strict observation → reasoning → accuracy_check ordering (Requirements 8.1, 8.2, 8.3, 8.5)
    - Gather context from prior segments and nautivecs in observation phase (Requirement 8.1)
    - Use observation output as input to reasoning phase (Requirement 8.2)
    - Verify reasoning output in accuracy_check phase (Requirement 8.3)
    - If accuracy_check fails, reprocess reasoning with violation feedback in steering vector (Requirement 8.4)
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5_

  - [x] 10.3 Implement pipeline orchestration with retry and cross-validation
    - Call InputGuardrail before each RSU execution
    - Call OutputGuardrail after each RSU execution
    - Call CrossSegmentValidator between segments (Requirement 6.1)
    - On cross-validation failure, reprocess with augmented steering vector (Requirement 6.2)
    - Enforce max 3 retries before pipeline failure (Requirement 6.3)
    - Make Segment N output available as input context for Segment N+1 (Requirement 6.4)
    - Record metrics via Monitor after each segment
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 11.1_

  - [x] 10.4 Implement error handling and pipeline resume
    - Retry phase once after 2-second backoff on LlmClient error (Requirement 12.1)
    - Mark segment as failed and halt with structured failure report after retry failure (Requirement 12.2)
    - Preserve completed segment outputs for resume capability (Requirement 12.3)
    - Proceed with empty steering vector if nautivecs unreachable, mark as unsteered (Requirement 12.4)
    - Lower drift threshold to 0.02 for unsteered segments (Requirement 12.5)
    - Implement `resume(state: PipelineState)` to continue from failed segment
    - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.5_

  - [x] 10.5 Implement context TTL and VRAM eviction
    - Track context creation timestamps per segment
    - Evict expired contexts based on `context_ttl_seconds` from RSU meta
    - Use TTL defaults: low=15s, medium=30s, high=60s
    - Integrate with ContextPruner for hardware-aware memory management
    - _Requirements: 9.5_

  - [x] 10.6 Write unit tests for pipeline execution
    - Test strict phase ordering (observation before reasoning before accuracy_check)
    - Test retry logic with max 3 retries
    - Test pipeline halt on blocker fidelity failure
    - Test resume from saved PipelineState
    - Test unsteered segment gets stricter threshold
    - _Requirements: 8.5, 6.3, 12.1, 12.2, 12.3, 12.4, 12.5_

- [x] 11. Wire modules together and update mod.rs
  - [x] 11.1 Update `mod.rs` to export new modules
    - Add `pub mod segmenter;`, `pub mod steering_ctrl;`, `pub mod guardrail;`, `pub mod validator;`, `pub mod pipeline;`, `pub mod monitor;`
    - Add re-exports for primary public types (PipelineCoordinator, Rsu, FidelityCheck, MonitoringSummary)
    - Ensure all modules compile together without circular dependencies
    - _Requirements: 10.1, 10.4_

  - [x] 11.2 Integrate SCM pipeline with existing MCP tool handlers
    - Add an MCP tool handler (or extend existing one) that accepts a high-level objective and runs the full pipeline
    - Wire `PipelineCoordinator::execute_objective()` into the MCP server's tool dispatch
    - Expose `Monitor::query_metrics()` as an MCP resource or tool for observability
    - _Requirements: 2.1, 11.5_

  - [x] 11.3 Write integration tests for end-to-end pipeline
    - Test full objective → decompose → execute → validate flow with mock LlmClient
    - Test pipeline handles LLM errors gracefully
    - Test monitoring metrics are recorded correctly across segments
    - _Requirements: 2.1, 8.1, 11.1, 12.1_

- [x] 12. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- The existing 6 modules (drift.rs, feedback.rs, stats.rs, rsu.rs, pruner.rs, executor.rs) and 24 passing tests are preserved — new code extends rather than replaces
- All storage uses nautivecs serverless JSON store (no LanceDB/Arrow)
- The LlmClient targets KoboldCPP on cesarops2 (Qwen2.5-Coder-3B via 1070)
- Context pruning is critical for Pascal-era GPUs (8GB VRAM on 1070)
- Unit tests validate specific examples and edge cases

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "2.1", "8.1"] },
    { "id": 1, "tasks": ["1.2", "1.3", "2.2", "2.3", "8.2"] },
    { "id": 2, "tasks": ["1.4", "2.4", "3.1", "4.1"] },
    { "id": 3, "tasks": ["3.2", "4.2", "4.3", "6.1"] },
    { "id": 4, "tasks": ["4.4", "6.2", "7.1"] },
    { "id": 5, "tasks": ["6.3", "7.2", "10.1"] },
    { "id": 6, "tasks": ["10.2", "10.3"] },
    { "id": 7, "tasks": ["10.4", "10.5"] },
    { "id": 8, "tasks": ["10.6", "11.1"] },
    { "id": 9, "tasks": ["11.2"] },
    { "id": 10, "tasks": ["11.3"] }
  ]
}
```
