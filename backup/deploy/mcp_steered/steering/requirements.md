# Requirements Document

## Introduction

The Segmented Context Manager (SCM) is the core orchestration module that decomposes complex prompts into structured sub-tasks (RSUs — Region SPEC Units), steers each segment's execution toward accuracy via think-prefixes, and validates outputs against fidelity rules before committing results. It integrates with the existing `SteeringEngine` and `nautivecs` infrastructure in the `cesarops-mcp-steered` crate, extending the current "Librarian Pattern" from research decomposition to full prompt execution management. The SCM enforces a closed-loop of decomposition → steered execution → cross-segment validation, ensuring no segment drifts from the original intent.

## Glossary

- **SCM**: The Segmented Context Manager — the top-level module coordinating segmentation, steering, and validation.
- **RSU**: Region SPEC Unit — a JSON-LD structured record defining a single atomic sub-task with metadata, phases, and execution parameters.
- **Segmenter**: The component that decomposes a high-level objective into an ordered sequence of RSUs.
- **Steering_Controller**: The non-bypassable layer that monitors every segment's proposal, applies think-prefixes, and enforces budget guidance before execution proceeds.
- **Validator**: The external safety layer that checks segment outputs against fidelity rules defined in steering files.
- **Thinking_Budget**: A classification (low, medium, high) that controls how much reasoning effort the model applies to a given RSU, balancing efficiency with accuracy.
- **Fidelity_Check**: A set of validation rules (with severity levels) that determine whether a segment's output is acceptable.
- **Logical_Drift**: A numeric metric (0.0–1.0) measuring how far a segment's output has deviated from the original intent constraints.
- **Drift_Threshold**: The maximum acceptable Logical_Drift value (default 0.05) before a segment must be reprocessed.
- **Input_Guardrail**: The pre-execution check that inspects an RSU's prompt for drift or injection before it reaches the LLM.
- **Output_Guardrail**: The post-execution check that validates a segment's response against fidelity rules before committing.
- **Steering_Vector**: The nautivecs-injected context payload tailored to a specific RSU's domain requirements.
- **Phase_Sequence**: The ordered execution of observation → reasoning → accuracy_check within a single RSU.
- **Cross_Segment_Validation**: The process of comparing Segment N's output against the original constraints before Segment N+1 begins.
- **SteeringEngine**: The existing nautivecs-based context injection system in `cesarops-mcp-steered/src/steering.rs`.
- **LlmClient**: The existing OpenAI-compatible LLM client in `cesarops-mcp-steered/src/llm_client.rs`.

## Requirements

### Requirement 1: RSU Schema Parsing and Serialization

**User Story:** As a developer, I want RSUs to be parsed from and serialized to a well-defined JSON-LD schema, so that segment contracts are machine-readable, validated at boundaries, and round-trip safely through the system.

#### Acceptance Criteria

1. WHEN a JSON-LD payload conforming to the SegmentTask schema is provided, THE Segmenter SHALL parse it into a typed RSU struct containing id, meta, phases, and execution fields
2. WHEN an RSU struct is serialized, THE Segmenter SHALL produce a JSON-LD payload that conforms to the SegmentTask schema with `@context` set to `https://kiro.ai` and `@type` set to `SegmentTask`
3. IF a JSON-LD payload contains fields not defined in the SegmentTask schema, THEN THE Segmenter SHALL reject the payload with an error identifying the unexpected fields
4. FOR ALL valid RSU structs, parsing then serializing then parsing SHALL produce an equivalent RSU struct (round-trip property)
5. WHEN the `thinking_budget` field in an RSU's meta is provided, THE Segmenter SHALL validate that it is one of: `low`, `medium`, or `high`
6. IF a required field (id, meta.parent_goal, phases.observation, phases.reasoning, phases.accuracy_check) is missing, THEN THE Segmenter SHALL return a descriptive error identifying the missing field

### Requirement 2: Objective Decomposition into RSUs

**User Story:** As a system operator, I want high-level objectives automatically decomposed into ordered atomic RSUs, so that complex tasks are broken into manageable, steerable units without manual segmentation.

#### Acceptance Criteria

1. WHEN a high-level objective string is submitted, THE Segmenter SHALL decompose it into an ordered sequence of 2 to 10 RSUs
2. WHEN generating RSUs, THE Segmenter SHALL assign each RSU a unique identifier following the pattern `segment_NNN` where NNN is a zero-padded sequence number
3. WHEN generating RSUs, THE Segmenter SHALL set the `parent_goal` field of every RSU to the original high-level objective text
4. WHEN generating RSUs, THE Segmenter SHALL assign a `thinking_budget` to each RSU based on estimated complexity (low for lookups, medium for logic, high for architecture decisions)
5. WHEN generating RSUs, THE Segmenter SHALL populate the `phases.observation` field with the specific context dependencies from prior segments
6. WHEN the decomposition produces RSUs, THE Segmenter SHALL ensure that no two RSUs in the same sequence share the same identifier

### Requirement 3: Steering Controller Think-Prefix Injection

**User Story:** As a system architect, I want every RSU's execution prefixed with a steering prompt that guides the model toward accurate reasoning, so that off-target thinking is prevented before it reaches the execution layer.

#### Acceptance Criteria

1. WHEN an RSU enters the execution phase, THE Steering_Controller SHALL prepend a think-prefix to the LLM prompt that references the RSU's `phases.reasoning` content
2. WHEN constructing the think-prefix, THE Steering_Controller SHALL include the RSU's `phases.accuracy_check` as an explicit constraint the model must satisfy
3. THE Steering_Controller SHALL query the existing SteeringEngine to build a Steering_Vector from nautivecs fragments relevant to the RSU's domain
4. WHEN the `thinking_budget` is `low`, THE Steering_Controller SHALL set the LLM max_tokens to 512 for the reasoning phase
5. WHEN the `thinking_budget` is `medium`, THE Steering_Controller SHALL set the LLM max_tokens to 2048 for the reasoning phase
6. WHEN the `thinking_budget` is `high`, THE Steering_Controller SHALL set the LLM max_tokens to 4096 for the reasoning phase
7. THE Steering_Controller SHALL never allow an RSU to reach the LlmClient without a think-prefix attached (non-bypassable guarantee)

### Requirement 4: Input Guardrail — Pre-Execution Drift Detection

**User Story:** As a safety operator, I want every RSU's prompt inspected for drift or injection before it reaches the LLM, so that corrupted or adversarial segments are caught early.

#### Acceptance Criteria

1. WHEN an RSU is submitted for execution, THE Input_Guardrail SHALL compare the RSU's `phases.observation` content against the `parent_goal` to detect semantic drift
2. IF the Input_Guardrail detects that the RSU's observation references goals or contexts not present in the parent_goal or prior segment outputs, THEN THE Input_Guardrail SHALL reject the RSU with a drift error
3. WHEN inspecting an RSU, THE Input_Guardrail SHALL verify that the `execution.action` field contains only permitted actions (writeFile, readFile, runCommand, queryNautivecs)
4. IF the `execution.action` contains an action not in the permitted set, THEN THE Input_Guardrail SHALL reject the RSU with an unauthorized action error
5. THE Input_Guardrail SHALL complete its inspection within 50 milliseconds for RSUs with fewer than 1000 characters of combined phase content

### Requirement 5: Output Guardrail — Post-Execution Fidelity Validation

**User Story:** As a safety operator, I want every segment's output validated against fidelity rules before it is committed, so that hallucinated or drifted responses never propagate to downstream segments.

#### Acceptance Criteria

1. WHEN a segment produces output, THE Output_Guardrail SHALL load the FidelityCheck rules from the steering_ref path specified in the RSU's meta
2. WHEN the `strict_schema_enforcement` rule is active, THE Output_Guardrail SHALL reject any output containing fields or structures not defined in the validated schema
3. WHEN the `canonical_serialization` rule is active, THE Output_Guardrail SHALL verify that any tool calls in the output are regenerated through the trusted serializer
4. WHEN computing Logical_Drift, THE Validator SHALL produce a numeric score between 0.0 and 1.0 representing deviation from the original constraints
5. IF the Logical_Drift score exceeds the Drift_Threshold (0.05), THEN THE Validator SHALL reject the output and flag the segment for reprocessing
6. WHEN a fidelity rule with severity `blocker` fails, THE Validator SHALL halt the pipeline and prevent any further segments from executing
7. WHEN a fidelity rule with severity `warning` fails, THE Validator SHALL log the violation but allow the pipeline to continue

### Requirement 6: Cross-Segment Validation

**User Story:** As a pipeline operator, I want each segment's output validated against the original constraints before the next segment begins, so that errors do not compound across the segment chain.

#### Acceptance Criteria

1. WHEN Segment N completes successfully, THE Validator SHALL compare Segment N's output against the original high-level objective constraints before Segment N+1 is permitted to start
2. IF Cross_Segment_Validation fails, THEN THE SCM SHALL reprocess Segment N with an augmented steering vector that includes the specific constraint violation
3. WHEN reprocessing a failed segment, THE SCM SHALL increment a retry counter and allow a maximum of 3 retries before escalating to a pipeline failure
4. WHEN Cross_Segment_Validation passes, THE SCM SHALL make Segment N's output available as input context for Segment N+1's `phases.observation`
5. THE Cross_Segment_Validation SHALL verify that Segment N's output does not contradict any output from Segments 1 through N-1

### Requirement 7: Fidelity Check Schema Parsing and Serialization

**User Story:** As a developer, I want FidelityCheck rules parsed from and serialized to a well-defined JSON-LD schema, so that validation configurations are machine-readable and round-trip safely.

#### Acceptance Criteria

1. WHEN a JSON-LD payload conforming to the FidelityCheck schema is provided, THE Validator SHALL parse it into a typed struct containing validation_rules and monitoring fields
2. WHEN a FidelityCheck struct is serialized, THE Validator SHALL produce a JSON-LD payload with `@context` set to `https://kiro.ai` and `@type` set to `FidelityCheck`
3. IF a validation_rule entry is missing the `rule`, `description`, or `severity` field, THEN THE Validator SHALL return a descriptive error identifying the incomplete rule
4. THE Validator SHALL accept only `blocker` and `warning` as valid severity values
5. FOR ALL valid FidelityCheck structs, parsing then serializing then parsing SHALL produce an equivalent FidelityCheck struct (round-trip property)
6. WHEN the `monitoring.threshold` field is provided, THE Validator SHALL validate that it is a float between 0.0 and 1.0 inclusive

### Requirement 8: Phase Sequence Execution

**User Story:** As a system operator, I want RSU phases executed in strict order (observation → reasoning → accuracy_check), so that each cognitive step builds on the previous one and the accuracy check always runs last.

#### Acceptance Criteria

1. WHEN executing an RSU, THE SCM SHALL process the `phases.observation` phase first, gathering context from prior segments and nautivecs
2. WHEN the observation phase completes, THE SCM SHALL process the `phases.reasoning` phase using the observation output as input context
3. WHEN the reasoning phase completes, THE SCM SHALL process the `phases.accuracy_check` phase to verify the reasoning output satisfies the stated constraint
4. IF the accuracy_check phase determines the reasoning violates its constraint, THEN THE SCM SHALL reprocess the reasoning phase with the violation feedback included in the steering vector
5. THE SCM SHALL never execute the reasoning phase before observation, or accuracy_check before reasoning (strict ordering guarantee)

### Requirement 9: Budget Guidance and Efficiency Control

**User Story:** As a system architect, I want the SCM to enforce thinking budgets per segment, so that simple tasks complete quickly while complex tasks receive adequate reasoning depth.

#### Acceptance Criteria

1. WHEN an RSU has `thinking_budget` set to `low`, THE Steering_Controller SHALL constrain total LLM token generation across all phases to 1024 tokens
2. WHEN an RSU has `thinking_budget` set to `medium`, THE Steering_Controller SHALL constrain total LLM token generation across all phases to 4096 tokens
3. WHEN an RSU has `thinking_budget` set to `high`, THE Steering_Controller SHALL constrain total LLM token generation across all phases to 8192 tokens
4. IF an LLM response reaches the token limit for its budget tier, THEN THE Steering_Controller SHALL truncate the response and mark the segment as budget-exhausted
5. WHEN a segment is marked budget-exhausted, THE SCM SHALL log the event and proceed to the accuracy_check phase with the truncated output

### Requirement 10: Integration with Existing SteeringEngine

**User Story:** As a developer, I want the SCM to use the existing SteeringEngine and nautivecs infrastructure for context injection, so that the system builds on proven grounding mechanisms rather than duplicating them.

#### Acceptance Criteria

1. WHEN building a Steering_Vector for an RSU, THE SCM SHALL call `SteeringEngine::build_context` with the RSU's reasoning content as the query parameter
2. WHEN the SteeringEngine returns a SteeringContext with `needs_human_approval` set to true, THE SCM SHALL pause the segment and emit a low-confidence warning before proceeding
3. WHEN the SteeringEngine returns corrections via the correction store, THE SCM SHALL include those corrections in the think-prefix with highest priority
4. THE SCM SHALL pass the RSU's execution domain as the `role_hint` parameter to `SteeringEngine::build_context` for targeted fragment retrieval
5. THE SCM SHALL respect the SteeringEngine's existing context_budget and not request fragments exceeding the configured token limit

### Requirement 11: Closed-Loop Monitoring

**User Story:** As a system operator, I want continuous evaluation of reasoning behaviors across segments, so that systematic drift patterns are detected and corrected before they compound.

#### Acceptance Criteria

1. WHEN a segment completes, THE SCM SHALL record the segment's Logical_Drift score, token usage, retry count, and elapsed time in a monitoring log
2. WHILE a pipeline is executing, THE SCM SHALL compute a rolling average of Logical_Drift across the last 5 completed segments
3. IF the rolling average Logical_Drift exceeds 0.03, THEN THE SCM SHALL increase the steering intensity for subsequent segments by including additional constraint reminders in the think-prefix
4. WHEN a segment requires more than 1 retry, THE SCM SHALL flag the segment's domain as a potential weakness and log it for future steering file updates
5. THE SCM SHALL expose monitoring metrics (total segments processed, average drift, total retries, budget exhaustion count) via a structured query interface

### Requirement 12: Pipeline Error Handling and Recovery

**User Story:** As a system operator, I want the SCM to handle failures gracefully at every stage, so that partial progress is preserved and failures produce actionable diagnostics.

#### Acceptance Criteria

1. IF the LlmClient returns an error during any phase execution, THEN THE SCM SHALL retry the phase once after a 2-second backoff
2. IF the retry also fails, THEN THE SCM SHALL mark the segment as failed, record the error, and halt the pipeline with a structured failure report
3. WHEN a pipeline is halted due to failure, THE SCM SHALL preserve all completed segment outputs so that the pipeline can be resumed from the failed segment
4. IF the nautivecs store is unreachable during Steering_Vector construction, THEN THE SCM SHALL proceed with an empty steering vector and mark the segment as unsteered
5. WHEN a segment is marked as unsteered, THE SCM SHALL increase the accuracy_check strictness by lowering the Drift_Threshold to 0.02 for that segment
