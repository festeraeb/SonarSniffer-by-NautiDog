use std::path::PathBuf;

mod capabilities;
mod model_registry;
mod server;
mod tools;

use clap::{Parser, ValueHint};
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "cesarops-mcp-worker", version, about = "MCP-compatible GPU worker for self-organizing agent swarm")]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// GPU index to use (for multi-GPU systems)
    #[arg(long, default_value_t = 0)]
    gpu_index: usize,

    /// Worker specialty override (coder, thinker, general)
    #[arg(long, default_value = "general")]
    specialty: String,

    /// Project root path for tool execution
    #[arg(long, value_hint = ValueHint::DirPath, default_value = ".")]
    project_root: PathBuf,

    /// Model name/path (optional — auto-select if not given)
    #[arg(long)]
    model: Option<String>,

    /// Koboldcpp endpoint URL
    #[arg(long, default_value = "http://localhost:5001")]
    koboldcpp_url: String,

    /// Model registry directory
    #[arg(long, value_hint = ValueHint::DirPath, default_value = "/codebase/models")]
    models_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    info!("Starting cesarops-mcp-worker on port {}", args.port);
    info!("GPU index: {}, Specialty: {}", args.gpu_index, args.specialty);
    info!("Project root: {:?}", args.project_root);
    info!("Koboldcpp URL: {}", args.koboldcpp_url);
    info!("Models dir: {:?}", args.models_dir);

    // Discover worker capabilities
    let manifest = capabilities::discover_self(&args.specialty, args.model.clone());
    info!("Worker manifest: {:#?}", manifest);

    // Scan for available models
    let registry = model_registry::ModelRegistry::scan(&args.models_dir);
    info!("Loaded {} models from registry", registry.models.len());

    // Auto-select model if not specified
    if let Some(ref model_path) = args.model {
        info!("Using explicitly specified model: {}", model_path);
    } else {
        if !registry.models.is_empty() {
            let selected = registry.select_model_for_task(&args.specialty, manifest.vram_free_mb);
            if let Some(model) = selected {
                info!("Auto-selected model: {} (specialty={}, vram_needed={}MB)", 
                      model.path, model.specialty, model.estimated_vram_mb);
            } else {
                warn!("No suitable model found in registry for specialty '{}' with {}MB VRAM free",
                      args.specialty, manifest.vram_free_mb);
            }
        } else {
            warn!("No models found in {}. Set --model to specify one.", args.models_dir.display());
        }
    }

    // Create and run the HTTP server
    let app = server::create_app(
        manifest,
        registry,
        args.project_root,
        args.koboldcpp_url,
    );

    let addr = format!("0.0.0.0:{}", args.port);
    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
