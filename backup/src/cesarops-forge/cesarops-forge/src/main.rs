use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

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
    #[arg(short = 'd', long)]
    target_dir: PathBuf,

    /// Specific task index to run (0-based). If omitted, runs all.
    #[arg(short = 'n', long)]
    task: Option<usize>,

    /// Show status of tasks
    #[arg(long)]
    status: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("cesarops_forge=info")
        .init();

    let args = Args::parse();

    if args.status {
        if args.tasks.exists() {
            let content = tokio::fs::read_to_string(&args.tasks).await?;
            let tasks = context::parse_tasks(&content);
            for task in &tasks {
                let status = if task.done { "DONE" } else { "PENDING" };
                println!("[{}] Task {}: {}", status, task.index, task.text);
            }
        } else {
            println!("Tasks file not found: {:?}", args.tasks);
        }
        return Ok(());
    }

    let llm_client = llm::LlmClient::new(
        "http://100.72.182.77:5001",
        "http://100.102.158.111:5555",
    );
    let search_client = search::SearchClient::new(
        "http://100.72.182.77:5003",
        "http://100.72.182.77:5010",
    );

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
