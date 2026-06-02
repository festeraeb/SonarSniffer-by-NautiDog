//! Pilot graph runner: executes mission pipeline stages via HTTP (Forge + nautivecs).
//! Replaces separate per-node binaries until full dora-rs nodes land.

use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use std::time::Duration;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "cesarops-dora-runner")]
struct Cli {
    #[arg(long, default_value = "/codebase/repos/wreckhunter2000-1/infra/nomad/dora/mission_pilot_graph.yaml")]
    graph: String,

    #[arg(long, default_value = "http://127.0.0.1:9100")]
    forge_url: String,

    #[arg(long, default_value = "http://127.0.0.1:5003")]
    nautivecs_url: String,

    #[arg(long, default_value = "http://127.0.0.1:5678")]
    n8n_url: String,

    /// Run once and exit (batch). Default: service loop every 5 minutes.
    #[arg(long)]
    once: bool,

    #[arg(long, default_value_t = 300)]
    interval_secs: u64,
}

#[derive(Debug, Deserialize)]
struct GraphFile {
    nodes: Vec<GraphNode>,
}

#[derive(Debug, Deserialize)]
struct GraphNode {
    id: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cesarops_dora_runner=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let graph_raw = std::fs::read_to_string(&cli.graph)
        .with_context(|| format!("read graph {}", cli.graph))?;
    let graph: GraphFile = serde_yaml::from_str(&graph_raw).context("parse graph yaml")?;
    info!(nodes = graph.nodes.len(), graph = %cli.graph, "loaded pilot graph");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    loop {
        if let Err(e) = run_pipeline(&client, &cli, &graph).await {
            warn!(error = %e, "pilot pipeline iteration failed");
        }
        if cli.once {
            break;
        }
        tokio::time::sleep(Duration::from_secs(cli.interval_secs)).await;
    }
    Ok(())
}

async fn run_pipeline(client: &reqwest::Client, cli: &Cli, graph: &GraphFile) -> Result<()> {
    info!(step = "mission-intake", "forge health");
    let health = client
        .get(format!("{}/health", cli.forge_url.trim_end_matches('/')))
        .send()
        .await?
        .error_for_status()?;
    info!(status = %health.status(), "forge ok");

    info!(step = "context-enrich", "nautivecs health");
    let nv = client
        .get(format!("{}/health", cli.nautivecs_url.trim_end_matches('/')))
        .send()
        .await?;
    if nv.status().is_success() {
        info!(status = %nv.status(), "nautivecs ok");
    } else {
        warn!(status = %nv.status(), "nautivecs degraded — continuing");
    }

    info!(step = "infer-dispatch", "forge mcp-stack");
    let stack = client
        .get(format!(
            "{}/forge/mcp-stack",
            cli.forge_url.trim_end_matches('/')
        ))
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;
    let up = stack
        .get("status")
        .and_then(|s| s.as_object())
        .map(|o| o.values().filter(|v| v.get("online") == Some(&serde_json::Value::Bool(true))).count())
        .unwrap_or(0);
    info!(services_up = up, "mcp stack probed");

    info!(step = "result-validate", "orchestration probe");
    let orch = client
        .get(format!(
            "{}/cluster/orchestration",
            cli.forge_url.trim_end_matches('/')
        ))
        .send()
        .await;
    match orch {
        Ok(r) if r.status().is_success() => info!("orchestration ok"),
        Ok(r) => warn!(status = %r.status(), "orchestration non-200"),
        Err(e) => warn!(error = %e, "orchestration unreachable"),
    }

    info!(step = "publish-out", "n8n healthz (optional)");
    let n8n = client
        .get(format!("{}/healthz", cli.n8n_url.trim_end_matches('/')))
        .send()
        .await;
    match n8n {
        Ok(r) if r.status().is_success() => info!("n8n ok"),
        Ok(r) => warn!(status = %r.status(), "n8n non-200"),
        Err(e) => warn!(error = %e, "n8n unreachable"),
    }

    info!(
        nodes = ?graph.nodes.iter().map(|n| &n.id).collect::<Vec<_>>(),
        "pilot graph iteration complete"
    );
    Ok(())
}
