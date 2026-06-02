use reqwest::Client;
use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};
use crate::models::{PlanSpec, SubTask};

// KoboldCPP (Local 8B Model) Client
#[derive(Clone)]
pub struct KoboldClient {
    client: Client,
    base_url: String,
}

impl KoboldClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: std::env::var("LLAMA_CPP_BASE_URL")
                .or_else(|_| std::env::var("LLAMA_SERVER_BASE_URL"))
                .or_else(|_| std::env::var("VLLM_BASE_URL"))
                .or_else(|_| std::env::var("KOBOLD_BASE_URL"))
                .unwrap_or_else(|_| "http://localhost:5555/v1".to_string()),
        }
    }

    pub async fn chat_completion(&self, messages: Vec<Message>, temperature: f32, max_tokens: u32) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        
        let request = ChatRequest {
            model: "qwen3-8b".to_string(),
            messages,
            temperature,
            max_tokens,
            stream: false,
        };

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to reach local OpenAI-compatible endpoint")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Local OpenAI-compatible endpoint returned {}: {}", status, body);
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse local OpenAI-compatible response")?;

        chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| anyhow::anyhow!("Local OpenAI-compatible endpoint returned empty choices"))
    }

    pub async fn health_check(&self) -> bool {
        let url = format!("{}/health", self.base_url.trim_end_matches('/'));
        match self.client.get(&url).send().await {
            Ok(r) => r.status().is_success(),
            Err(_) => false,
        }
    }
}

// Nautivecs (Codebase Search) Client
#[derive(Clone)]
pub struct NautivecsClient {
    client: Client,
    base_url: String,
}

impl NautivecsClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: std::env::var("NAUTIVECS_URL")
                .unwrap_or_else(|_| "http://100.72.182.77:5003".to_string()),
        }
    }

    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchResult>> {
        let url = format!("{}/search", self.base_url.trim_end_matches('/'));
        
        let response = self.client
            .get(&url)
            .query(&[("q", query), ("limit", &limit.to_string())])
            .send()
            .await
            .context("Failed to reach Nautivecs endpoint")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Nautivecs returned {}: {}", status, body);
        }

        let search_response: SearchResponse = response
            .json()
            .await
            .context("Failed to parse Nautivecs response")?;

        Ok(search_response.results)
    }
}

// Cesarops (Remote 35B Model) Client
#[derive(Clone)]
pub struct CesaropsClient {
    client: Client,
    base_url: String,
}

impl CesaropsClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300)) // Longer timeout for 35B
                .build()
                .unwrap(),
            base_url: std::env::var("CESAROPS_API_URL")
                .unwrap_or_else(|_| "http://100.72.182.77:5001".to_string()),
        }
    }

    pub async fn execute_task(&self, spec: &PlanSpec, context: &[String]) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        
        let system_prompt = r#"You are CESAROPS, a powerful code generation and analysis engine.
You receive structured tasks from a planning frontend.
Your job is to produce high-quality, correct, and complete implementations.

Input Format:
- Sub-tasks: List of specific coding tasks.
- Required Context: Code snippets or references to include.
- Output Format: Expected structure of the result.

Instructions:
1. Follow the sub-tasks precisely.
2. Use the provided context.
3. Adhere strictly to the output format.
4. If you encounter ambiguity, make reasonable assumptions and note them."#;

        let user_content = format!(
            "Sub-tasks:\n{}\n\nRequired Context:\n{}\n\nOutput Format:\n{}",
            serde_json::to_string_pretty(&spec.sub_tasks).unwrap_or_default(),
            context.join("\n---\n"),
            spec.output_format
        );

        let messages = vec![
            Message {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            Message {
                role: "user".to_string(),
                content: user_content,
            },
        ];

        let request = ChatRequest {
            model: "qwen3.6-35b".to_string(),
            messages,
            temperature: 0.2, // Low temperature for deterministic code
            max_tokens: 8192,
            stream: false,
        };

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to reach Cesarops endpoint")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Cesarops returned {}: {}", status, body);
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse Cesarops response")?;

        chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| anyhow::anyhow!("Cesarops returned empty choices"))
    }
}

// Common types for LLM clients
#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    max_tokens: u32,
    
    stream: bool,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

// Nautivecs specific types
#[derive(Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
}

#[derive(Deserialize)]
pub struct SearchResult {
    pub file_path: String,
    pub text: String,
}
