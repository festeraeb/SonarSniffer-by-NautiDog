

=== FILE: Cargo.toml ===
```toml
[package]
name = "cesarops-thought-engine"
version = "0.1.0"
edition = "2021"
description = "CESAROPS Thought Engine: Distributed reasoning system for code generation"

[dependencies]
axum = { version = "0.8", features = ["json"] }
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tower-http = { version = "0.6", features = ["cors"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
uuid = { version = "1", features = ["v4"] }
anyhow = "1"
```

=== FILE: src/main.rs ===
```rust
use std::sync::Arc;

use axum::{Router};
use tower_http::cors::CorsLayer;

mod clients;
mod engine;
mod handlers;
mod models;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into())
        )
        .init();

    // Initialize clients
    let kobold_client = clients::KoboldClient::new();
    let nautivecs_client = clients::NautivecsClient::new();
    let cesarops_client = clients::CesaropsClient::new();

    // Initialize engine
    let thought_engine = Arc::new(engine::ThoughtEngine::new(
        kobold_client,
        nautivecs_client,
        cesarops_client,
    ));

    // Build router
    let app = Router::new()
        .route("/tasks", axum::routing::post(handlers::create_task))
        .route("/tasks/{task_id}", axum::routing::get(handlers::get_task))
        .route("/health", axum::routing::get(handlers::health))
        .with_state(thought_engine)
        .layer(CorsLayer::permissive());

    // Start server
    let addr = "0.0.0.0:5556";
    tracing::info!("Starting Thought Engine on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

=== FILE: src/clients.rs ===
```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};

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
            base_url: std::env::var("KOBOLD_BASE_URL")
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
            .context("Failed to reach KoboldCPP endpoint")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("KoboldCPP returned {}: {}", status, body);
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse KoboldCPP response")?;

        chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| anyhow::anyhow!("KoboldCPP returned empty choices"))
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
    #[serde(skip_serializing_if = "std::ops::Not::not")]
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
struct Message {
    role: String,
    content: String,
}

// Nautivecs specific types
#[derive(Deserialize)]
struct SearchResponse {
    results: Vec<SearchResult>,
}

#[derive(Deserialize)]
struct SearchResult {
    file: String,
    snippet: String,
}

// PlanSpec type (also defined in models.rs, but needed here for CesaropsClient)
#[derive(Serialize, Clone)]
pub struct PlanSpec {
    pub sub_tasks: Vec<SubTask>,
    pub required_context: Vec<String>,
    pub output_format: String,
}

#[derive(Serialize, Clone)]
pub struct SubTask {
    pub description: String,
}
```

=== FILE: src/engine.rs ===
```rust
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::clients::{KoboldClient, NautivecsClient, CesaropsClient};
use crate::models::{TaskRequest, TaskResponse, PlanSpec, SubTask};

pub struct ThoughtEngine {
    kobold: KoboldClient,
    nautivecs: NautivecsClient,
    cesarops: CesaropsClient,
    tasks: Arc<std::collections::HashMap<String, TaskResponse>>,
}

impl ThoughtEngine {
    pub fn new(
        kobold: KoboldClient,
        nautivecs: NautivecsClient,
        cesarops: CesaropsClient,
    ) -> Self {
        Self {
            kobold,
            nautivecs,
            cesarops,
            tasks: Arc::new(std::collections::HashMap::new()),
        }
    }

    pub async fn process_task(&self, request: TaskRequest) -> Result<TaskResponse> {
        let task_id = request.task_id.unwrap_or_else(|| {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            format!("task_{}", timestamp)
        });

        let mut response = TaskResponse {
            task_id: task_id.clone(),
            status: "processing".to_string(),
            result: None,
            error: None,
            steps: Vec::new(),
        };

        // Store initial task state
        {
            let mut tasks = self.tasks.lock().unwrap();
            tasks.insert(task_id.clone(), response.clone());
        }

        // Step 1: THINK - Decompose Query
        tracing::info!("[{}] Step 1: Thinking/Decomposing...", task_id);
        let thinking_result = self.think(&request.query).await;
        
        match thinking_result {
            Ok(plan_json) => {
                response.steps.push(crate::models::Step {
                    step: "think".to_string(),
                    output: Some(plan_json.chars().take(200).collect::<String>() + "..."),
                });

                // Parse JSON from thinking result
                let sub_tasks = self.parse_thinking_result(&plan_json, &request.query);
                
                // Step 2: SEARCH - Gather Context
                tracing::info!("[{}] Step 2: Searching...", task_id);
                let context_list = self.search(&sub_tasks).await;
                response.steps.push(crate::models::Step {
                    step: "search".to_string(),
                    output: Some(format!("Found {} context items", context_list.len())),
                });

                // Step 3: PLAN - Create Spec for 35B
                tracing::info!("[{}] Step 3: Planning Spec...", task_id);
                let spec = self.plan(&request.query, &sub_tasks, &context_list).await;
                response.steps.push(crate::models::Step {
                    step: "plan".to_string(),
                    output: Some(format!("Spec created with {} sub-tasks", spec.sub_tasks.len())),
                });

                // Step 4: DISPATCH - Send to 35B
                tracing::info!("[{}] Step 4: Dispatching to 35B...", task_id);
                let execution_result = self.dispatch(&spec, &context_list).await;
                response.steps.push(crate::models::Step {
                    step: "dispatch".to_string(),
                    output: Some("Execution completed".to_string()),
                });

                // Step 5: VERIFY - Check Output
                tracing::info!("[{}] Step 5: Verifying...", task_id);
                let verification_result = self.verify(&request.query, &execution_result).await;
                
                if self.is_verified(&verification_result) {
                    response.status = "completed".to_string();
                    response.result = Some(execution_result);
                } else {
                    response.status = "completed_with_warnings".to_string();
                    response.result = Some(execution_result);
                    response.error = Some(format!("Verification warning: {}", verification_result.chars().take(100).collect::<String>()));
                }
            }
            Err(e) => {
                tracing::error!("[{}] Thinking failed: {}", task_id, e);
                response.status = "failed".to_string();
                response.error = Some(format!("Thinking failed: {}", e));
            }
        }

        // Update final state
        {
            let mut tasks = self.tasks.lock().unwrap();
            tasks.insert(task_id.clone(), response.clone());
        }

        Ok(response)
    }

    async fn think(&self, query: &str) -> Result<String> {
        let thinking_prompt = vec![
            crate::clients::Message {
                role: "system".to_string(),
                content: "You are a Research Lead. Break down this complex query into specific sub-tasks. Return JSON.".to_string(),
            },
            crate::clients::Message {
                role: "user".to_string(),
                content: format!(
                    "Query: {}\n\nBreak this into 3-5 sub-tasks. Specify for each: task description, source (code/web), and rationale. Return as JSON array.",
                    query
                ),
            },
        ];

        self.kobold
            .chat_completion(thinking_prompt, 0.7, 4096)
            .await
            .context("Failed to get thinking result from KoboldCPP")
    }

    fn parse_thinking_result(&self, thinking_result: &str, original_query: &str) -> Vec<SubTask> {
        // Try to parse as JSON array
        if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(thinking_result) {
            return parsed
                .iter()
                .map(|item| SubTask {
                    description: item["task"].as_str().unwrap_or(
                        item["query"].as_str().unwrap_or(original_query)
                    ).to_string(),
                })
                .filter(|st| !st.description.is_empty())
                .collect();
        }

        // Fallback if LLM didn't return pure JSON
        vec![SubTask {
            description: original_query.to_string(),
        }]
    }

    async fn search(&self, sub_tasks: &[SubTask]) -> Vec<String> {
        let mut context_list = Vec::new();

        for sub_task in sub_tasks {
            // Search Nautivecs
            match self.nautivecs.search(&sub_task.description, 5).await {
                Ok(results) => {
                    for result in results {
                        context_list.push(format!(
                            "Code Reference: {} -> {}",
                            result.file, result.snippet
                        ));
                    }
                }
                Err(e) => {
                    tracing::warn!("Nautivecs search failed for '{}': {}", sub_task.description, e);
                }
            }

            // Web search would go here (placeholder)
            // For now, we skip web search as it's not implemented
        }

        context_list
    }

    async fn plan(&self, query: &str, sub_tasks: &[SubTask], context: &[String]) -> PlanSpec {
        let planning_prompt = vec![
            crate::clients::Message {
                role: "system".to_string(),
                content: "You are a Technical Planner. Convert research findings into a structured execution spec for a code generation engine.".to_string(),
            },
            crate::clients::Message {
                role: "user".to_string(),
                content: format!(
                    "Original Query: {}\n\nSub-tasks from thinking: {}\n\nGathered Context: {}\n\nCreate a structured spec with: sub_tasks (list of dicts with 'description'), required_context (list of strings), and output_format (string). Return JSON.",
                    query,
                    serde_json::to_string_pretty(sub_tasks).unwrap_or_default(),
                    serde_json::to_string_pretty(&context.iter().take(5).cloned().collect::<Vec<_>>()).unwrap_or_default()
                ),
            },
        ];

        match self.kobold.chat_completion(planning_prompt, 0.3, 4096).await {
            Ok(plan_result) => {
                // Try to parse as JSON
                if let Ok(spec_json) = serde_json::from_str::<serde_json::Value>(&plan_result) {
                    let sub_tasks: Vec<SubTask> = spec_json
                        .get("sub_tasks")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|item| {
                                    item.get("description")
                                        .and_then(|d| d.as_str())
                                        .map(|s| SubTask { description: s.to_string() })
                                })
                                .collect()
                        })
                        .unwrap_or_else(|| vec![SubTask { description: query.to_string() }]);

                    let required_context: Vec<String> = spec_json
                        .get("required_context")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_else(|| context.to_vec());

                    let output_format = spec_json
                        .get("output_format")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Code blocks with explanations")
                        .to_string();

                    PlanSpec {
                        sub_tasks,
                        required_context,
                        output_format,
                    }
                } else {
                    // Fallback spec
                    PlanSpec {
                        sub_tasks: vec![SubTask { description: query.to_string() }],
                        required_context: context.to_vec(),
                        output_format: "Code".to_string(),
                    }
                }
            }
            Err(_) => {
                // Fallback spec on error
                PlanSpec {
                    sub_tasks: vec![SubTask { description: query.to_string() }],
                    required_context: context.to_vec(),
                    output_format: "Code".to_string(),
                }
            }
        }
    }

    async fn dispatch(&self, spec: &PlanSpec, context: &[String]) -> Result<String> {
        self.cesarops
            .execute_task(spec, context)
            .await
            .context("Failed to execute task with Cesarops")
    }

    async fn verify(&self, query: &str, execution_result: &str) -> Result<String> {
        let verification_prompt = vec![
            crate::clients::Message {
                role: "system".to_string(),
                content: "You are a Code Reviewer. Check the following code for obvious errors, syntax issues, or logical flaws. If it looks good, say 'VERIFIED'. If not, explain why.".to_string(),
            },
            crate::clients::Message {
                role: "user".to_string(),
                content: format!(
                    "Original Task: {}\n\nGenerated Code:\n{}",
                    query, execution_result
                ),
            },
        ];

        self.kobold
            .chat_completion(verification_prompt, 0.1, 4096)
            .await
            .context("Failed to get verification result from KoboldCPP")
    }

    fn is_verified(&self, verification_result: &str) -> bool {
        let lower = verification_result.to_lowercase();
        lower.contains("verified") || lower.contains("correct")
    }
}
```

=== FILE: src/models.rs ===
```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct TaskRequest {
    pub task_id: Option<String>,
    pub query: String,
    pub context_hint: Option<String>, // Optional hint for nautivecs search
}

#[derive(Serialize, Clone)]
pub struct TaskResponse {
    pub task_id: String,
    pub status: String, // 'processing', 'completed', 'failed', 'completed_with_warnings'
    pub result: Option<String>,
    pub error: Option<String>,
    pub steps: Vec<Step>,
}

#[derive(Serialize, Clone)]
pub struct Step {
    pub step: String,
    pub output: Option<String>,
}

// PlanSpec type (also used in clients.rs)
#[derive(Serialize, Deserialize, Clone)]
pub struct PlanSpec {
    pub sub_tasks: Vec<SubTask>,
    pub required_context: Vec<String>,
    pub output_format: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SubTask {
    pub description: String,
}
```

=== FILE: src/handlers.rs ===
```rust
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;

use crate::engine::ThoughtEngine;
use crate::models::{TaskRequest, TaskResponse};

pub async fn create_task(
    State(engine): State<Arc<ThoughtEngine>>,
    Json(request): Json<TaskRequest>,
) -> Result<Json<TaskResponse>, (StatusCode, Json<String>)> {
    match engine.process_task(request).await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            tracing::error!("Task processing failed: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(format!("Task processing failed: {}", e)),
            ))
        }
    }
}

pub async fn get_task(
    State(engine): State<Arc<ThoughtEngine>>,
    Path(task_id): Path<String>,
) -> Result<Json<TaskResponse>, (StatusCode, Json<String>)> {
    let tasks = engine.tasks.lock().unwrap();
    
    match tasks.get(&task_id) {
        Some(response) => Ok(Json(response.clone())),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(format!("Task {} not found", task_id)),
        )),
    }
}

pub async fn health(
    State(engine): State<Arc<ThoughtEngine>>,
) -> Json<serde_json::Value> {
    let kobold_healthy = engine.kobold.health_check().await;
    
    serde_json::json!({
        "status": "ok",
        "kobold": kobold_healthy
    })
}
```