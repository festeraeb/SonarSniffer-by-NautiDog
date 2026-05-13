use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, warn};

#[derive(Debug, Serialize)]
struct GenerateRequest {
    prompt: String,
    max_length: usize,
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct KoboldResult {
    text: String,
}

#[derive(Debug, Deserialize)]
struct GenerateResponse {
    results: Vec<KoboldResult>,
}

pub struct LlmClient {
    client: Client,
    coder_url: String,
    reviewer_url: String,
}

impl LlmClient {
    pub fn new(coder_base: &str, reviewer_base: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .expect("Failed to build reqwest client");

        Self {
            client,
            coder_url: format!("{}/api/v1/generate", coder_base),
            reviewer_url: format!("{}/api/v1/generate", reviewer_base),
        }
    }

    async fn generate(&self, url: &str, prompt: String, max_length: usize, temperature: f32) -> Result<String> {
        let request = GenerateRequest { prompt, max_length, temperature };

        let response = self.client
            .post(url)
            .json(&request)
            .send()
            .await
            .context("Failed to send LLM request")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("LLM API error: {}", response.status()));
        }

        let body = response.text().await.context("Failed to read LLM response")?;
        let parsed: GenerateResponse = serde_json::from_str(&body)
            .context("Failed to parse KoboldCPP response")?;

        let text = parsed.results.first()
            .map(|r| r.text.clone())
            .unwrap_or_default();

        if text.is_empty() {
            warn!("LLM returned empty response — likely context overflow");
            return Err(anyhow::anyhow!("Empty response from LLM"));
        }

        info!("LLM generated {} chars", text.len());
        Ok(text)
    }

    pub async fn generate_coder(&self, prompt: String) -> Result<String> {
        self.generate(&self.coder_url, prompt, 8192, 0.2).await
    }

    pub async fn generate_reviewer(&self, prompt: String) -> Result<String> {
        self.generate(&self.reviewer_url, prompt, 4096, 0.3).await
    }
}
