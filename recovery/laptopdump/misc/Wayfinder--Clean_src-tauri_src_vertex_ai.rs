// GitHub Models API integration for task decomposition and embeddings suggestions
// Uses GitHub's free models API (gpt-4o-mini, claude-3.5-sonnet, etc.)

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubModelsConfig {
    pub github_token: String,
    pub model: String, // e.g., "gpt-4o-mini", "claude-3-5-sonnet"
}

#[derive(Debug, Serialize)]
struct GitHubModelsRequest {
    messages: Vec<Message>,
    model: String,
    temperature: Option<f32>,
    max_tokens: Option<i32>,
    top_p: Option<f32>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct GitHubModelsResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

pub struct GitHubModelsClient {
    github_token: String,
    model: String,
    client: Client,
}

impl GitHubModelsClient {
    pub fn new(config: GitHubModelsConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());
            
        println!("[GitHubModels] Client created for model: {}", config.model);
        
        GitHubModelsClient {
            github_token: config.github_token,
            model: config.model,
            client,
        }
    }

    pub async fn decompose_idea_into_tasks(
        &self,
        idea: &str,
        system_prompt: &str,
    ) -> Result<Vec<serde_json::Value>, String> {
        let endpoint = "https://models.inference.ai.azure.com/chat/completions";

        println!("[GitHubModels] Starting decompose_idea_into_tasks");
        println!("[GitHubModels] Endpoint: {}", endpoint);
        println!("[GitHubModels] Model: {}", self.model);
        println!("[GitHubModels] Token length: {}", self.github_token.len());

        let messages = vec![
            Message {
                role: "system".to_string(),
                content: format!(
                    "You are an expert task decomposer. {}",
                    system_prompt
                ),
            },
            Message {
                role: "user".to_string(),
                content: format!(
                    "Break down this idea into exactly 8 concrete, actionable tasks that can each be completed in 1-3 hours.\n\nIdea: {}\n\nReturn as JSON array with fields: number, title, file_to_edit, why, steps (array), code_stub, test_criteria (array), error_message, time_hours, dependencies (array)",
                    idea
                ),
            },
        ];

        let request = GitHubModelsRequest {
            messages,
            model: self.model.clone(),
            temperature: Some(0.3),
            max_tokens: Some(2000),
            top_p: Some(0.95),
        };

        println!("[GitHubModels] Making request to API...");
        match self.client
            .post(endpoint)
            .bearer_auth(&self.github_token)
            .json(&request)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                println!("[GitHubModels] Response status: {}", status);
                
                if !status.is_success() {
                    let error_text = response.text().await.unwrap_or_default();
                    println!("[GitHubModels] Error response: {}", error_text);
                    return Err(format!("API returned status {}: {}", status, error_text));
                }
                
                match response.json::<GitHubModelsResponse>().await {
                    Ok(data) => {
                        println!("[GitHubModels] Successfully parsed response");
                        if let Some(choice) = data.choices.first() {
                            // Try to extract JSON array from response
                            let response_text = &choice.message.content;
                            println!("[GitHubModels] Response text length: {}", response_text.len());
                            
                            if let Ok(tasks) = serde_json::from_str::<Vec<serde_json::Value>>(response_text) {
                                println!("[GitHubModels] Successfully parsed tasks JSON");
                                return Ok(tasks);
                            }
                            // If not valid JSON, try to extract from markdown code blocks
                            println!("[GitHubModels] Trying to extract JSON from markdown blocks...");
                            if let Some(start) = response_text.find('[') {
                                if let Some(end) = response_text.rfind(']') {
                                    if let Ok(tasks) = serde_json::from_str::<Vec<serde_json::Value>>(&response_text[start..=end]) {
                                        println!("[GitHubModels] Successfully extracted tasks from markdown");
                                        return Ok(tasks);
                                    }
                                }
                            }
                        }
                        println!("[GitHubModels] Error: No valid response from GitHub Models API");
                        return Err("No valid response from GitHub Models API".to_string());
                    }
                    Err(e) => {
                        println!("[GitHubModels] Error parsing response JSON: {}", e);
                        return Err(format!("Failed to parse response: {}", e));
                    }
                }
            }
            Err(e) => {
                println!("[GitHubModels] API call failed: {}", e);
                return Err(format!("GitHub Models API call failed: {}", e));
            }
        }
    }

    pub async fn get_embedding_suggestion(
        &self,
        context: &str,
        question: &str,
    ) -> Result<String, String> {
        let endpoint = "https://models.inference.ai.azure.com/chat/completions";

        let messages = vec![
            Message {
                role: "system".to_string(),
                content: "You are a code understanding expert. Provide concise, actionable suggestions.".to_string(),
            },
            Message {
                role: "user".to_string(),
                content: format!(
                    "Based on this context:\n{}\n\nAnswer this question concisely:\n{}",
                    context, question
                ),
            },
        ];

        let request = GitHubModelsRequest {
            messages,
            model: self.model.clone(),
            temperature: Some(0.3),
            max_tokens: Some(500),
            top_p: Some(0.9),
        };

        match self.client
            .post(endpoint)
            .bearer_auth(&self.github_token)
            .json(&request)
            .send()
            .await
        {
            Ok(response) => {
                match response.json::<GitHubModelsResponse>().await {
                    Ok(data) => {
                        if let Some(choice) = data.choices.first() {
                            Ok(choice.message.content.clone())
                        } else {
                            Err("No response from GitHub Models API".to_string())
                        }
                    }
                    Err(e) => Err(format!("Failed to parse response: {}", e)),
                }
            }
            Err(e) => Err(format!("GitHub Models API call failed: {}", e)),
        }
    }
}

// Get global config directory
fn get_global_config_dir() -> Result<std::path::PathBuf, String> {
    if let Some(home) = dirs::home_dir() {
        Ok(home.join(".wayfinder"))
    } else {
        Err("Could not determine home directory".to_string())
    }
}

// Load GitHub Models config from global file
pub fn load_github_models_config(_index_dir: &Path) -> Result<GitHubModelsConfig, String> {
    let config_dir = get_global_config_dir()?;
    let config_file = config_dir.join("github_models_config.json");

    if !config_file.exists() {
        return Err(format!("GitHub Models config not found at: {}", config_file.display()));
    }

    let content = fs::read_to_string(&config_file)
        .map_err(|e| format!("Failed to read config: {}", e))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse config: {}", e))
}

// Save GitHub Models config to global file
pub fn save_github_models_config(
    _index_dir: &Path,
    github_token: String,
    model: String,
) -> Result<(), String> {
    let config = GitHubModelsConfig {
        github_token,
        model,
    };

    let config_dir = get_global_config_dir()?;
    fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Failed to create config directory: {}", e))?;

    let config_file = config_dir.join("github_models_config.json");
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    fs::write(&config_file, json)
        .map_err(|e| format!("Failed to write config: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_models_config() {
        let config = GitHubModelsConfig {
            github_token: "test-token".to_string(),
            model: "gpt-4o-mini".to_string(),
        };

        assert_eq!(config.github_token, "test-token");
        assert_eq!(config.model, "gpt-4o-mini");
    }
}
