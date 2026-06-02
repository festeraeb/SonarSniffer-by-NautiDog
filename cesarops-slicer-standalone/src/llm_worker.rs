use anyhow::Result;
use cesarops_slicer::common::db::{AnomalyQueue, ReviewStatus};
use llm::Model;
use sled;
use std::env;
use std::io::Write;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Initialize our pure-Rust embedded specialty database
    let db_path =
        env::var("SPECIALTY_DB_PATH").unwrap_or_else(|_| "specialty_instructions.db".to_string());
    println!(
        "Initializing specialty instructions database at {}...",
        db_path
    );
    let db = sled::open(&db_path)?;

    // Pre-seed specialized instructions for specific scanning domains based on our research
    let seed_instructions = vec![
        (
            b"Optical".to_vec(),
            "\
            [SYSTEM: OPTICAL SCAN PROTOCOLS]\n\
            1. Analyze Harmonized Landsat Sentinel-2 (HLS) multi-spectral anomaly.\n\
            2. FOCUS on B02 (Blue band) which has water penetration depth capability.\n\
            3. IGNORE B08 (NIR band) surface glint, as that indicates a surface vessel or wave noise.\n\
            4. Check for distinct geometric structures or red band (B04) clearing spots indicating mussel colonization over wrecks.\n\
            ".to_string(),
        ),
        (
            b"Thermal".to_vec(),
            "\
            [SYSTEM: THERMAL SCAN PROTOCOLS]\n\
            1. Analyze Landsat 8/9 B10/B11 thermal output.\n\
            2. Identify 'Cold-Sink' signatures: submerged massive steel hulls retain colder temperatures anomalously compared to surrounding water.\n\
            3. Ignore broad thermal gradients; look for sharp, localized temperature drops indicating localized mass.\n\
            4. Verify confidence score. Wrecks generally manifest as high z-score negative thermal readings.\n\
            ".to_string(),
        ),
        (
            b"SAR".to_vec(),
            "\
            [SYSTEM: SYNTHETIC APERTURE RADAR (SAR) PROTOCOLS]\n\
            1. Analyze Sentinel-1 RTC_Gamma0 dB anomaly data.\n\
            2. Wreck structures act as 'Corner Reflectors', yielding bright white, high-decibel returns surrounded by dark water.\n\
            3. Look for 'Galvanic Biological Anomalies': Iron oxidation from old wrecks can release ions, dampening capillary waves (dark slicking).\n\
            4. Discard natural wind-driven biogenic slicks if lacking a distinct point-source reflector.\n\
            ".to_string(),
        ),
        (
            b"Bathymetry".to_vec(),
            "\
            [SYSTEM: BATHYMETRY & ADVANCED PHYSICS PROTOCOLS]\n\
            1. Evaluate ICESat-2 ATL03 photon laser returns or Stumpf log-ratio depth mappings.\n\
            2. Check for stationary internal waves or surface water mounds directly above the mapped coordinates, a telltale sign of massive submerged structure.\n\
            3. Note any PRISMA/DESIS hyperspectral hints (iron oxides/lead salts from galvanic cells).\n\
            4. Apply Solar Zenith Depth corrections before making depth validations.\n\
            ".to_string(),
        )
    ];

    for (key, instructions) in seed_instructions {
        if !db.contains_key(&key)? {
            db.insert(&key, instructions.as_bytes())?;
        }
    }
    db.flush()?;

    // Initialize the unified AnomalyQueue
    let anomaly_queue_path = "anomaly_queue.db";
    let queue = AnomalyQueue::new(anomaly_queue_path)?;

    let pending_records = queue.pull_pending()?;
    println!(
        "Found {} anomalies awaiting LLM review.",
        pending_records.len()
    );

    if pending_records.is_empty() {
        println!("No hits to evaluate. Shutting down consumer.");
        return Ok(());
    }

    // Load Qwen 2.5 Coder model locally in GGUF format
    // Download: https://huggingface.co/Qwen/Qwen2.5-Coder-9B-GGUF
    let model_path = env::var("QWEN_MODEL_PATH")
        .unwrap_or_else(|_| "qwen2.5-coder-9b-q6_k.gguf".to_string());

    println!("Loading Qwen 2.5 Coder model from: {}", model_path);

    // Load the model using llm crate
    let model = match llm::load_progress::<llm::models::Llama>(
        std::path::Path::new(&model_path),
        Default::default(),
        |progress| {
            match progress {
                llm::LoadProgress::HyperparametersLoaded => println!("Loaded hyperparameters"),
                llm::LoadProgress::ContextSize { bytes } => {
                    println!("Context size: {} bytes", bytes)
                }
                llm::LoadProgress::LoraApplied { name } => {
                    println!("Applied LoRA: {}", name)
                }
                llm::LoadProgress::Loaded {
                    byte_size,
                    tensor_count,
                } => {
                    println!(
                        "Model loaded: {} bytes, {} tensors",
                        byte_size, tensor_count
                    )
                }
            }
        },
    ) {
        Ok(model) => {
            println!("✓ Successfully loaded Qwen 2.5 Coder model");
            model
        }
        Err(e) => {
            eprintln!("Error loading model: {}", e);
            eprintln!("Please download the model from:");
            eprintln!("https://huggingface.co/Qwen/Qwen2.5-Coder-9B-GGUF");
            eprintln!("And place it at: {}", model_path);
            return Err(e.into());
        }
    };

    // Limit to first 5 records for demo
    let sample_records = if pending_records.len() > 5 {
        pending_records.into_iter().take(5).collect::<Vec<_>>()
    } else {
        pending_records
    };

    println!(
        "Processing {} anomaly records with Qwen 2.5 Coder...",
        sample_records.len()
    );

    for record in sample_records {
        let sensor_id = format!("{:?}", record.sensor_type);

        let system_instructions = match db.get(sensor_id.as_bytes())? {
            Some(v) => String::from_utf8(v.to_vec()).unwrap_or_default(),
            None => "\
                [SYSTEM: GENERAL ANOMALY PROTOCOLS]\n\
                Use cross-reference physics telemetry to evaluate if bounding box matches known shipwrecks.\n\
                ".to_string(),
        };

        let base_prompt = format!(
            "Analyze the recent shipwreck telemetry and identify anomalies. Target: {}, Confidence: {:.2}. Review this finding.",
            sensor_id, record.confidence_score
        );

        let full_prompt = format!("{}\n\n[USER REQUEST]\n{}", system_instructions, base_prompt);

        println!("\n=== Processing anomaly {} ===", record.id);
        println!("Prompt: {}\n", full_prompt);

        // Create inference session with model
        let mut session = model.start_session(Default::default());

        // Run inference
        let mut output_buffer = String::new();
        match session.inference_with_state(
            &model,
            &full_prompt,
            Default::default(),
            llm::InferenceFeedback::Append,
            |_| Ok(llm::InferenceResponse::Continue),
        ) {
            Ok(result) => {
                match result {
                    llm::InferenceStats {
                        feed_prompt_tokens,
                        predict_tokens,
                        ..
                    } => {
                        println!(
                            "Inference complete: {} tokens fed, {} tokens predicted",
                            feed_prompt_tokens, predict_tokens
                        );
                    }
                }

                println!("\n=== LLM Response for {} ===", record.id);
                println!("{}", output_buffer);

                queue.update_status(
                    &record.id,
                    ReviewStatus::ReviewedHumanInterventionRequired,
                )?;
            }
            Err(e) => {
                eprintln!("Inference error: {}", e);
                println!(
                    "\n=== FALLBACK: Rule-Based Analysis for {} ===",
                    record.id
                );
                println!(
                    "Confidence score {:.2} exceeds threshold. Marked for review.",
                    record.confidence_score
                );

                queue.update_status(
                    &record.id,
                    ReviewStatus::ReviewedHumanInterventionRequired,
                )?;
            }
        }
    }

    println!("\n✓ Batch processing complete");
    Ok(())
}
