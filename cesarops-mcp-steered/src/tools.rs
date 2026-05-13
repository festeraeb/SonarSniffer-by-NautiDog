//! MCP Tool definitions — exposed to Kiro/Claude Desktop/any MCP client
//!
//! Each tool:
//! 1. Receives input from the MCP client
//! 2. Queries nautivecs for relevant context (steering)
//! 3. Sends steered prompt to the LLM
//! 4. Returns grounded response

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::llm_client::LlmClient;
use crate::scm::PipelineCoordinator;
use crate::steering::SteeringEngine;
use crate::Config;

/// Tool input schemas
#[derive(Debug, Deserialize)]
struct SteeredQueryInput {
    query: String,
    /// Optional role hint for more targeted context injection
    role: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TuneParametersInput {
    /// What parameters to tune (e.g., "detection thresholds for post-storm Erie tiles")
    task: String,
    /// Current parameter values (JSON object)
    current_params: Option<Value>,
    /// Constraints or context about the data
    constraints: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnalyzeCodeInput {
    /// What to analyze (function name, file path, or description)
    target: String,
    /// What kind of analysis (explain, optimize, debug, extend)
    analysis_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReindexInput {
    /// Directory to re-index
    path: String,
}

#[derive(Debug, Deserialize)]
struct ExecuteObjectiveInput {
    /// High-level objective to decompose and execute through the SCM pipeline
    objective: String,
}

/// Shared state for all tools
struct ToolState {
    llm: LlmClient,
    steering: SteeringEngine,
    pipeline: PipelineCoordinator,
}

/// Serve the MCP protocol over stdio
pub async fn serve_mcp(config: Config) -> Result<()> {
    // Initialize components
    let llm = LlmClient::new(
        &config.llm_url,
        &config.llm_model,
        &config.llm_api_key,
        config.temperature,
        config.max_tokens,
    );

    let steering = SteeringEngine::init(
        &config.nautivecs_db,
        &config.embedding_url,
        config.context_budget,
    )
    .await?;

    let state = Arc::new(Mutex::new(ToolState { llm, steering, pipeline: PipelineCoordinator::new() }));

    // MCP server loop over stdio
    // Read JSON-RPC messages from stdin, write responses to stdout
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    tracing::info!("MCP server ready — listening on stdio");

    let mut reader = tokio::io::BufReader::new(stdin);
    let mut line = String::new();

    loop {
        line.clear();
        use tokio::io::AsyncBufReadExt;
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break; // EOF — client disconnected
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Parse JSON-RPC request
        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Invalid JSON-RPC: {}", e);
                continue;
            }
        };

        let method = request["method"].as_str().unwrap_or("");
        let id = request["id"].clone();
        let params = request["params"].clone();

        let response = match method {
            "initialize" => handle_initialize(&id),
            "tools/list" => handle_tools_list(&id),
            "tools/call" => handle_tool_call(&id, &params, state.clone()).await,
            "notifications/initialized" => continue, // no response needed
            _ => json_rpc_error(&id, -32601, &format!("Method not found: {}", method)),
        };

        // Write response to stdout
        use tokio::io::AsyncWriteExt;
        let response_str = serde_json::to_string(&response)?;
        let mut out = tokio::io::stdout();
        out.write_all(response_str.as_bytes()).await?;
        out.write_all(b"\n").await?;
        out.flush().await?;
    }

    Ok(())
}

fn handle_initialize(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "cesarops-mcp-steered",
                "version": "0.1.0"
            }
        }
    })
}

fn handle_tools_list(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "tools": [
                {
                    "name": "steered_query",
                    "description": "Ask a question grounded in the CESARops codebase. The LLM receives relevant code context via nautivecs vector injection, preventing hallucination while preserving creative reasoning.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Your question or task"
                            },
                            "role": {
                                "type": "string",
                                "description": "Optional role hint for targeted context: sensor, geometry, orchestrator, research",
                                "enum": ["sensor", "geometry", "orchestrator", "research"]
                            }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "tune_parameters",
                    "description": "Get parameter tuning recommendations grounded in actual codebase thresholds and detection logic. Returns JSON with suggested values and rationale.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "task": {
                                "type": "string",
                                "description": "What parameters to tune and for what scenario"
                            },
                            "current_params": {
                                "type": "object",
                                "description": "Current parameter values (optional)"
                            },
                            "constraints": {
                                "type": "string",
                                "description": "Data constraints or context (e.g., 'post-storm Erie tiles, 15m depth')"
                            }
                        },
                        "required": ["task"]
                    }
                },
                {
                    "name": "analyze_code",
                    "description": "Analyze code from the CESARops workspace with full project context. The LLM sees related functions, data structures, and usage patterns.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "target": {
                                "type": "string",
                                "description": "Function name, file path, or description of what to analyze"
                            },
                            "analysis_type": {
                                "type": "string",
                                "description": "Type of analysis: explain, optimize, debug, extend, review",
                                "enum": ["explain", "optimize", "debug", "extend", "review"]
                            }
                        },
                        "required": ["target"]
                    }
                },
                {
                    "name": "reindex",
                    "description": "Re-index a directory to update the nautivecs vector store with latest code changes.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Directory path to index (relative to workspace root)"
                            }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "health",
                    "description": "Check the health of the steered LLM system: LLM endpoint reachability, nautivecs store status, indexed chunk count.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "execute_shader",
                    "description": "Trigger a wgpu/WGSL compute shader on the GPU cluster. Used for heavy math operations (anomaly detection, dipole scanning, curvelet transforms) that the LLM shouldn't attempt to compute itself.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "shader_type": {
                                "type": "string",
                                "description": "Which compute shader to dispatch",
                                "enum": ["dipole_scan", "curvelet_filter", "thermal_submersion", "optical_structural", "nauticus_scan", "spectral_analysis"]
                            },
                            "parameters": {
                                "type": "object",
                                "description": "Shader-specific parameters (thresholds, grid dimensions, etc.)"
                            },
                            "target_node": {
                                "type": "string",
                                "description": "Which GPU node to dispatch to (auto = best available)",
                                "enum": ["auto", "t440", "cesarops2", "cesarops3"]
                            }
                        },
                        "required": ["shader_type"]
                    }
                },
                {
                    "name": "execute_objective",
                    "description": "Execute a high-level objective through the Segmented Context Manager (SCM) pipeline. Decomposes the objective into RSU segments, steers each through observation → reasoning → accuracy_check phases, validates outputs against fidelity rules, and returns the full pipeline result.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "objective": {
                                "type": "string",
                                "description": "The high-level objective to decompose and execute (e.g., 'Analyze thermal anomaly in tile B02_20240715 for wreck signatures')"
                            }
                        },
                        "required": ["objective"]
                    }
                },
                {
                    "name": "query_scm_metrics",
                    "description": "Query the Segmented Context Manager's monitoring metrics. Returns total segments processed, average drift, total retries, budget exhaustion count, and segment mode breakdown.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }
            ]
        }
    })
}

async fn handle_tool_call(id: &Value, params: &Value, state: Arc<Mutex<ToolState>>) -> Value {
    let tool_name = params["name"].as_str().unwrap_or("");
    let arguments = &params["arguments"];

    let result = match tool_name {
        "steered_query" => tool_steered_query(arguments, state).await,
        "tune_parameters" => tool_tune_parameters(arguments, state).await,
        "analyze_code" => tool_analyze_code(arguments, state).await,
        "reindex" => tool_reindex(arguments, state).await,
        "health" => tool_health(state).await,
        "execute_objective" => tool_execute_objective(arguments, state).await,
        "query_scm_metrics" => tool_query_scm_metrics(state).await,
        _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
    };

    match result {
        Ok(content) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": content
                }]
            }
        }),
        Err(e) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": format!("Error: {}", e)
                }],
                "isError": true
            }
        }),
    }
}

async fn tool_steered_query(args: &Value, state: Arc<Mutex<ToolState>>) -> Result<String> {
    let input: SteeredQueryInput = serde_json::from_value(args.clone())?;
    let mut state = state.lock().await;

    // Build steered context from nautivecs
    let context = state
        .steering
        .build_context(&input.query, input.role.as_deref(), None)
        .await?;

    tracing::info!(
        "steered_query: injected {} fragments from queries {:?}",
        context.fragments_used,
        context.query_terms
    );

    // Send to LLM with grounding context
    let response = state
        .llm
        .steered_completion(&context.system_prompt, &input.query)
        .await?;

    Ok(response)
}

async fn tool_tune_parameters(args: &Value, state: Arc<Mutex<ToolState>>) -> Result<String> {
    let input: TuneParametersInput = serde_json::from_value(args.clone())?;
    let mut state = state.lock().await;

    // Build context with sensor/detection focus
    let context = state
        .steering
        .build_context(&input.task, Some("sensor"), Some("tune_parameters"))
        .await?;

    // Construct a structured prompt for parameter tuning
    let mut user_prompt = format!(
        "Task: {}\n\nProvide parameter recommendations as JSON with a rationale for each value.",
        input.task
    );

    if let Some(params) = &input.current_params {
        user_prompt.push_str(&format!("\n\nCurrent parameters:\n{}", serde_json::to_string_pretty(params)?));
    }
    if let Some(constraints) = &input.constraints {
        user_prompt.push_str(&format!("\n\nConstraints: {}", constraints));
    }

    let response = state
        .llm
        .steered_completion(&context.system_prompt, &user_prompt)
        .await?;

    Ok(response)
}

async fn tool_analyze_code(args: &Value, state: Arc<Mutex<ToolState>>) -> Result<String> {
    let input: AnalyzeCodeInput = serde_json::from_value(args.clone())?;
    let mut state = state.lock().await;

    // Search for the target code
    let context = state
        .steering
        .build_context(&input.target, None, Some("analyze_code"))
        .await?;

    let analysis_type = input.analysis_type.as_deref().unwrap_or("explain");
    let user_prompt = format!(
        "Analyze the following code target: {}\n\nAnalysis type: {}\n\n\
        Provide your analysis grounded in the actual code shown in the context above.",
        input.target, analysis_type
    );

    let response = state
        .llm
        .steered_completion(&context.system_prompt, &user_prompt)
        .await?;

    Ok(response)
}

async fn tool_reindex(args: &Value, state: Arc<Mutex<ToolState>>) -> Result<String> {
    let input: ReindexInput = serde_json::from_value(args.clone())?;
    let mut state = state.lock().await;

    let path = std::path::Path::new(&input.path);
    let count = state.steering.reindex(path).await?;

    Ok(format!(
        "Re-indexed {} chunks from {}. Total chunks in store: {}",
        count,
        input.path,
        state.steering.chunk_count()
    ))
}

async fn tool_health(state: Arc<Mutex<ToolState>>) -> Result<String> {
    let state = state.lock().await;

    let llm_ok = state.llm.health_check().await.unwrap_or(false);
    let chunks = state.steering.chunk_count();

    let status = json!({
        "llm_endpoint": if llm_ok { "reachable" } else { "unreachable" },
        "nautivecs_chunks": chunks,
        "nautivecs_status": if chunks > 0 { "indexed" } else { "empty — run reindex" },
    });

    Ok(serde_json::to_string_pretty(&status)?)
}

/// Execute a high-level objective through the SCM pipeline (Requirement 2.1).
///
/// Creates a PipelineCoordinator, decomposes the objective into RSU segments,
/// executes each segment through the steered pipeline, and returns the full
/// PipelineResult as JSON.
async fn tool_execute_objective(args: &Value, state: Arc<Mutex<ToolState>>) -> Result<String> {
    let input: ExecuteObjectiveInput = serde_json::from_value(args.clone())?;
    let mut state = state.lock().await;

    tracing::info!(
        objective = %input.objective,
        "SCM pipeline: executing objective"
    );

    // Destructure to allow split borrows on the locked state
    let ToolState {
        ref llm,
        ref mut steering,
        ref mut pipeline,
    } = *state;

    // Execute the full pipeline using the shared LlmClient and SteeringEngine
    let result = pipeline
        .execute_objective(&input.objective, llm, steering)
        .await?;

    tracing::info!(
        segments = result.segments.len(),
        elapsed_ms = result.total_elapsed_ms,
        avg_drift = result.monitoring.average_drift,
        "SCM pipeline: objective completed"
    );

    Ok(serde_json::to_string_pretty(&result)?)
}

/// Query the SCM monitoring metrics (Requirement 11.5).
///
/// Returns the current MonitoringSummary from the pipeline's Monitor,
/// including total segments processed, average drift, total retries,
/// budget exhaustion count, and segment mode breakdown.
async fn tool_query_scm_metrics(state: Arc<Mutex<ToolState>>) -> Result<String> {
    let state = state.lock().await;
    let summary = state.pipeline.monitor().query_metrics();

    tracing::info!(
        total_segments = summary.total_segments_processed,
        avg_drift = summary.average_drift,
        "SCM metrics queried"
    );

    Ok(serde_json::to_string_pretty(&summary)?)
}

fn json_rpc_error(id: &Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}
