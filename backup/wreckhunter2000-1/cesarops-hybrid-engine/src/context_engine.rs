//! Context Injection Engine — Dynamic Knowledge Compilation for Worker Agents
//!
//! Instead of fine-tuning or static system prompts, this engine compiles
//! task-specific context from multiple sources and injects it into the
//! LLM request. A 7B model with perfect context outperforms a 70B model
//! with generic instructions.
//!
//! Architecture:
//!   1. Task arrives (natural language + metadata)
//!   2. ContextEngine queries multiple knowledge sources:
//!      - ToolRecipe DB (what tools are available + how to use them)
//!      - Scan History DB (what's been done before on this bbox)
//!      - Weather State (current conditions for the target area)
//!      - Hardware State (what nodes are available, what's loaded)
//!      - Domain Rules (steering docs, scan strategy, cluster ops)
//!      - Research Findings (latest papers + hypotheses)
//!   3. Reranker selects top-N most relevant items per source
//!   4. Compiler assembles a structured injection payload
//!   5. Payload is prepended to the LLM request as system + context messages
//!
//! The result: a small model receives exactly the knowledge it needs,
//! formatted exactly how it needs it, with hardware guards and examples.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::info;

use crate::tool_db::{ToolIndex, ToolRecipe, ToolRouter};

// ── Context Sources ───────────────────────────────────────────────────────────

/// All the knowledge sources the engine can pull from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSources {
    /// Relevant tool recipes (top-N by semantic match)
    pub tools: Vec<ToolRecipeSnippet>,
    /// Recent scan history for the target bbox
    pub scan_history: Vec<ScanHistoryEntry>,
    /// Current weather conditions
    pub weather: Option<WeatherContext>,
    /// Hardware state (what's available right now)
    pub hardware: HardwareContext,
    /// Domain rules (from steering docs)
    pub rules: Vec<String>,
    /// Research findings (if relevant to the task)
    pub research: Vec<ResearchSnippet>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRecipeSnippet {
    pub tool_name: String,
    pub use_case: String,
    pub example: String,
    pub hardware_guard: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanHistoryEntry {
    pub date: String,
    pub weather_condition: String,
    pub sensor: String,
    pub detection_count: u32,
    pub max_confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherContext {
    pub condition: String,       // calm, post_storm_1, etc.
    pub wind_speed_mph: f32,
    pub wave_height_m: f32,
    pub recommendation: String,  // "Good for optical" / "SAR only" / etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareContext {
    pub available_nodes: Vec<NodeStatus>,
    pub llm_mode_active: bool,
    pub p100_free_vram_gb: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub name: String,
    pub role: String,
    pub gpu: String,
    pub vram_free_gb: f32,
    pub is_online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSnippet {
    pub title: String,
    pub technique: String,
    pub relevance: String,
}

// ── Injection Payload ─────────────────────────────────────────────────────────

/// The compiled context that gets injected into the LLM request.
/// This replaces the static system prompt with dynamic, task-specific knowledge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionPayload {
    /// Role definition (who the model is for this task)
    pub role: String,
    /// Task-specific rules (subset of all rules, relevant to this task)
    pub rules: Vec<String>,
    /// Tool schemas + examples (only the ones needed)
    pub tools: Vec<ToolRecipeSnippet>,
    /// Situational awareness (weather, hardware, history)
    pub situation: String,
    /// The actual task instruction
    pub task: String,
    /// Output format requirements
    pub output_format: String,
}

impl InjectionPayload {
    /// Compile into a system message string for the LLM.
    pub fn to_system_message(&self) -> String {
        let mut msg = String::with_capacity(4096);

        // Role
        msg.push_str(&format!("ROLE: {}\n\n", self.role));

        // Rules (numbered for clarity)
        if !self.rules.is_empty() {
            msg.push_str("RULES:\n");
            for (i, rule) in self.rules.iter().enumerate() {
                msg.push_str(&format!("{}. {}\n", i + 1, rule));
            }
            msg.push('\n');
        }

        // Tools
        if !self.tools.is_empty() {
            msg.push_str("AVAILABLE TOOLS:\n");
            for tool in &self.tools {
                msg.push_str(&format!(
                    "  [{tool_name}]\n    USE: {use_case}\n    EXAMPLE: {example}\n    GUARD: {guard}\n\n",
                    tool_name = tool.tool_name,
                    use_case = tool.use_case,
                    example = tool.example,
                    guard = tool.hardware_guard,
                ));
            }
        }

        // Situation
        if !self.situation.is_empty() {
            msg.push_str(&format!("CURRENT SITUATION:\n{}\n\n", self.situation));
        }

        // Output format
        if !self.output_format.is_empty() {
            msg.push_str(&format!("OUTPUT FORMAT: {}\n\n", self.output_format));
        }

        msg
    }

    /// Compile into OpenAI-compatible messages array.
    pub fn to_messages(&self, user_message: &str) -> Vec<ChatMessage> {
        vec![
            ChatMessage {
                role: "system".to_string(),
                content: self.to_system_message(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_message.to_string(),
            },
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

// ── Context Engine ────────────────────────────────────────────────────────────

/// The main engine that compiles context for each task.
pub struct ContextEngine {
    pub tool_router: ToolRouter,
    /// Domain rules loaded from steering docs
    pub domain_rules: Vec<DomainRule>,
    /// LLM endpoint for the reranker (optional — uses heuristics if unavailable)
    pub llm_endpoint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DomainRule {
    pub category: String,    // "scan_strategy", "hardware", "coding", "cluster_ops"
    pub rule: String,
    pub keywords: Vec<String>,
}

impl ContextEngine {
    pub fn new(tool_index: ToolIndex, rules: Vec<DomainRule>) -> Self {
        Self {
            tool_router: ToolRouter::new(tool_index),
            domain_rules: rules,
            llm_endpoint: None,
        }
    }

    /// Compile a full injection payload for a given task.
    ///
    /// This is the core function — it decides what knowledge the model needs
    /// based on the task description and current system state.
    pub fn compile_context(
        &self,
        task_description: &str,
        task_type: TaskCategory,
        bbox: Option<[f64; 4]>,
    ) -> InjectionPayload {
        // 1. Select role based on task type
        let role = match task_type {
            TaskCategory::ScanPlanning => "You are the CESAROPS Scan Planner — an elite remote sensing specialist. You decide which satellites to query, which dates to download, and how to build temporal stacks for wreck detection. Prioritize post-storm plume days and thermal contrast windows. Never rely on a single pass.".to_string(),
            TaskCategory::CodeGeneration => "You are the primary reasoning kernel for WreckHunter-2000, specialized in high-performance Rust architecture. Write strictly valid, production-grade Rust. Prioritize zero-copy patterns, type-safety, and cache friendliness. No heap allocations in hot loops if slices are possible. Use wgpu for GPU, tokio for async, rayon for CPU. Never cast f64 to f32 in precision math.".to_string(),
            TaskCategory::ResearchSynthesis => "You are a remote sensing research analyst for CESAROPS. Cross-reference sonar backscatter, magnetometer dipoles, multibeam bathymetry, and satellite imagery. Propose testable improvements grounded in real papers. Never assert findings — only propose experiments.".to_string(),
            TaskCategory::AnomalyClassification => "You are the CESAROPS anomaly classifier. Fuse side-scan sonar geometry, magnetometer dipole spikes, and multibeam micro-topography to classify detections. Look for hard linear edges, right-angle returns, hull-shaped shadows, and ferrous dipole signatures that deviate from natural geology.".to_string(),
            TaskCategory::DataAcquisition => "You are the CESAROPS data acquisition specialist. Determine optimal satellite sources based on target profile and weather windows. Steel vessels need thermal + magnetic. Recent sinkings need before/after change detection. Always stack 20+ dates with weather diversity.".to_string(),
            TaskCategory::SystemOperation => "You are the CESAROPS cluster operator. Use scripts/swap_model.sh for model changes. Always stop systemd services before killing processes. Verify GPU memory is free before loading models. Log every operation.".to_string(),
        };

        // 2. Select relevant rules (keyword match against task)
        let rules = self.select_rules(task_description, &task_type);

        // 3. Select relevant tools (semantic search)
        let tools: Vec<ToolRecipeSnippet> = self.tool_router.index
            .search(task_description, 3)
            .iter()
            .map(|r| ToolRecipeSnippet {
                tool_name: r.tool_name.clone(),
                use_case: r.semantic_anchor.clone(),
                example: r.execution_example.clone(),
                hardware_guard: r.hardware_guard.clone(),
            })
            .collect();

        // 4. Build situation string
        let situation = self.build_situation(bbox);

        // 5. Determine output format
        let output_format = match task_type {
            TaskCategory::CodeGeneration => "Return only compilable Rust code with minimal comments. No markdown fences unless showing a complete file.".to_string(),
            TaskCategory::ScanPlanning => "Return a JSON object with: {sensors: [...], dates: [...], weather_filter: {...}, priority_order: [...]}".to_string(),
            TaskCategory::AnomalyClassification => "Return JSON: {classification: string, confidence: float, reasoning: string}".to_string(),
            TaskCategory::ResearchSynthesis => "Return: 1) Key technique (2 sentences), 2) How it improves CESAROPS (2 sentences), 3) Specific test to run (1 sentence)".to_string(),
            _ => "Be concise and actionable.".to_string(),
        };

        InjectionPayload {
            role,
            rules,
            tools,
            situation,
            task: task_description.to_string(),
            output_format,
        }
    }

    /// Select rules relevant to this task using keyword matching.
    fn select_rules(&self, task: &str, category: &TaskCategory) -> Vec<String> {
        let task_lower = task.to_lowercase();
        let cat_str = match category {
            TaskCategory::ScanPlanning => "scan_strategy",
            TaskCategory::CodeGeneration => "coding",
            TaskCategory::SystemOperation => "cluster_ops",
            _ => "",
        };

        self.domain_rules.iter()
            .filter(|r| {
                // Match by category
                if !cat_str.is_empty() && r.category == cat_str {
                    return true;
                }
                // Match by keywords in task
                r.keywords.iter().any(|kw| task_lower.contains(&kw.to_lowercase()))
            })
            .take(5) // Max 5 rules per injection
            .map(|r| r.rule.clone())
            .collect()
    }

    /// Build a situation awareness string from current system state.
    fn build_situation(&self, bbox: Option<[f64; 4]>) -> String {
        let mut parts = Vec::new();

        if let Some(bb) = bbox {
            parts.push(format!(
                "Target area: [{:.3}, {:.3}] to [{:.3}, {:.3}]",
                bb[0], bb[1], bb[2], bb[3]
            ));
        }

        // In production, this would query live state from the cluster
        parts.push("Hardware: T440 (dual P100 32GB, Xeon AVX-512), cesarops2 (1070 8GB + P1000 4GB)".to_string());

        parts.join("\n")
    }
}

// ── Task Categories ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum TaskCategory {
    ScanPlanning,
    CodeGeneration,
    ResearchSynthesis,
    AnomalyClassification,
    DataAcquisition,
    SystemOperation,
}

impl TaskCategory {
    /// Infer task category from the task description.
    pub fn infer(task: &str) -> Self {
        let t = task.to_lowercase();
        if t.contains("scan") || t.contains("download") || t.contains("tile") || t.contains("stack") {
            Self::ScanPlanning
        } else if t.contains("write") || t.contains("implement") || t.contains("code") || t.contains("function") || t.contains("struct") {
            Self::CodeGeneration
        } else if t.contains("paper") || t.contains("research") || t.contains("arxiv") || t.contains("improve") {
            Self::ResearchSynthesis
        } else if t.contains("classify") || t.contains("anomaly") || t.contains("detection") || t.contains("wreck or") {
            Self::AnomalyClassification
        } else if t.contains("fetch") || t.contains("sentinel") || t.contains("landsat") || t.contains("satellite") {
            Self::DataAcquisition
        } else if t.contains("restart") || t.contains("deploy") || t.contains("service") || t.contains("node") {
            Self::SystemOperation
        } else {
            Self::CodeGeneration // default
        }
    }
}

// ── Bootstrap domain rules from steering docs ─────────────────────────────────

/// Load domain rules from the steering markdown files.
/// In production, these would be parsed from .kiro/steering/*.md
pub fn bootstrap_domain_rules() -> Vec<DomainRule> {
    vec![
        // Scan strategy rules
        DomainRule {
            category: "scan_strategy".into(),
            rule: "Stack 20+ days of tiles per target area — never rely on a single pass".into(),
            keywords: vec!["scan".into(), "tile".into(), "download".into(), "stack".into()],
        },
        DomainRule {
            category: "scan_strategy".into(),
            rule: "Post-storm days (1-3 after N/NW winds >20mph for 24h) catch sediment plumes from wreck structures".into(),
            keywords: vec!["storm".into(), "plume".into(), "weather".into(), "post_storm".into()],
        },
        DomainRule {
            category: "scan_strategy".into(),
            rule: "Calm days (wind <5mph, Jul-Oct) give optical clarity for hull outlines and sun glint".into(),
            keywords: vec!["calm".into(), "optical".into(), "clear".into(), "glint".into()],
        },
        DomainRule {
            category: "scan_strategy".into(),
            rule: "Thermal contrast days reveal steel heat sinks — hot sunny days after cold nights".into(),
            keywords: vec!["thermal".into(), "heat".into(), "steel".into(), "temperature".into()],
        },
        DomainRule {
            category: "scan_strategy".into(),
            rule: "SAR always works through clouds — use for texture anomalies and coherence change".into(),
            keywords: vec!["sar".into(), "cloud".into(), "radar".into(), "sentinel-1".into()],
        },
        DomainRule {
            category: "scan_strategy".into(),
            rule: "Never duplicate the same weather category stack from a previous scan of the same area".into(),
            keywords: vec!["history".into(), "repeat".into(), "diversity".into(), "previous".into()],
        },
        // Hardware rules
        DomainRule {
            category: "hardware".into(),
            rule: "All precision math (f64) runs on Xeon AVX-512 — NEVER on GPU (WGSL has no f64)".into(),
            keywords: vec!["f64".into(), "precision".into(), "curvelet".into(), "geodetic".into()],
        },
        DomainRule {
            category: "hardware".into(),
            rule: "Embarrassingly parallel work (dipole scan, pixel sweeps) routes to P100 GPU cluster".into(),
            keywords: vec!["parallel".into(), "gpu".into(), "dipole".into(), "pixel".into()],
        },
        DomainRule {
            category: "hardware".into(),
            rule: "P100s flip between LLM mode and Spatial mode — check LLM_MODE_ACTIVE before staging tiles".into(),
            keywords: vec!["p100".into(), "mode".into(), "spatial".into(), "llm".into()],
        },
        // Coding rules
        DomainRule {
            category: "coding".into(),
            rule: "Write Rust when possible. Use wgpu for GPU, tokio for async, rayon for CPU parallelism.".into(),
            keywords: vec!["rust".into(), "code".into(), "implement".into(), "write".into()],
        },
        DomainRule {
            category: "coding".into(),
            rule: "Never cast f64 to f32 in geodetic or curvelet calculations — catastrophic cancellation".into(),
            keywords: vec!["f64".into(), "f32".into(), "cast".into(), "precision".into()],
        },
        DomainRule {
            category: "coding".into(),
            rule: "No raw CUDA. No Python for compute-heavy work. wgpu/Vulkan only for GPU.".into(),
            keywords: vec!["cuda".into(), "gpu".into(), "compute".into(), "shader".into()],
        },
        // Cluster ops rules
        DomainRule {
            category: "cluster_ops".into(),
            rule: "systemd services with Restart=always respawn in 10s — MUST stop the unit first, then kill".into(),
            keywords: vec!["kill".into(), "restart".into(), "service".into(), "systemd".into()],
        },
        DomainRule {
            category: "cluster_ops".into(),
            rule: "Model swaps: stop service → wait 3s for VRAM free → start new model. CUDA OOM = old process still holding memory.".into(),
            keywords: vec!["model".into(), "swap".into(), "vram".into(), "oom".into()],
        },
        DomainRule {
            category: "cluster_ops".into(),
            rule: "Use scripts/swap_model.sh for model changes — sources credentials.sh for sudo access".into(),
            keywords: vec!["swap".into(), "model".into(), "kobold".into(), "load".into()],
        },
    ]
}
