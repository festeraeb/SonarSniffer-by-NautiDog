//! cesarops-mcp-steered — MCP server with nautivecs vector injection
//!
//! Exposes tools to any MCP client (Kiro, Claude Desktop, etc.) that:
//! 1. Accept a query/task from the client
//! 2. Query nautivecs for relevant codebase context
//! 3. Inject that context into the system prompt
//! 4. Forward to any OpenAI-compatible LLM endpoint
//! 5. Return the grounded response
//!
//! The LLM gets to think creatively, but nautivecs injection keeps it
//! anchored to real code, real functions, real parameters.
//!
//! Usage:
//!   cesarops-mcp --llm-url https://llm.cesarops.org/v1 --nautivecs-db ./data/nautivecs_store.json
//!
//! Or via environment:
//!   LLM_URL=https://llm.cesarops.org/v1 NAUTIVECS_DB=./data/nautivecs_store.json cesarops-mcp

mod llm_client;
mod steering;
mod tools;
mod feedback;
mod verification;
mod research_engine;
mod scm;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug, Clone)]
#[command(name = "cesarops-mcp")]
#[command(about = "Vector-steered MCP server — grounded LLM reasoning via nautivecs injection")]
pub struct Config {
    /// OpenAI-compatible LLM endpoint URL
    #[arg(long, env = "LLM_URL", default_value = "https://llm.cesarops.org/v1")]
    pub llm_url: String,

    /// Model name to request from the endpoint
    #[arg(long, env = "LLM_MODEL", default_value = "default")]
    pub llm_model: String,

    /// API key for the LLM endpoint (if required)
    #[arg(long, env = "LLM_API_KEY", default_value = "local")]
    pub llm_api_key: String,

    /// Path to nautivecs JSON vector store
    #[arg(long, env = "NAUTIVECS_DB", default_value = "./data/nautivecs_store.json")]
    pub nautivecs_db: String,

    /// Embedding endpoint for nautivecs hybrid search
    #[arg(long, env = "EMBEDDING_URL", default_value = "https://llm.cesarops.org/v1")]
    pub embedding_url: String,

    /// Max tokens for nautivecs context injection (budget)
    #[arg(long, env = "CONTEXT_BUDGET", default_value_t = 4096)]
    pub context_budget: usize,

    /// Temperature for LLM inference
    #[arg(long, env = "LLM_TEMPERATURE", default_value_t = 0.7)]
    pub temperature: f32,

    /// Max tokens for LLM response
    #[arg(long, env = "LLM_MAX_TOKENS", default_value_t = 2048)]
    pub max_tokens: u32,

    /// n8n webhook URL for passive decision logging (optional)
    #[arg(long, env = "N8N_WEBHOOK_URL")]
    pub n8n_webhook_url: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("cesarops_mcp=info".parse()?))
        .with_writer(std::io::stderr)
        .init();

    let config = Config::parse();

    tracing::info!("cesarops-mcp-steered starting");
    tracing::info!("  LLM endpoint: {}", config.llm_url);
    tracing::info!("  Model: {}", config.llm_model);
    tracing::info!("  nautivecs DB: {}", config.nautivecs_db);
    tracing::info!("  Context budget: {} tokens", config.context_budget);

    // Initialize the MCP server with our tools
    tools::serve_mcp(config).await
}
