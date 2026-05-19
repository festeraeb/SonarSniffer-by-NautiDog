//! Shader Synthesizer — hardware-adaptive kernel generation pipeline
//!
//! Detects GPU class, scans existing shaders, picks quantization strategy,
//! builds prompts for Gemma, compiles SPIR-V, benchmarks, and evolves.

pub mod gpu_probe;
pub mod shader_scan;
pub mod quant_policy;
pub mod prompt_builder;
pub mod compiler;
pub mod benchmark_db;
