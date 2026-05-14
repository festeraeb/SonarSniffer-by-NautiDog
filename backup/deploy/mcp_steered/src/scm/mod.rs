//! Segmented Context Manager (SCM)
//!
//! Decomposes complex prompts into RSUs (Region SPEC Units),
//! steers each segment's execution, and validates outputs against
//! fidelity rules before committing.
//!
//! Architecture:
//! - RSUProvider: converts objectives into structured RSU sequences
//! - SteeringController: injects think-prefixes, enforces budgets
//! - DriftMonitor: adaptive threshold checking (0.05 precision / 0.15 exploratory)
//! - FeedbackDispatcher: translates drift into retry/halt actions
//! - ContextPruner: strips context for low-VRAM hardware

pub mod drift;
pub mod executor;
pub mod feedback;
pub mod guardrail;
pub mod monitor;
pub mod pipeline;
pub mod pruner;
pub mod rsu;
pub mod segmenter;
pub mod stats;
pub mod steering_ctrl;
pub mod validator;

// ── Re-exports for primary public types ──────────────────────────────────────
// Users can import these directly from `crate::scm::` without navigating submodules.

// RSU types (rsu.rs)
pub use rsu::{Rsu, RsuMeta, RsuPhases, RsuExecution, ThinkingBudget, SegmentMode};

// Validator types (validator.rs)
pub use validator::{FidelityCheck, ValidationResult, CrossValidationResult};

// Monitor types (monitor.rs)
pub use monitor::{MonitoringSummary, SegmentMetrics, Monitor};

// Steering controller types (steering_ctrl.rs)
pub use steering_ctrl::{SteeringController, Phase, BudgetConfig};

// Guardrail types (guardrail.rs)
pub use guardrail::{InputGuardrail, OutputGuardrail, SegmentOutput};

// Segmenter (segmenter.rs)
pub use segmenter::Segmenter;

// Pipeline types (pipeline.rs)
pub use pipeline::{PipelineCoordinator, PipelineResult, PipelineState};
