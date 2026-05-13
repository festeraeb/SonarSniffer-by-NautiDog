use anyhow::{Context, Result};
use regex::Regex;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;
use tracing::{error, info, warn};

use crate::context;
use crate::llm::LlmClient;
use crate::search::SearchClient;
use crate::tools;

// Re-export parse_tasks so main.rs can use it via context module directly
pub use crate::context::parse_tasks;

const MAX_RETRIES: usize = 3;

pub async fn run_agent(
    tasks_path: &PathBuf,
    target_dir: &PathBuf,
    specific_task: Option<usize>,
    llm_client: &LlmClient,
    search_client: &SearchClient,
) -> Result<()> {
    let content = fs::read_to_string(tasks_path)
        .context("Failed to read tasks file")?;
    let tasks = context::parse_tasks(&content);

    if tasks.is_empty() {
        warn!("No tasks found in {}", tasks_path.display());
        return Ok(());
    }

    info!("Found {} tasks ({} pending)", tasks.len(), tasks.iter().filter(|t| !t.done).count());

    for task in &tasks {
        // Skip if specific task requested and this isn't it
        if let Some(target) = specific_task {
            if task.index != target {
                continue;
            }
        }

        if task.done {
            info!("Skipping completed task {}: {}", task.index, task.text);
            continue;
        }

        info!("=== Processing Task {}: {} ===", task.index, task.text);

        match process_task(task, llm_client, search_client, target_dir).await {
            Ok(_) => info!("Task {} completed successfully", task.index),
            Err(e) => error!("Task {} FAILED: {}", task.index, e),
        }
    }

    info!("Agent run complete");
    Ok(())
}

async fn process_task(
    task: &context::Task,
    llm_client: &LlmClient,
    search_client: &SearchClient,
    target_dir: &PathBuf,
) -> Result<()> {
    // 1. Research — search nautivecs for relevant context
    info!("Step 1: Searching for context...");
    let search_results = search_client
        .search_nautivecs(&task.text, 5)
        .await
        .unwrap_or_default();

    let context_snippets: Vec<String> = search_results
        .iter()
        .map(|r| format!("// From: {}\n{}", r.path, r.content))
        .collect();

    // 2. Generate code with retry loop
    let mut corrections: Vec<String> = Vec::new();
    let mut code_output = String::new();

    for attempt in 1..=MAX_RETRIES {
        info!("Step 2: Code generation (attempt {}/{})", attempt, MAX_RETRIES);

        let prompt = context::build_coder_prompt(&task.text, &context_snippets, &corrections);
        let response = llm_client.generate_coder(prompt).await;

        match response {
            Ok(text) => {
                code_output = text;
            }
            Err(e) => {
                warn!("Coder failed on attempt {}: {}", attempt, e);
                corrections.push(format!("Previous attempt failed: {}", e));
                continue;
            }
        }

        // 3. Extract code blocks
        let code_blocks = extract_code_blocks(&code_output);
        if code_blocks.is_empty() {
            warn!("No code blocks found in response");
            corrections.push("No code blocks found in response. Wrap code in ```rust blocks.".to_string());
            continue;
        }

        // 4. Write to temp dir and cargo check
        let temp_dir = TempDir::new().context("Failed to create temp dir")?;
        let project_dir = temp_dir.path().to_path_buf();

        // Write extracted code blocks as files
        let files: Vec<(String, String)> = code_blocks.iter()
            .enumerate()
            .map(|(i, block)| (format!("src/file_{}.rs", i), block.clone()))
            .collect();

        tools::write_files(&project_dir, &files)?;

        // 5. Cargo check
        info!("Step 3: Running cargo check...");
        let check_errors = tools::cargo_check(&project_dir)?;

        if check_errors.is_empty() {
            info!("Cargo check PASSED on attempt {}", attempt);

            // 6. Review with 8B
            info!("Step 4: Sending to reviewer...");
            let review_prompt = context::build_reviewer_prompt(&code_output);
            match llm_client.generate_reviewer(review_prompt).await {
                Ok(review) => {
                    if review.contains("APPROVED") || review.contains("approved") {
                        info!("Reviewer APPROVED");
                    } else {
                        warn!("Reviewer found issues: {}...", &review[..review.len().min(200)]);
                        // Feed review back as correction for one more pass
                        if attempt < MAX_RETRIES {
                            corrections.push(format!("Reviewer feedback: {}", &review[..review.len().min(500)]));
                            continue;
                        }
                    }
                }
                Err(e) => {
                    warn!("Reviewer unavailable ({}), proceeding with cargo-check-passed code", e);
                }
            }

            // 7. Commit files
            info!("Step 5: Committing to target dir...");
            tools::commit_files(&project_dir, target_dir)?;
            return Ok(());
        } else {
            // Cargo check failed — feed errors back
            let error_messages: Vec<String> = check_errors.iter()
                .map(|e| e.to_string())
                .collect();
            warn!("Cargo check failed with {} errors", error_messages.len());
            corrections = error_messages;
        }
    }

    Err(anyhow::anyhow!("Task failed after {} attempts", MAX_RETRIES))
}

/// Extracts all code blocks from a markdown-formatted LLM response.
/// Uses (?s) flag so dot matches newlines across multi-line blocks.
fn extract_code_blocks(response: &str) -> Vec<String> {
    let re = Regex::new(r"(?s)```(?:rust|toml)?\s*\n(.*?)\n```").unwrap();
    let blocks: Vec<String> = re.captures_iter(response)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect();
    info!("Extracted {} code blocks from response", blocks.len());
    blocks
}
