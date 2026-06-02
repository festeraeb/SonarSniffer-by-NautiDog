//! Inference routing for llama-server (completions vs chat, reasoning merge).
//! Persisted in `cluster_config.toml` under `[inference]`.

use serde::{Deserialize, Serialize};

pub const CFG_PATH: &str = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/cluster_config.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceConfig {
    /// Use `/v1/chat/completions` for listed endpoints (Gemma/R1 thinking models).
    pub use_chat_completions: bool,
    /// When `message.content` is empty, use `reasoning_content` (llama-server thinking).
    pub merge_reasoning_content: bool,
    /// Base URLs (no trailing path) that should use chat API + reasoning merge.
    pub chat_completion_endpoints: Vec<String>,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            use_chat_completions: true,
            merge_reasoning_content: true,
            chat_completion_endpoints: vec![
                "http://127.0.0.1:5001".to_string(),
                "http://127.0.0.1:5002".to_string(),
            ],
        }
    }
}

impl InferenceConfig {
    fn from_toml(table: &toml::Table) -> Self {
        let d = Self::default();
        Self {
            use_chat_completions: table
                .get("use_chat_completions")
                .and_then(|v| v.as_bool())
                .unwrap_or(d.use_chat_completions),
            merge_reasoning_content: table
                .get("merge_reasoning_content")
                .and_then(|v| v.as_bool())
                .unwrap_or(d.merge_reasoning_content),
            chat_completion_endpoints: table
                .get("chat_completion_endpoints")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or(d.chat_completion_endpoints),
        }
    }

    pub fn endpoint_uses_chat(&self, base_url: &str) -> bool {
        if !self.use_chat_completions {
            return false;
        }
        let base = normalize_endpoint(base_url);
        self.chat_completion_endpoints
            .iter()
            .any(|e| normalize_endpoint(e) == base)
    }
}

pub fn normalize_endpoint(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

pub fn load() -> InferenceConfig {
    let content = std::fs::read_to_string(CFG_PATH).unwrap_or_default();
    let table: toml::Table = content.parse().unwrap_or_default();
    table
        .get("inference")
        .and_then(|v| v.as_table())
        .map(InferenceConfig::from_toml)
        .unwrap_or_default()
}
