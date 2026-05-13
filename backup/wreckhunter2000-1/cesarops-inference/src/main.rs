// src/main.rs — cesarops-inference binary entry point
//!
//! Loads a GGUF model, audits hardware, and starts the KoboldCPP-compatible server.
//! The server runs real inference: embed → transformer forward → sample → decode.

use std::path::PathBuf;
use std::sync::Arc;
use std::env;
use tracing::{info, warn, Level};
use tracing_subscriber::FmtSubscriber;

use cesarops_inference::hardware;
use cesarops_inference::loader;
use cesarops_inference::arena::InferenceArena;
use cesarops_inference::tokenizer::ZeroAllocBpeTokenizer;
use cesarops_inference::server;

#[derive(Debug)]
struct Args {
    model: PathBuf,
    tokenizer: Option<PathBuf>,
    port: u16,
}

fn parse_args() -> Result<Args, anyhow::Error> {
    let args: Vec<String> = env::args().collect();
    let mut model = None;
    let mut tokenizer = None;
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
            "--tokenizer" => {
                if i + 1 < args.len() {
                    tokenizer = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    return Err(anyhow::anyhow!("Missing value for --tokenizer"));
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
    Ok(Args { model, tokenizer, port })
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
            eprintln!("CLI Error: {}", e);
            eprintln!("Usage: cesarops-inference --model <path.gguf> [--tokenizer <tokenizer.json>] [--port 5001]");
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
    info!("Model loaded: {} ({} layers, {} hidden, {} heads, {} kv_heads, {} vocab, {} tensors)",
        model_name, weights.n_layers, weights.hidden_dim, weights.n_heads,
        weights.n_kv_heads, weights.vocab_size, weights.tensors.len());

    // Print first few tensor names for debugging
    info!("Sample tensors:");
    for (i, name) in weights.tensors.keys().take(8).enumerate() {
        info!("  [{}] {}", i, name);
    }

    info!("Allocating 1GB inference arena (NUMA node 0)...");
    let arena = InferenceArena::new(1024 * 1024 * 1024, 0);

    // Load tokenizer — try explicit path, then auto-detect next to model
    let tokenizer = if let Some(tok_path) = &args.tokenizer {
        info!("Loading tokenizer from {:?}...", tok_path);
        match ZeroAllocBpeTokenizer::load_from_file(tok_path, Arc::clone(&arena)) {
            Ok(t) => {
                info!("Tokenizer loaded: {} vocab entries, {} merges",
                    t.vocab.len(), t.merges.len());
                Arc::new(t)
            }
            Err(e) => {
                warn!("Failed to load tokenizer: {} — falling back to byte-level", e);
                Arc::new(ZeroAllocBpeTokenizer::new(Arc::clone(&arena)))
            }
        }
    } else {
        // Try to find tokenizer.json next to the model file
        let auto_path = args.model.parent()
            .map(|p| p.join("tokenizer.json"))
            .unwrap_or_default();
        if auto_path.exists() {
            info!("Auto-detected tokenizer at {:?}", auto_path);
            match ZeroAllocBpeTokenizer::load_from_file(&auto_path, Arc::clone(&arena)) {
                Ok(t) => {
                    info!("Tokenizer loaded: {} vocab entries, {} merges",
                        t.vocab.len(), t.merges.len());
                    Arc::new(t)
                }
                Err(e) => {
                    warn!("Failed to load auto-detected tokenizer: {} — byte-level fallback", e);
                    Arc::new(ZeroAllocBpeTokenizer::new(Arc::clone(&arena)))
                }
            }
        } else {
            info!("No tokenizer.json found — using byte-level tokenization");
            Arc::new(ZeroAllocBpeTokenizer::new(Arc::clone(&arena)))
        }
    };

    let weights = Arc::new(weights);

    info!("Starting server on port {}...", args.port);
    server::run_server(model_name, args.port, weights, arena, tokenizer).await?;

    Ok(())
}
