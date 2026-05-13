use anyhow::{anyhow, Context, Result};
use clap::Parser;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(
    name = "model_team_tool",
    about = "Run a reasoning model and coding model as a coordinated team"
)]
struct Args {
    #[arg(long)]
    task: Option<String>,

    #[arg(long)]
    task_file: Option<PathBuf>,

    #[arg(long, default_value_t = 0.2)]
    reasoning_temperature: f32,

    #[arg(long, default_value_t = 0.1)]
    coding_temperature: f32,

    #[arg(long, default_value_t = 600)]
    reasoning_max_tokens: u32,

    #[arg(long, default_value_t = 800)]
    coding_max_tokens: u32,

    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct RoleModelConfig {
    base_url: String,
    model: String,
    api_key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Serialize)]
struct TeamRunOutput {
    task: String,
    reasoning_model: String,
    coding_model: String,
    planning_note: String,
    coding_note: String,
    reviewer_note: String,
}

fn env_or_default(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}

fn load_role_config(role: &str) -> RoleModelConfig {
    match role {
        "reasoning" => RoleModelConfig {
            base_url: env_or_default("REASONING_BASE_URL", "http://127.0.0.1:5001/v1"),
            model: env_or_default("REASONING_MODEL", "deepseek-r1-distill-qwen-7b"),
            api_key: env_or_default("REASONING_API_KEY", "not-needed"),
        },
        "coding" => RoleModelConfig {
            base_url: env_or_default("CODING_BASE_URL", "http://127.0.0.1:5002/v1"),
            model: env_or_default("CODING_MODEL", "qwen2.5-coder-7b-instruct"),
            api_key: env_or_default("CODING_API_KEY", "not-needed"),
        },
        _ => unreachable!("unknown role"),
    }
}

fn read_task(args: &Args) -> Result<String> {
    if let Some(task) = &args.task {
        let task = task.trim();
        if task.is_empty() {
            return Err(anyhow!("--task cannot be empty"));
        }
        return Ok(task.to_string());
    }

    if let Some(task_file) = &args.task_file {
        let content = fs::read_to_string(task_file)
            .with_context(|| format!("failed to read task file {}", task_file.display()))?;
        let task = content.trim();
        if task.is_empty() {
            return Err(anyhow!("task file is empty: {}", task_file.display()));
        }
        return Ok(task.to_string());
    }

    Err(anyhow!("provide --task or --task-file"))
}

async fn call_openai_compatible(
    client: &reqwest::Client,
    config: &RoleModelConfig,
    messages: &[ChatMessage],
    temperature: f32,
    max_tokens: u32,
) -> Result<String> {
    let url = format!(
        "{}/chat/completions",
        config.base_url.trim_end_matches('/')
    );
    let payload = json!({
        "model": config.model,
        "messages": messages,
        "temperature": temperature,
        "max_tokens": max_tokens
    });

    let response = client
        .post(&url)
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {}", config.api_key))
        .json(&payload)
        .send()
        .await
        .with_context(|| format!("request failed for {}", url))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .with_context(|| format!("failed reading response body for {}", url))?;

    if !status.is_success() {
        return Err(anyhow!(
            "model call failed [{}] at {}: {}",
            status,
            url,
            body
        ));
    }

    let parsed: ChatCompletionResponse = serde_json::from_str(&body)
        .with_context(|| format!("invalid chat response from {}: {}", url, body))?;

    let first = parsed
        .choices
        .first()
        .ok_or_else(|| anyhow!("no choices returned by {}", url))?;
    Ok(first.message.content.clone())
}

fn build_planning_messages(task: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            role: "system".to_string(),
            content: "You are a reasoning lead. Create a concise implementation plan for the coding agent. Include: goals, constraints, proposed files/changes, and tests.".to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!("Task:\n{}\n\nReturn a practical plan.", task),
        },
    ]
}

fn build_coding_messages(task: &str, planning_note: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            role: "system".to_string(),
            content: "You are a senior coding agent. Produce actionable implementation output from a planning note. Focus on concrete code-level steps, command snippets, and file-level changes.".to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Task:\n{}\n\nPlanning note from reasoning model:\n{}\n\nReturn the implementation output.",
                task, planning_note
            ),
        },
    ]
}

fn build_review_messages(task: &str, planning_note: &str, coding_note: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            role: "system".to_string(),
            content: "You are the reviewer. Check if the coding output satisfies the task and the plan. Return: verdict (pass/fail), risks, and required fixes.".to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Task:\n{}\n\nPlan:\n{}\n\nCoding output:\n{}\n\nReview this now.",
                task, planning_note, coding_note
            ),
        },
    ]
}

fn write_output(path: &Path, output: &TeamRunOutput) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output dir {}", parent.display()))?;
    }
    let serialized = serde_json::to_string_pretty(output)?;
    fs::write(path, serialized).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let task = read_task(&args)?;

    let reasoning_cfg = load_role_config("reasoning");
    let coding_cfg = load_role_config("coding");

    let client = reqwest::Client::new();

    let planning_note = call_openai_compatible(
        &client,
        &reasoning_cfg,
        &build_planning_messages(&task),
        args.reasoning_temperature,
        args.reasoning_max_tokens,
    )
    .await
    .context("reasoning planner step failed")?;

    let coding_note = call_openai_compatible(
        &client,
        &coding_cfg,
        &build_coding_messages(&task, &planning_note),
        args.coding_temperature,
        args.coding_max_tokens,
    )
    .await
    .context("coding executor step failed")?;

    let reviewer_note = call_openai_compatible(
        &client,
        &reasoning_cfg,
        &build_review_messages(&task, &planning_note, &coding_note),
        args.reasoning_temperature,
        args.reasoning_max_tokens,
    )
    .await
    .context("reasoning reviewer step failed")?;

    let output = TeamRunOutput {
        task,
        reasoning_model: reasoning_cfg.model,
        coding_model: coding_cfg.model,
        planning_note,
        coding_note,
        reviewer_note,
    };

    if let Some(path) = args.output.as_deref() {
        write_output(path, &output)?;
        println!("Wrote team output to {}", path.display());
    }

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
