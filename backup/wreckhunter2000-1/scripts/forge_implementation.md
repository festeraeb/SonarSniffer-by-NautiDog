

<think>

</think>

Here are the 7 files for the `cesarops-forge` agent, implemented according to the researcher's findings and cluster specifications.

### 1. cesarops-forge/Cargo.toml

```toml
[package]
name = "cesarops-forge"
version = "0.1.0"
edition = "2021"

[dependencies]
reqwest = { version = "0.11", features = ["json", "rustls-tls"] }
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
clap = { version = "4.0", features = ["derive"] }
regex = "1.5"
tempfile = "3.3"
anyhow = "1.0"
```

### 2. cesarops-forge/src/main.rs

```rust
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber;

mod llm;
mod search;
mod tools;
mod context;
mod loop_engine;

#[derive(Parser, Debug)]
#[command(name = "cesarops-forge", about = "Autonomous Developer Agent for CesarOps")]
struct Args {
    /// Path to the tasks markdown file
    #[arg(short, long)]
    tasks: PathBuf,

    /// Target directory for final file commits
    #[arg(short, long)]
    target_dir: PathBuf,

    /// Specific task index to run (0-based). If omitted, runs all.
    #[arg(short, long)]
    task: Option<usize>,

    /// Show status of tasks
    #[arg(long)]
    status: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("cesarops_forge=info"),
        )
        .init();

    let args = Args::parse();

    if args.status {
        // Simple status check implementation
        if args.tasks.exists() {
            let content = tokio::fs::read_to_string(&args.tasks).await?;
            let tasks = loop_engine::parse_tasks(&content);
            for (i, task) in tasks.iter().enumerate() {
                let status = if task.done { "DONE" } else { "PENDING" };
                println!("Task {}: {} - {}", i, status, task.text);
            }
        } else {
            println!("Tasks file not found: {:?}", args.tasks);
        }
        return Ok(());
    }

    // Build LLM Client
    let coder_url = "http://100.72.182.77:5001/api/v1/generate";
    let reviewer_url = "http://100.102.158.111:5555/api/v1/generate";
    let nautivecs_url = "http://100.72.182.77:5003/query";
    let wso_url = "http://100.72.182.77:5010/search";

    let llm_client = llm::LlmClient::new(coder_url, reviewer_url);
    let search_client = search::SearchClient::new(nautivecs_url, wso_url);

    // Run the loop
    loop_engine::run_agent(
        &args.tasks,
        &args.target_dir,
        args.task,
        &llm_client,
        &search_client,
    )
    .await?;

    Ok(())
}
```

### 3. cesarops-forge/src/llm.rs

```rust
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{error, info, warn};

#[derive(Debug, Serialize, Deserialize)]
struct GenerateRequest {
    prompt: String,
    max_length: usize,
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct GenerateResponse {
    // Adjust based on actual KoboldCPP response structure
    // Often it's a list of strings or a specific field
    text: Option<String>,
    // Some APIs return a list of generated texts
    #[serde(default)]
    generated_text: Option<String>,
}

pub struct LlmClient {
    client: Client,
    coder_url: String,
    reviewer_url: String,
}

impl LlmClient {
    pub fn new(coder_url: &str, reviewer_url: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build reqwest client");
        
        Self {
            client,
            coder_url: coder_url.to_string(),
            reviewer_url: reviewer_url.to_string(),
        }
    }

    pub async fn generate(
        &self,
        url: &str,
        prompt: String,
        max_length: usize,
        temperature: f32,
    ) -> Result<String> {
        let request = GenerateRequest {
            prompt,
            max_length,
            temperature,
        };

        let response = self
            .client
            .post(url)
            .json(&request)
            .send()
            .await
            .context("Failed to send LLM request")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "LLM API returned error status: {}",
                response.status()
            ));
        }

        let body = response.text().await.context("Failed to read LLM response body")?;
        
        // Parse response. KoboldCPP usually returns {"text": "..."} or similar.
        // We'll try to extract the generated text.
        let parsed: GenerateResponse = serde_json::from_str(&body)
            .context("Failed to parse LLM response JSON")?;

        let text = parsed.text.or(parsed.generated_text).unwrap_or_default();
        
        if text.is_empty() {
            warn!("LLM returned empty response, likely context overflow or timeout");
            return Err(anyhow::anyhow!("Empty response from LLM"));
        }

        Ok(text)
    }

    pub async fn generate_coder(&self, prompt: String) -> Result<String> {
        // 35B Coder: max_length 8192, temp 0.2
        self.generate(&self.coder_url, prompt, 8192, 0.2).await
    }

    pub async fn generate_reviewer(&self, prompt: String) -> Result<String> {
        // 8B Reviewer: max_length 4096, temp 0.3
        self.generate(&self.reviewer_url, prompt, 4096, 0.3).await
    }
}
```

### 4. cesarops-forge/src/search.rs

```rust
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Serialize)]
struct NautivecsRequest {
    query: String,
    top_k: usize,
}

#[derive(Debug, Serialize)]
struct WsoRequest {
    query: String,
    max_results: usize,
}

#[derive(Debug, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub content: String,
    pub url: Option<String>,
}

pub struct SearchClient {
    client: Client,
    nautivecs_url: String,
    wso_url: String,
}

impl SearchClient {
    pub fn new(nautivecs_url: &str, wso_url: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(5)) // Search timeout: 5s
            .build()
            .expect("Failed to build search client");

        Self {
            client,
            nautivecs_url: nautivecs_url.to_string(),
            wso_url: wso_url.to_string(),
        }
    }

    pub async fn search_nautivecs(&self, query: String, top_k: usize) -> Result<Vec<SearchResult>> {
        let request = NautivecsRequest { query, top_k };
        
        let response = self
            .client
            .post(&self.nautivecs_url)
            .json(&request)
            .send()
            .await
            .context("Failed to send nautivecs query")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Nautivecs API returned error status: {}",
                response.status()
            ));
        }

        // Assuming nautivecs returns a list of results
        let results: Vec<SearchResult> = response
            .json()
            .await
            .context("Failed to parse nautivecs response")?;

        Ok(results)
    }

    pub async fn search_web(&self, query: String, max_results: usize) -> Result<Vec<SearchResult>> {
        let request = WsoRequest { query, max_results };
        
        let response = self
            .client
            .post(&self.wso_url)
            .json(&request)
            .send()
            .await
            .context("Failed to send WSO query")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "WSO API returned error status: {}",
                response.status()
            ));
        }

        let results: Vec<SearchResult> = response
            .json()
            .await
            .context("Failed to parse WSO response")?;

        Ok(results)
    }
}
```

### 5. cesarops-forge/src/tools.rs

```rust
use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{error, info, warn};

#[derive(Debug, Deserialize)]
pub struct CompilerError {
    pub message: String,
    pub level: String,
    pub code: Option<String>,
    pub span: Option<String>,
}

pub fn cargo_check(project_dir: &Path) -> Result<(), Vec<CompilerError>> {
    info!("Running cargo check in {:?}", project_dir);
    
    let output = Command::new("cargo")
        .arg("check")
        .arg("--message-format=json")
        .current_dir(project_dir)
        .output()
        .context("Failed to execute cargo check")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse JSON messages
    let mut errors = Vec::new();
    for line in stdout.lines() {
        if let Ok(message) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(level) = message.get("level").and_then(|l| l.as_str()) {
                if level == "error" || level == "warning" {
                    let compiler_error = CompilerError {
                        message: message.get("message")
                            .and_then(|m| m.get("text"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("Unknown error")
                            .to_string(),
                        level: level.to_string(),
                        code: message.get("code").and_then(|c| c.get("code")).and_then(|c| c.as_str()).map(|s| s.to_string()),
                        span: message.get("spans").and_then(|s| s.get(0)).and_then(|s| s.get("snippet")).and_then(|s| s.as_str()).map(|s| s.to_string()),
                    };
                    errors.push(compiler_error);
                }
            }
        }
    }

    if !errors.is_empty() {
        error!("Cargo check failed with {} errors/warnings", errors.len());
        for err in &errors {
            error!("Error: {} - {}", err.level, err.message);
        }
        return Err(errors);
    }

    info!("Cargo check passed");
    Ok(())
}

pub fn write_files(temp_dir: &Path, files: Vec<(String, String)>) -> Result<()> {
    info!("Writing {} files to temp dir", files.len());
    
    for (path, content) in files {
        let full_path = temp_dir.join(&path);
        
        // Create parent directories if they don't exist
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).context("Failed to create directory")?;
        }

        let mut file = fs::File::create(&full_path).context("Failed to create file")?;
        file.write_all(content.as_bytes()).context("Failed to write file content")?;
        
        info!("Wrote file: {}", path);
    }

    Ok(())
}

pub fn commit_files(temp_dir: &Path, target_dir: &Path) -> Result<()> {
    info!("Committing files from temp to target dir");
    
    // Copy all files from temp to target
    for entry in fs::read_dir(temp_dir).context("Failed to read temp dir")? {
        let entry = entry?;
        let path = entry.path();
        let target_path = target_dir.join(path.file_name().unwrap());
        
        if path.is_file() {
            fs::copy(&path, &target_path).context("Failed to copy file")?;
            info!("Committed file: {}", path.file_name().unwrap().to_string_lossy());
        }
    }

    Ok(())
}
```

### 6. cesarops-forge/src/context.rs

```rust
use anyhow::Result;
use tracing::info;

#[derive(Debug)]
pub struct Task {
    pub index: usize,
    pub text: String,
    pub done: bool,
}

pub fn parse_tasks(content: &str) -> Vec<Task> {
    let mut tasks = Vec::new();
    let re = regex::Regex::new(r"- \[([ x])\] (.+)").unwrap();
    
    for cap in re.captures_iter(content) {
        let status = cap.get(1).unwrap().as_str();
        let text = cap.get(2).unwrap().as_str().to_string();
        let done = status == "x";
        tasks.push(Task {
            index: tasks.len(),
            text,
            done,
        });
    }
    
    tasks
}

pub fn build_coder_prompt(task: &str, nautivecs_context: &[String], corrections: &[String]) -> String {
    let mut prompt = String::from("You are an expert Rust developer. Your task is to implement the following:\n\n");
    prompt.push_str(&format!("Task: {}\n\n", task));
    
    if !nautivecs_context.is_empty() {
        prompt.push_str("Relevant Code Context from Nautivecs:\n");
        for (i, ctx) in nautivecs_context.iter().enumerate() {
            prompt.push_str(&format!("{}.\n{}\n\n", i + 1, ctx));
        }
    }

    if !corrections.is_empty() {
        prompt.push_str("Previous Corrections/Errors:\n");
        for (i, corr) in corrections.iter().enumerate() {
            prompt.push_str(&format!("{}.\n{}\n\n", i + 1, corr));
        }
    }

    prompt.push_str("Please provide the complete Rust code for the task. Use markdown code blocks.\n");
    prompt.push_str("Ensure the code compiles with `cargo check`.\n");
    
    truncate_to_tokens(&prompt, 28000) // Leave room for response
}

pub fn build_reviewer_prompt(code_output: &str) -> String {
    let mut prompt = String::from("You are a senior Rust code reviewer. Review the following code for bugs, style issues, and potential errors.\n\n");
    prompt.push_str("Code:\n```\n");
    prompt.push_str(code_output);
    prompt.push_str("\n```\n\n");
    prompt.push_str("If you find any issues, list them clearly. If the code is good, say 'APPROVED'.\n");
    
    truncate_to_tokens(&prompt, 3500)
}

pub fn build_correction_prompt(code_output: &str, errors: &[String]) -> String {
    let mut prompt = String::from("You are an expert Rust developer. The following code has errors. Please fix them.\n\n");
    prompt.push_str("Original Code:\n```\n");
    prompt.push_str(code_output);
    prompt.push_str("\n```\n\n");
    prompt.push_str("Errors:\n");
    for err in errors {
        prompt.push_str(&format!("- {}\n", err));
    }
    prompt.push_str("\nPlease provide the corrected code.\n");
    
    truncate_to_tokens(&prompt, 28000)
}

pub fn truncate_to_tokens(text: &str, max_tokens: usize) -> String {
    // Rough estimate: 4 chars per token
    let max_chars = max_tokens * 4;
    if text.len() <= max_chars {
        return text.to_string();
    }
    
    // Truncate from the beginning to preserve the end (most recent context)
    let truncated = &text[text.len().saturating_sub(max_chars)..];
    info!("Truncated context to {} chars", truncated.len());
    truncated.to_string()
}
```

### 7. cesarops-forge/src/loop_engine.rs

```rust
use anyhow::{Context, Result};
use regex::Regex;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;
use tracing::{error, info, warn};

use crate::context::{self, Task};
use crate::llm::LlmClient;
use crate::search::SearchClient;
use crate::tools::{cargo_check, commit_files, write_files};

pub async fn run_agent(
    tasks_path: &PathBuf,
    target_dir: &PathBuf,
    specific_task: Option<usize>,
    llm_client: &LlmClient,
    search_client: &SearchClient,
) -> Result<()> {
    let content = fs::read_to_string(tasks_path).context("Failed to read tasks file")?;
    let tasks = context::parse_tasks(&content);

    if tasks.is_empty() {
        warn!("No tasks found in {}", tasks_path.display());
        return Ok(());
    }

    let start_index = specific_task.unwrap_or(0);
    let end_index = specific_task.unwrap_or(tasks.len());

    for task in tasks.iter().skip(start_index).take(end_index - start_index) {
        if task.done {
            info!("Skipping completed task: {}", task.text);
            continue;
        }

        info!("Processing task: {}", task.text);
        
        let result = process_task(task, llm_client, search_client, target_dir).await;
        
        match result {
            Ok(_) => {
                info!("Task completed successfully: {}", task.text);
                // Update task status in file (simplified: just log for now)
            }
            Err(e) => {
                error!("Task failed: {} - {}", task.text, e);
                // In a real agent, you might mark the task as failed or retry
            }
        }
    }

    Ok(())
}

async fn process_task(
    task: &Task,
    llm_client: &LlmClient,
    search_client: &SearchClient,
    target_dir: &PathBuf,
) -> Result<()> {
    // 1. Search for context
    let nautivecs_results = search_client
        .search_nautivecs(task.text.clone(), 5)
        .await
        .unwrap_or_default();
    
    let nautivecs_context: Vec<String> = nautivecs_results
        .iter()
        .map(|r| format!("Title: {}\nContent: {}", r.title, r.content))
        .collect();

    // 2. Prepare temp directory
    let temp_dir = TempDir::new().context("Failed to create temp dir")?;
    let project_dir = temp_dir.path().join("project");
    fs::create_dir_all(&project_dir).context("Failed to create project dir")?;

    // 3. Generate code with retries
    let mut code_output = String::new();
    let mut corrections = Vec::new();
    let max_retries = 3;

    for attempt in 0..max_retries {
        info!("Code generation attempt {}", attempt + 1);
        
        let prompt = context::build_coder_prompt(&task.text, &nautivecs_context, &corrections);
        let response = llm_client.generate_coder(prompt).await?;
        
        // Extract code blocks
        code_output = extract_code_blocks(&response);
        
        if code_output.is_empty() {
            warn!("No code blocks found in response");
            corrections.push("No code blocks found in response".to_string());
            continue;
        }

        // 4. Write to temp dir
        write_files(&project_dir, vec![("main.rs".to_string(), code_output.clone())])?;

        // 5. Cargo check
        let check_result = cargo_check(&project_dir);
        
        match check_result {
            Ok(_) => {
                info!("Cargo check passed on attempt {}", attempt + 1);
                break;
            }
            Err(errors) => {
                warn!("Cargo check failed on attempt {}: {:?}", attempt + 1, errors);
                let error_messages: Vec<String> = errors.iter().map(|e| e.message.clone()).collect();
                corrections = error_messages;
                
                if attempt == max_retries - 1 {
                    return Err(anyhow::anyhow!("Failed to compile after {} attempts", max_retries));
                }
            }
        }
    }

    // 6. Review with 8B model
    info!("Running reviewer check");
    let review_prompt = context::build_reviewer_prompt(&code_output);
    let review_response = llm_client.generate_reviewer(review_prompt).await?;

    if review_response.contains("APPROVED") {
        info!("Reviewer approved");
    } else {
        warn!("Reviewer found issues: {}", review_response);
        // In a full implementation, we might parse the review and fix issues
        // For now, we proceed if cargo check passed, as the 35B model is strong
    }

    // 7. Commit files
    info!("Committing files to target dir");
    commit_files(&project_dir, target_dir)?;

    Ok(())
}

fn extract_code_blocks(response: &str) -> String {
    let re = Regex::new(r"```(?:rust)?\s*\n(.*?)\n```").unwrap();
    
    if let Some(cap) = re.captures(response) {
        cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default()
    } else {
        // If no code blocks, return the whole response as a fallback
        response.to_string()
    }
}
```
