//! Dynamic Tool Routing Database — Context-Enriched Tool Registry
//!
//! Instead of stuffing all tool schemas into the LLM system prompt (causing
//! context bloat and attention rot), we store "Usage Recipes" with semantic
//! anchors and inject only the top-N matching recipes per request.
//!
//! Architecture:
//!   1. ToolRecipe — stores schema + how-to-use + hardware guards
//!   2. ToolIndex  — in-memory cosine similarity search over semantic anchors
//!   3. ToolRouter — compiles enriched system prompts with only relevant tools
//!
//! This directly protects the P100/Xeon hardware separation by embedding
//! hardware warnings into the recipes themselves.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::{info, warn};

// ── Tool Recipe ───────────────────────────────────────────────────────────────

/// A complete "how-to-use" record for a single tool.
/// Contains everything a worker LLM needs to call the tool correctly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRecipe {
    /// Unique tool identifier (e.g. "curvelet_forward", "dipole_scan_f32")
    pub tool_name: String,

    /// Raw JSON/function schema — the exact parameters the tool accepts.
    pub native_schema: String,

    /// Human-readable description of what this tool does and when to use it.
    /// This is the primary field used for semantic matching.
    pub semantic_anchor: String,

    /// A concrete example of a successful invocation with real parameters.
    pub execution_example: String,

    /// Hardware constraint warning — injected verbatim into the prompt.
    /// e.g. "Requires FP64 precision. DO NOT route to GTX 1070 or P1000."
    pub hardware_guard: String,

    /// Which node(s) can run this tool.
    pub valid_targets: Vec<String>,

    /// Tags for fast pre-filtering before semantic search.
    pub tags: Vec<String>,

    /// How many times this tool was successfully invoked (for ranking).
    pub success_count: u32,

    /// Average latency in milliseconds (for routing decisions).
    pub avg_latency_ms: u32,
}

// ── Bag-of-Words Embedding (no external deps) ────────────────────────────────

/// Simple TF-based vector for cosine similarity.
/// We don't need a neural embedder for 50-100 tools — bag-of-words with
/// domain-specific vocabulary is fast and accurate enough.
#[derive(Debug, Clone)]
struct BowVector {
    /// Sparse representation: word_hash → tf weight
    terms: HashMap<u64, f32>,
    norm: f32,
}

impl BowVector {
    fn from_text(text: &str) -> Self {
        let mut terms: HashMap<u64, f32> = HashMap::new();
        for word in text.to_lowercase().split_whitespace() {
            let word = word.trim_matches(|c: char| !c.is_alphanumeric());
            if word.len() < 2 { continue; }
            let hash = Self::hash_word(word);
            *terms.entry(hash).or_insert(0.0) += 1.0;
        }
        let norm = terms.values().map(|v| v * v).sum::<f32>().sqrt().max(1e-10);
        Self { terms, norm }
    }

    fn cosine_similarity(&self, other: &BowVector) -> f32 {
        let dot: f32 = self.terms.iter()
            .filter_map(|(k, v)| other.terms.get(k).map(|ov| v * ov))
            .sum();
        dot / (self.norm * other.norm)
    }

    fn hash_word(word: &str) -> u64 {
        // FNV-1a hash — fast, good distribution for short strings
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in word.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }
}

// ── Tool Index ────────────────────────────────────────────────────────────────

/// In-memory index of all tool recipes with pre-computed embeddings.
/// Supports fast top-N retrieval by semantic similarity.
pub struct ToolIndex {
    recipes: Vec<ToolRecipe>,
    embeddings: Vec<BowVector>,
}

impl ToolIndex {
    /// Create a new empty index.
    pub fn new() -> Self {
        Self {
            recipes: Vec::new(),
            embeddings: Vec::new(),
        }
    }

    /// Load recipes from a JSON file on disk.
    pub fn load_from_file(path: &Path) -> Self {
        let mut index = Self::new();
        if let Ok(data) = std::fs::read_to_string(path) {
            if let Ok(recipes) = serde_json::from_str::<Vec<ToolRecipe>>(&data) {
                for recipe in recipes {
                    index.add_recipe(recipe);
                }
                info!("ToolIndex: loaded {} recipes from {}", index.recipes.len(), path.display());
            } else {
                warn!("ToolIndex: failed to parse {}", path.display());
            }
        } else {
            info!("ToolIndex: no existing file at {} — starting fresh", path.display());
        }
        index
    }

    /// Add a recipe to the index, computing its embedding.
    pub fn add_recipe(&mut self, recipe: ToolRecipe) {
        // Combine semantic anchor + tags + tool name for the embedding
        let text = format!(
            "{} {} {} {}",
            recipe.tool_name,
            recipe.semantic_anchor,
            recipe.tags.join(" "),
            recipe.hardware_guard,
        );
        let embedding = BowVector::from_text(&text);
        self.embeddings.push(embedding);
        self.recipes.push(recipe);
    }

    /// Find the top-N most relevant recipes for a given task description.
    pub fn search(&self, query: &str, top_n: usize) -> Vec<&ToolRecipe> {
        let query_vec = BowVector::from_text(query);

        let mut scored: Vec<(usize, f32)> = self.embeddings.iter()
            .enumerate()
            .map(|(i, emb)| (i, query_vec.cosine_similarity(emb)))
            .collect();

        // Sort by similarity descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored.iter()
            .take(top_n)
            .filter(|(_, score)| *score > 0.05) // minimum relevance threshold
            .map(|(i, _)| &self.recipes[*i])
            .collect()
    }

    /// Find recipes by tag (exact match, fast pre-filter).
    pub fn by_tag(&self, tag: &str) -> Vec<&ToolRecipe> {
        self.recipes.iter()
            .filter(|r| r.tags.iter().any(|t| t == tag))
            .collect()
    }

    /// Find recipes valid for a specific target node.
    pub fn for_target(&self, target: &str) -> Vec<&ToolRecipe> {
        self.recipes.iter()
            .filter(|r| r.valid_targets.iter().any(|t| t == target))
            .collect()
    }

    /// Persist all recipes to a JSON file.
    pub fn save_to_file(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self.recipes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    /// Record a successful tool invocation (updates success_count and latency).
    pub fn record_success(&mut self, tool_name: &str, latency_ms: u32) {
        if let Some(recipe) = self.recipes.iter_mut().find(|r| r.tool_name == tool_name) {
            let total = recipe.avg_latency_ms as u64 * recipe.success_count as u64
                + latency_ms as u64;
            recipe.success_count += 1;
            recipe.avg_latency_ms = (total / recipe.success_count as u64) as u32;
        }
    }

    pub fn len(&self) -> usize {
        self.recipes.len()
    }
}

// ── Tool Router ───────────────────────────────────────────────────────────────

/// Compiles enriched system prompts with only the relevant tool recipes.
/// This is the pre-pass that runs before sending a request to the worker LLM.
pub struct ToolRouter {
    pub index: ToolIndex,
    /// Maximum number of tools to inject per request.
    pub max_tools_per_request: usize,
}

impl ToolRouter {
    pub fn new(index: ToolIndex) -> Self {
        Self {
            index,
            max_tools_per_request: 3,
        }
    }

    /// Given a task description, compile an enriched system prompt
    /// containing only the most relevant tool recipes.
    pub fn compile_enriched_prompt(&self, task_description: &str) -> String {
        let matched = self.index.search(task_description, self.max_tools_per_request);

        if matched.is_empty() {
            return Self::fallback_prompt();
        }

        let mut prompt = String::from(
            "You are an autonomous worker node. You only have access to the specific \
             tools documented below. Execute tool calls using strict syntax parameters.\n\n"
        );

        for recipe in &matched {
            prompt.push_str(&format!(
                "### TOOL: {}\n\
                 * SCHEMA: {}\n\
                 * USE CASE: {}\n\
                 * EXAMPLE CALL: {}\n\
                 * HARDWARE GUARD: {}\n\
                 * VALID TARGETS: {}\n\n",
                recipe.tool_name,
                recipe.native_schema,
                recipe.semantic_anchor,
                recipe.execution_example,
                recipe.hardware_guard,
                recipe.valid_targets.join(", "),
            ));
        }

        prompt
    }

    /// Minimal fallback when no tools match — prevents the LLM from hallucinating tools.
    fn fallback_prompt() -> String {
        String::from(
            "You are an autonomous worker node. No tools matched this request. \
             Respond with a plan of action and request clarification on which \
             tools are needed. Do NOT invent tool names or parameters.\n"
        )
    }

    /// Compile a prompt for a specific target node (only tools valid for that hardware).
    pub fn compile_for_target(&self, task_description: &str, target: &str) -> String {
        let target_recipes = self.index.for_target(target);
        if target_recipes.is_empty() {
            return Self::fallback_prompt();
        }

        // Further filter by semantic relevance
        let query_vec = BowVector::from_text(task_description);
        let mut scored: Vec<(&ToolRecipe, f32)> = target_recipes.iter()
            .map(|r| {
                let text = format!("{} {} {}", r.tool_name, r.semantic_anchor, r.tags.join(" "));
                let emb = BowVector::from_text(&text);
                (*r, query_vec.cosine_similarity(&emb))
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut prompt = String::from(
            "You are an autonomous worker node. You only have access to the specific \
             tools documented below. Execute tool calls using strict syntax parameters.\n\n"
        );

        for (recipe, _) in scored.iter().take(self.max_tools_per_request) {
            prompt.push_str(&format!(
                "### TOOL: {}\n\
                 * SCHEMA: {}\n\
                 * USE CASE: {}\n\
                 * EXAMPLE CALL: {}\n\
                 * HARDWARE GUARD: {}\n\n",
                recipe.tool_name,
                recipe.native_schema,
                recipe.semantic_anchor,
                recipe.execution_example,
                recipe.hardware_guard,
            ));
        }

        prompt
    }
}

// ── Default recipes for the CESAROPS cluster ──────────────────────────────────

/// Bootstrap the tool index with the known CESAROPS tools.
pub fn bootstrap_cesarops_recipes() -> Vec<ToolRecipe> {
    vec![
        ToolRecipe {
            tool_name: "curvelet_forward".into(),
            native_schema: r#"{"input_signal": "[f64]", "output_grid": "[f64]", "len": "usize"}"#.into(),
            semantic_anchor: "Curvelet forward transform for sub-surface anomaly detection. \
                Sequential math with data dependencies. Meyer window application on frequency-domain wedges.".into(),
            execution_example: r#"curvelet_forward(input_signal: &magnetic_grid[0..65536], output_grid: &mut result[0..65536])"#.into(),
            hardware_guard: "REQUIRES FP64 precision and sequential execution. \
                Route ONLY to Xeon AVX-512 (XeonCpuBackend). \
                DO NOT route to GPU — WGSL does not support f64. \
                DO NOT route to GTX 1070, P1000, or P106 (1:32 FP64 rate).".into(),
            valid_targets: vec!["t440_xeon".into()],
            tags: vec!["curvelet".into(), "fdct".into(), "fp64".into(), "sequential".into(), "precision".into()],
            success_count: 0,
            avg_latency_ms: 0,
        },
        ToolRecipe {
            tool_name: "richardson_weighting".into(),
            native_schema: r#"{"depth_layers": "[f64]", "weight_matrix": "[f64]"}"#.into(),
            semantic_anchor: "Richardson Number depth-layer weighting for oceanographic stability analysis. \
                Strictly sequential — iteration i depends on i-1. Cannot be parallelised.".into(),
            execution_example: r#"richardson_weighting(depth_layers: &layers[0..128], weight_matrix: &mut weights[0..128])"#.into(),
            hardware_guard: "SEQUENTIAL dependency chain — each layer depends on the previous. \
                Route ONLY to Xeon AVX-512. GPU cannot help here. \
                Requires FP64 for Brunt-Väisälä frequency precision.".into(),
            valid_targets: vec!["t440_xeon".into()],
            tags: vec!["richardson".into(), "oceanography".into(), "fp64".into(), "sequential".into()],
            success_count: 0,
            avg_latency_ms: 0,
        },
        ToolRecipe {
            tool_name: "dipole_scan_f32".into(),
            native_schema: r#"{"input_grid": "[f32]", "output_scores": "[f32]", "width": "u32", "height": "u32"}"#.into(),
            semantic_anchor: "Parallel dipole pixel scan for aeromagnetic anomaly detection. \
                Embarrassingly parallel — each pixel independent. Inner/outer annulus analysis. \
                Detects magnetic dipole signatures from submerged ferrous objects.".into(),
            execution_example: r#"dipole_scan_f32(input_grid: &mag_tile[0..4194304], output_scores: &mut scores, width: 2048, height: 2048)"#.into(),
            hardware_guard: "EMBARRASSINGLY PARALLEL — route to P100 GPU cluster (wgpu compute shader). \
                Uses f32 only. Safe on any GPU including 1070, P1000, P106. \
                Optimal on P100 with 32x32 workgroups filling 56 SMs.".into(),
            valid_targets: vec!["t440_p100".into(), "cesarops2_1070".into(), "cesarops3_p106".into()],
            tags: vec!["dipole".into(), "scan".into(), "parallel".into(), "gpu".into(), "f32".into(), "magnetic".into()],
            success_count: 0,
            avg_latency_ms: 0,
        },
        ToolRecipe {
            tool_name: "geotransform_wgs84_to_enu".into(),
            native_schema: r#"{"inputs": "[Wgs84Coord]", "anchor": "Wgs84Coord", "outputs": "[LocalEnuCoord]"}"#.into(),
            semantic_anchor: "WGS84 to local East-North-Up coordinate transformation. \
                Batch geodetic reprojection for scan tile positioning. \
                Iterative transcendental math with catastrophic cancellation risk.".into(),
            execution_example: r#"batch_wgs84_to_local_enu(inputs: &gps_coords, anchor: scan_center, outputs: &mut local_grid)"#.into(),
            hardware_guard: "REQUIRES FP64 — catastrophic cancellation in ECEF subtraction \
                destroys sub-metre accuracy if done in f32. \
                Route to Xeon AVX-512 (8 doubles/cycle, chunk size 8 for zmm alignment). \
                DO NOT cast to f32 at any intermediate step.".into(),
            valid_targets: vec!["t440_xeon".into()],
            tags: vec!["geotransform".into(), "wgs84".into(), "enu".into(), "fp64".into(), "geodetic".into()],
            success_count: 0,
            avg_latency_ms: 0,
        },
        ToolRecipe {
            tool_name: "llm_inference".into(),
            native_schema: r#"{"prompt": "string", "max_tokens": "u32", "temperature": "f32"}"#.into(),
            semantic_anchor: "LLM text generation via KoboldCPP. Qwen3.6-35B MoE on dual P100s. \
                Code generation, research synthesis, reasoning, planning.".into(),
            execution_example: r#"POST http://100.72.182.77:5001/v1/chat/completions {"model":"default","messages":[...],"max_tokens":512}"#.into(),
            hardware_guard: "Runs on T440 dual P100 16GB (32GB total HBM2). \
                Only available when P100s are in LLM mode (not spatial scanning). \
                Check LLM_MODE_ACTIVE flag before routing. \
                If P100s are in spatial mode, queue the request until mode flips back.".into(),
            valid_targets: vec!["t440_p100".into()],
            tags: vec!["llm".into(), "inference".into(), "qwen".into(), "coding".into(), "reasoning".into()],
            success_count: 0,
            avg_latency_ms: 0,
        },
        ToolRecipe {
            tool_name: "tile_ingest_geotiff".into(),
            native_schema: r#"{"file_path": "string", "band": "u32", "tile_size": "u32"}"#.into(),
            semantic_anchor: "Load a GeoTIFF tile from disk, extract a single band, \
                and stage it for GPU processing. I/O bound then DMA to P100 HBM2.".into(),
            execution_example: r#"tile_ingest_geotiff(file_path: "/mnt/data-external/tiles/mag_survey_001.tif", band: 1, tile_size: 4096)"#.into(),
            hardware_guard: "I/O phase runs on Xeon (disk read). \
                Staging phase uses coordinator.page_out_llm_and_stage_spatial() to DMA into P100. \
                Ensure P100 is NOT in LLM mode before staging — check LLM_MODE_ACTIVE.".into(),
            valid_targets: vec!["t440_xeon".into(), "t440_p100".into()],
            tags: vec!["geotiff".into(), "tile".into(), "ingest".into(), "io".into(), "staging".into()],
            success_count: 0,
            avg_latency_ms: 0,
        },
        ToolRecipe {
            tool_name: "research_cycle".into(),
            native_schema: r#"{"domain": "ResearchDomain"}"#.into(),
            semantic_anchor: "Run one research cycle — query arXiv, Semantic Scholar for papers \
                related to wreck detection, SAR processing, bathymetry, hydrocarbon recognition. \
                Stores grounded findings, proposes testable hypotheses.".into(),
            execution_example: r#"research_engine.run_cycle().await  // rotates through domains automatically"#.into(),
            hardware_guard: "Network I/O + LLM synthesis. Runs on Xeon (HTTP requests) \
                then calls LLM for hypothesis generation. \
                Low priority — only run during idle time when no scan jobs are queued.".into(),
            valid_targets: vec!["t440_xeon".into()],
            tags: vec!["research".into(), "arxiv".into(), "papers".into(), "idle".into(), "overnight".into()],
            success_count: 0,
            avg_latency_ms: 0,
        },
        ToolRecipe {
            tool_name: "anomaly_classify".into(),
            native_schema: r#"{"anomaly": "SubSurfaceAnomalyMetadata", "context_tiles": "[TileRef]"}"#.into(),
            semantic_anchor: "Classify a detected anomaly as wreck, geological, or noise. \
                Uses dipole separation, lobe symmetry, phase coherence, and geographic context. \
                May call LLM for ambiguous cases.".into(),
            execution_example: r#"anomaly_classify(anomaly: &detected, context_tiles: &[nearby_mag, nearby_spectral])"#.into(),
            hardware_guard: "Light compute — runs on Xeon. \
                If LLM consultation needed for ambiguous cases, requires P100 in LLM mode. \
                Queue LLM requests if P100s are busy with spatial scanning.".into(),
            valid_targets: vec!["t440_xeon".into(), "cesarops2_1070".into()],
            tags: vec!["classify".into(), "anomaly".into(), "wreck".into(), "decision".into()],
            success_count: 0,
            avg_latency_ms: 0,
        },
    ]
}
