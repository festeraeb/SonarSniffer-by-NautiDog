// src/main.rs — cesarops-inference binary entry point
use std::path::PathBuf;
use std::env;
use tracing::{info, error, Level};
use tracing_subscriber::FmtSubscriber;

use cesarops_inference::hardware;
use cesarops_inference::loader;
use cesarops_inference::arena::InferenceArena;
use cesarops_inference::server;

#[derive(Debug)]
struct Args {
    model: PathBuf,
    port: u16,
}

fn parse_args() -> Result<Args, anyhow::Error> {
    let args: Vec<String> = env::args().collect();
    let mut model = None;
    let mut port = 5001;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--model" => {
                if i + 1 < args.len() {
                    model = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    return Err(anyhow::anyhow!("Missing value for --model"));
                }
            }
            "--port" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse::<u16>()?;
                    i += 2;
                } else {
                    return Err(anyhow::anyhow!("Missing value for --port"));
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown argument: {}", args[i]));
            }
        }
    }

    let model = model.ok_or_else(|| anyhow::anyhow!("Missing: --model <path_to_gguf>"))?;
    Ok(Args { model, port })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("cesarops-inference starting...");

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            error!("CLI Error: {}", e);
            eprintln!("Usage: cesarops-inference --model <path.gguf> [--port 5001]");
            std::process::exit(1);
        }
    };

    info!("Auditing hardware...");
    let profile = hardware::audit_system();

    info!("Loading model from {:?}...", args.model);
    let weights = loader::load(&args.model, &profile)?;
    let model_name = args.model.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    info!("Model loaded: {} ({} layers, {} tensors)",
        model_name, weights.n_layers, weights.tensors.len());

    info!("Allocating 1GB inference arena (NUMA node 0)...");
    let _arena = InferenceArena::new(1024 * 1024 * 1024, 0);

    info!("Starting server on port {}...", args.port);
    server::run_server(model_name, args.port).await?;

    Ok(())
}
