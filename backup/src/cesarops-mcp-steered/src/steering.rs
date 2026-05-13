//! Steering Engine — nautivecs context injection for anti-drift grounding
//!
//! Core innovation: before every LLM call, we query nautivecs for code
//! fragments relevant to the current task, then inject them into the system
//! prompt. The LLM can still think creatively, but it's anchored to real
//! code, real functions, real parameters from YOUR codebase.
//!
//! Anti-drift properties:
//! - LLM sees actual function signatures, not hallucinated ones
//! - LLM sees actual parameter ranges from real code
//! - Creative reasoning preserved — injection is additive, not restrictive
//!
//! Correction injection (from Gemini bounce):
//! - Human feedback stored as high-priority fragments (score=2.0)
//! - Corrections injected FIRST with [CRITICAL: PREVIOUS HUMAN CORRECTION] prefix
//! - Scoped corrections only apply when scope matches current query context
//!
//! Confidence gating:
//! - If best match score < 0.7, engine flags "low confidence"
//! - Caller decides: ask human via n8n, or proceed with warning
//!
//! RRF (Reciprocal Rank Fusion):
//! - Multiple queries per user question (raw, technical terms, usage, role-specific)
//! - Results merged via RRF for 360-degree context view

use anyhow::Result;
use chrono::{DateTime, Utc};
use nautivecs::{Config as NautivecsConfig, NautivecsEngine, InjectedContextBuilder, ContextFragment};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ── Public Types ─────────────────────────────────────────────────────────────

/// Steering context built from nautivecs queries + corrections
pub struct SteeringContext {
    pub system_prompt: String,
    pub fragments_used: usize,
    pub corrections_applied: usize,
    /// Raw correction text for injection into think-prefix (highest priority)
    pub corrections_text: String,
    pub query_terms: Vec<String>,
    pub confidence: f32,
    pub needs_human_approval: bool,
}

/// A human correction stored in the correction store
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Correction {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub tool_name: String,
    pub original_query: String,
    pub llm_response_summary: String,
    pub feedback: FeedbackType,
    pub correction_text: String,
    pub scope: Option<CorrectionScope>,
    pub priority: f32,
    pub applied_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FeedbackType {
    Correct,
    Wrong,
    TooAggressive,
    TooConservative,
    FunctionDoesNotExist,
    DriftedOffTopic,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionScope {
    pub weather_window: Option<String>,
    pub sensor: Option<String>,
    pub region: Option<String>,
    pub tool_name: Option<String>,
}

// ── Steering Engine ──────────────────────────────────────────────────────────

pub struct SteeringEngine {
    engine: NautivecsEngine,
    corrections: Vec<Correction>,
    corrections_path: String,
    context_budget: usize,
}

impl SteeringEngine {
    pub async fn init(db_path: &str, embedding_url: &str, context_budget: usize) -> Result<Self> {
        let config = NautivecsConfig::builder()
            .embedding_endpoint(embedding_url)
            .db_path(db_path)
            .vector_dimensions(768)
            .build();

        let engine = NautivecsEngine::init(config).await?;
        let corrections_path = db_path.replace(".json", "_corrections.json");
        let corrections = Self::load_corrections(&corrections_path);

        tracing::info!(
            "SteeringEngine: {} chunks, {} corrections, budget={}",
            engine.chunk_count(), corrections.len(), context_budget
        );

        Ok(Self { engine, corrections, corrections_path, context_budget })
    }

    /// Build steered system prompt with RRF multi-query + corrections + confidence gate
    pub async fn build_context(
        &mut self,
        query: &str,
        role_hint: Option<&str>,
        tool_name: Option<&str>,
    ) -> Result<SteeringContext> {
        // 1. Find applicable corrections (highest priority)
        let applicable_corrections = self.find_corrections(query, tool_name);

        // 2. Multi-query search with RRF fusion
        let search_queries = self.derive_search_queries(query, role_hint);
        let mut scored_fragments: Vec<(ContextFragment, f32)> = Vec::new();

        for search_query in &search_queries {
            match self.engine.query(search_query, 3).await {
                Ok(results) => {
                    for (rank, result) in results.iter().enumerate() {
                        let fragment = ContextFragment::from(result);
                        let rrf_score = 1.0 / (60.0 + rank as f32);
                        scored_fragments.push((fragment, rrf_score));
                    }
                }
                Err(_) => {
                    let results = self.engine.query_keyword(search_query, 3);
                    for (rank, result) in results.iter().enumerate() {
                        let fragment = ContextFragment::from(result);
                        let rrf_score = 1.0 / (60.0 + rank as f32);
                        scored_fragments.push((fragment, rrf_score));
                    }
                }
            }
        }

        // 3. RRF merge — deduplicate, sum scores
        let merged = self.rrf_merge(scored_fragments);

        // 4. Confidence gate
        let confidence = merged.first().map(|f| f.search_score).unwrap_or(0.0);
        let needs_human_approval = confidence < 0.7;

        if needs_human_approval {
            tracing::warn!("Low confidence ({:.2}) for: '{}'", confidence, query);
        }

        // 5. Build prompt — corrections FIRST, then code context
        let correction_text = if !applicable_corrections.is_empty() {
            let mut text = String::from("\n## [CRITICAL: PREVIOUS HUMAN CORRECTIONS]\n\n");
            for c in &applicable_corrections {
                text.push_str(&format!(
                    "- **{}** ({}): {}\n",
                    c.tool_name, c.created_at.format("%Y-%m-%d"), c.correction_text
                ));
            }
            text.push('\n');
            text
        } else {
            String::new()
        };

        let code_budget = self.context_budget - (self.context_budget / 4);
        let builder = InjectedContextBuilder::new(code_budget, true);
        let code_context = builder.build_system_context(&merged);

        let system_prompt = format!(
            "{}{}\n\n\
            ## Grounding Rules\n\
            1. CITE SOURCES: Before providing logic or code, start with:\n\
               SOURCE: [filename] | REASON: [why this is relevant]\n\
            2. NO HALLUCINATION: If a function or struct is not in the context above,\n\
               output [MISSING_CONTEXT: name of entity] instead of inventing it.\n\
            3. CORRECTIONS: Any text marked [CRITICAL: PREVIOUS HUMAN CORRECTION] overrides all other data.\n\
            4. ACCURACY: Use exact types from the context. If you see f32, do not write f64.\n\
            5. PARAMETER VALUES: Reference ranges from actual code shown above.\n\
            6. UNCERTAINTY: If you are unsure about a connection between code pieces, say so explicitly.\n\
            7. CREATIVE REASONING: You may propose architecture and approach freely,\n\
               but ground all specifics (function names, types, values) in the context.\n",
            correction_text, code_context
        );

        Ok(SteeringContext {
            system_prompt,
            fragments_used: merged.len(),
            corrections_applied: applicable_corrections.len(),
            corrections_text: correction_text,
            query_terms: search_queries,
            confidence,
            needs_human_approval,
        })
    }

    /// Submit human feedback as a correction
    pub fn submit_correction(&mut self, correction: Correction) -> Result<()> {
        tracing::info!("Correction: {} — '{}'", correction.tool_name, correction.correction_text);
        self.corrections.push(correction);
        self.save_corrections()?;
        Ok(())
    }

    /// Find corrections applicable to the current query
    fn find_corrections(&self, query: &str, tool_name: Option<&str>) -> Vec<&Correction> {
        let query_lower = query.to_lowercase();
        self.corrections.iter().filter(|c| {
            if let Some(tn) = tool_name {
                if c.tool_name != tn && c.tool_name != "*" { return false; }
            }
            let terms: Vec<&str> = c.original_query.split_whitespace().collect();
            let matches = terms.iter().filter(|t| query_lower.contains(&t.to_lowercase())).count();
            matches >= 2 || self.scope_matches(&c.scope, &query_lower)
        }).collect()
    }

    fn scope_matches(&self, scope: &Option<CorrectionScope>, query: &str) -> bool {
        if let Some(s) = scope {
            if s.weather_window.as_ref().map_or(false, |w| query.contains(&w.to_lowercase())) { return true; }
            if s.sensor.as_ref().map_or(false, |w| query.contains(&w.to_lowercase())) { return true; }
            if s.region.as_ref().map_or(false, |w| query.contains(&w.to_lowercase())) { return true; }
        }
        false
    }

    /// RRF merge — deduplicate fragments, sum scores for same file::function
    fn rrf_merge(&self, fragments: Vec<(ContextFragment, f32)>) -> Vec<ContextFragment> {
        let mut score_map: HashMap<String, (ContextFragment, f32)> = HashMap::new();
        for (fragment, rrf_score) in fragments {
            let key = format!("{}::{}", fragment.file_path, fragment.function_name);
            let entry = score_map.entry(key).or_insert((fragment.clone(), 0.0));
            entry.1 += rrf_score;
        }
        let mut merged: Vec<ContextFragment> = score_map.into_values()
            .map(|(mut frag, score)| { frag.search_score = score; frag })
            .collect();
        merged.sort_by(|a, b| b.search_score.partial_cmp(&a.search_score).unwrap_or(std::cmp::Ordering::Equal));
        merged
    }

    /// Multi-query derivation for RRF (raw + technical + usage + role)
    fn derive_search_queries(&self, query: &str, role_hint: Option<&str>) -> Vec<String> {
        let mut queries = vec![query.to_string()];

        let technical: Vec<&str> = query.split_whitespace()
            .filter(|w| w.contains('_') || w.chars().any(|c| c.is_uppercase()) || w.len() > 8
                || DOMAIN_TERMS.iter().any(|t| w.to_lowercase().contains(t)))
            .collect();
        if !technical.is_empty() {
            queries.push(technical.join(" "));
            queries.push(format!("usage example call {}", technical.join(" ")));
        }

        if let Some(role) = role_hint {
            match role {
                "sensor" | "threshold" | "detection" => queries.push("band ratio threshold detection anomaly".into()),
                "geometry" | "grid" | "coordinate" => queries.push("sub-pixel grid alignment drift UTM".into()),
                "orchestrator" | "pipeline" => queries.push("temporal stack weather window download".into()),
                "research" => queries.push("research finding hypothesis source URL".into()),
                _ => {}
            }
        }
        queries
    }

    fn load_corrections(path: &str) -> Vec<Correction> {
        std::fs::read_to_string(path).ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    }

    fn save_corrections(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.corrections)?;
        std::fs::write(&self.corrections_path, json)?;
        Ok(())
    }

    pub fn chunk_count(&self) -> usize { self.engine.chunk_count() }
    pub fn correction_count(&self) -> usize { self.corrections.len() }
    pub fn context_budget(&self) -> usize { self.context_budget }

    pub async fn reindex(&mut self, path: &Path) -> Result<usize> {
        self.engine.index_directory(path).await
    }
}

const DOMAIN_TERMS: &[&str] = &[
    "wgpu", "wgsl", "shader", "buffer", "pipeline", "compute",
    "tile", "band", "sentinel", "landsat", "sar", "thermal",
    "anomaly", "detection", "threshold", "glint", "hydrocarbon",
    "drift", "grid", "pixel", "coordinate", "utm", "latlon",
    "nautivecs", "vector", "embedding", "injection", "context",
    "orchestrator", "worker", "dispatch", "node", "cluster",
    "p100", "vulkan", "cuda", "gpu", "vram",
    "wreck", "shipwreck", "plume", "seiche", "bathymetry",
];
