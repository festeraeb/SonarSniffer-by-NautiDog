use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct LlmClient {
    client: Client,
    base_url: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

impl LlmClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: "http://localhost:5001/v1/chat/completions".to_string(),
        }
    }

    pub async fn decide(&self, system_prompt: &str, user_prompt: &str) -> Result<String, reqwest::Error> {
        let request = ChatRequest {
            model: "local-model".to_string(),
            messages: vec![
                ChatMessage { role: "system".to_string(), content: system_prompt.to_string() },
                ChatMessage { role: "user".to_string(), content: user_prompt.to_string() },
            ],
        };
        
        let response = self.client.post(&self.base_url)
            .json(&request)
            .send()
            .await?;
            
        let chat_resp: ChatResponse = response.json().await?;
        Ok(chat_resp.choices.first()
            .map(|c| c.message.content.clone())
            .unwrap_or_else(|| "No response".to_string()))
    }
}
