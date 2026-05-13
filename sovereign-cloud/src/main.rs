mod allocation;
mod api;
mod discovery;
mod idle_scout;
mod pipeline;
mod research_engine;
mod tile_store;
mod tpu;

use anyhow::Result;
use std::sync::Arc;
use tracing::info;

const API_PORT: u16 = 8765;
const TILE_STORE_PATH: &str = "./data/tiles";
const RESEARCH_DB_PATH: &str = "./data/research.json";
/// n8n webhook URL — set via N8N_ALERT_URL env var or leave None for console alerts
const ENV_ALERT_URL: &str = "N8N_ALERT_URL";

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Init structured logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("=== CESARops Sovereign Cloud Node ===");

    // 1b. Initialise Edge TPU if present (libedgetpu FFI, Rust-native — no Python)
    let _tpu_ctx = tpu::try_init();

    // 2. Scan hardware — query NVML then wgpu fallback
    let allocation = Arc::new(allocation::AllocationEngine::detect().await?);
    let role = allocation.determine_role().await;
    info!("Assigned role: {:?}", role);

    // 3. Open tile store (sled-backed; swap for LanceDB by implementing TileStore trait)
    std::fs::create_dir_all(TILE_STORE_PATH).ok();
    let store: Arc<dyn tile_store::TileStore> =
        Arc::new(tile_store::SledTileStore::open(TILE_STORE_PATH)?);

    // 4. Build pipeline manager
    let alert_url = std::env::var(ENV_ALERT_URL).ok();
    if alert_url.is_some() {
        info!("Alert endpoint configured: {}", alert_url.as_deref().unwrap_or(""));
    } else {
        info!("No N8N_ALERT_URL set — anomaly alerts will print to console");
    }
    let pipeline = Arc::new(pipeline::PipelineManager::new(
        allocation.clone(),
        store.clone(),
        alert_url,
    ));

    // 4b. Build research engine
    let llm_base = std::env::var("LLM_BASE_URL")
        .or_else(|_| std::env::var("KOBOLD_BASE_URL"))
        .or_else(|_| std::env::var("OLLAMA_BASE_URL"))
        .unwrap_or_else(|_| "http://localhost:5001/v1".into());
    
    std::fs::create_dir_all("./data").ok();
    let research = Arc::new(research_engine::ResearchEngine::new(
        llm_base,
        RESEARCH_DB_PATH.to_string(),
    ));
    // Load existing findings
    {
        let (f, h) = research_engine::ResearchEngine::load_from_disk(RESEARCH_DB_PATH);
        *research.findings.write().await = f;
        *research.hypotheses.write().await = h;
        info!("ResearchEngine: loaded {} findings and {} hypotheses", 
            research.findings.read().await.len(),
            research.hypotheses.read().await.len());
    }

    // 5. Broadcast node capabilities via mDNS so Pi dispatcher and peers can discover us
    let discovery = Arc::new(discovery::NodeDiscovery::new()?);
    {
        let caps = allocation.capabilities.read().await;
        discovery.announce(&caps, API_PORT)?;
    }
    discovery.browse_peers().await?;
    discovery.spawn_periodic_discovery(); // Active periodic polling across mDNS + Tailscale
    // Give mDNS daemon a moment to start discovering peers
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    info!("mDNS: node announced on port {}", API_PORT);

    // 6. Build node state first so the idle scout can write hits/cell to it
    let node_state = api::NodeState::new(
        allocation.clone(), 
        pipeline.clone(), 
        Some(discovery.clone()),
        Some(research.clone()),
    );

    // 7. Spawn self-annealing idle scout background task
    idle_scout::spawn(node_state.clone());
    info!("IdleScout: background loop active");
    let shutdown_notify = node_state.shutdown.clone();
    let app = api::router(node_state.clone());

    let bind_addr = std::env::var("API_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0".into());
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind_addr, API_PORT)).await?;
    info!("API listening on http://{}:{}", bind_addr, API_PORT);
    info!("OpenAI-compatible endpoint: POST /v1/chat/completions");
    info!("Pipeline dispatch:          POST /v1/pipeline/dispatch");
    info!("Full pipeline run:          POST /v1/pipeline/run");
    info!("Node status:               GET  /v1/node/status");
    info!("Laptop mode on:            POST /v1/laptop/on");
    info!("Laptop mode off:           POST /v1/laptop/off");
    info!("Shutdown:                  POST /v1/node/shutdown");

    tokio::select! {
        result = axum::serve(listener, app) => { result?; }
        _ = async { shutdown_notify.notified().await } => {
            info!("Graceful shutdown complete.");
        }
    }

    Ok(())
}
