use axum::{Router, routing::{get, post}};
use cesarops_agent::dashboard::{dashboard_handler, ws_handler, status_handler, workflows_handler, trigger_handler, decisions_handler};
use cesarops_agent::workflow::{Workflow, WorkflowContext};
use cesarops_agent::nodes::Node;
use tokio::sync::broadcast;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    // Broadcast channel for real-time dashboard events
    let (tx, _rx) = broadcast::channel(100);
    let tx = Arc::new(tx);

    // Load and initialize workflows
    let workflows: Vec<(Uuid, Workflow)> = load_workflows();
    let mut ctx = WorkflowContext::new();

    // Background orchestrator loop
    let wf_tx = tx.clone();
    tokio::spawn(async move {
        for (_id, mut wf) in workflows {
            loop {
                // Compute before move: execute DAG, collect outputs, then broadcast
                if let Err(e) = wf.execute(&mut ctx).await {
                    tracing::error!("Workflow execution failed: {e}");
                }
                
                // Annotate collect types: Vec<(WorkflowId, NodeId, serde_json::Value)>
                let events: Vec<(Uuid, Uuid, serde_json::Value)> = ctx.broadcasts.drain(..).collect();
                for event in events {
                    let payload = serde_json::json!({
                        "type": "event",
                        "workflow": event.0,
                        "node": event.1,
                        "data": event.2
                    });
                    let _ = wf_tx.send(payload);
                }
                
                // Poll interval for workflow triggers
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            }
        }
    });

    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/status", get(status_handler))
        .route("/workflows", get(workflows_handler))
        .route("/workflows/{id}/trigger", post(trigger_handler))
        .route("/decisions", get(decisions_handler))
        .route("/ws", get(ws_handler));

    tracing::info!("CesarOps Agent listening on 0.0.0.0:9000");
    let listener = tokio::net::TcpListener::bind("0.0.0.0:9000").await.unwrap();
    axum::serve(listener, app.into_make_service()).await.unwrap();
}

fn load_workflows() -> Vec<(Uuid, Workflow)> {
    // Generate deterministic IDs for DAG edges
    let ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
    
    vec![(
        ids[0],
        Workflow::new(
            "Self-Annealing Loop".to_string(),
            vec![
                (ids[1], Node::HealthCheck { url: "http://localhost:5001/health".to_string(), threshold: 0.9 }),
                (ids[2], Node::LlmDecision { prompt: "KoboldCPP is down, GPU shows OOM. What do?".to_string(), system: "You are a system administrator. Respond with a concise action plan.".to_string() }),
                (ids[3], Node::ModelSwap { model_id: "coder14".to_string() }),
                (ids[4], Node::ProcessControl { action: "restart".to_string(), service: "koboldcpp".to_string() }),
            ],
            vec![
                (ids[1], ids[2]),
                (ids[2], ids[3]),
                (ids[3], ids[4]),
            ]
        )
    )]
}
