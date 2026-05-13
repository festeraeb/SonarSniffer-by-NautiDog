//! LLM Client — talks to any OpenAI-compatible endpoint
//!
//! Handles: chat completions, streaming, error recovery.
//! Works with: KoboldCPP, Cake, mistral.rs, vLLM, OpenAI, Groq, etc.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A message in the chat format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Chat completion request
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stop: Vec<String>,
}

/// Chat completion response
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

/// Client for OpenAI-compatible LLM endpoints
pub struct LlmClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
    temperature: f32,
    max_tokens: u32,
}

impl LlmClient {
    pub fn new(
        base_url: &str,
        model: &str,
        api_key: &str,
        temperature: f32,
        max_tokens: u32,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: api_key.to_string(),
            temperature,
            max_tokens,
        }
    }

    /// Send a steered query to the LLM.
    /// `system_context` is the nautivecs-injected grounding context.
    /// `user_query` is the actual question/task from the MCP client.
    pub async fn steered_completion(
        &self,
        system_context: &str,
        user_query: &str,
    ) -> Result<String> {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_context.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_query.to_string(),
            },
        ];

        self.chat_completion(messages).await
    }

    /// Raw chat completion with arbitrary messages
    pub async fn chat_completion(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url);

        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            stop: vec![
                "<|im_end|>".to_string(),
                "Question:".to_string(),
                "\nUser:".to_string(),
                "\n\n\n".to_string(),
            ],
        };

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to reach LLM endpoint")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("LLM returned {}: {}", status, body);
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse LLM response")?;

        chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| anyhow::anyhow!("LLM returned empty choices"))
    }

    /// Health check — verify the endpoint is reachable
    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/models", self.base_url);
        match self.http.get(&url).send().await {
            Ok(r) => Ok(r.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}
