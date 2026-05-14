use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;
use crate::llm::LlmClient;
use crate::harness::{AgentHarness, default_tools};
use crate::router::{SubAgentRouter, default_cluster_routes};
use std::time::Duration;

pub type NodeId = Uuid;

#[derive(Debug, Clone)]
pub enum Node {
    HealthCheck { url: String, threshold: f64 },
    Timer { cron: String },
    LlmDecision { prompt: String, system: String },
    ProcessControl { action: String, service: String },
    ModelSwap { model_id: String },
    Alert { message: String, channel: String },
    ShellExec { command: String },
    HttpRequest { method: String, url: String, headers: Option<Value> },
    Condition { expression: String },
    SelfReplace { binary_path: String },
    /// Autonomous agent task — runs the harness loop against the active LLM.
    AgentTask {
        task: String,
        llm_endpoint: String,
        max_steps: u32,
        timeout_secs: u64,
    },
    /// Route intent to a sub-agent on the cluster.
    RouteToSubAgent {
        intent: String,
        fallback_endpoint: String,
    },
}

#[derive(Debug, Clone)]
pub struct NodeOutput {
    pub data: Value,
    pub success: bool,
}

impl Node {
    // Enum dispatch: match on node variant to compute behavior
    pub async fn execute(&self, context: &HashMap<NodeId, Value>) -> Result<NodeOutput, String> {
        match self {
            Self::HealthCheck { url, threshold } => {
                let resp = reqwest::get(url).await.map_err(|e| e.to_string())?;
                let status = resp.status().as_u16();
                Ok(NodeOutput { 
                    data: serde_json::json!({"status": status, "healthy": status < 500, "threshold": threshold}), 
                    success: status < 500 
                })
            }
            Self::Timer { cron } => {
                Ok(NodeOutput { 
                    data: serde_json::json!({"triggered": true, "cron": cron}), 
                    success: true 
                })
            }
            Self::LlmDecision { prompt, system } => {
                let llm = LlmClient::new();
                let decision = llm.decide(system, prompt).await.map_err(|e| e.to_string())?;
                Ok(NodeOutput { 
                    data: serde_json::json!({"decision": decision}), 
                    success: true 
                })
            }
            Self::ProcessControl { action, service } => {
                // Mock watchdog API dispatch
                Ok(NodeOutput { 
                    data: serde_json::json!({"action": action, "service": service, "result": "ok"}), 
                    success: true 
                })
            }
            Self::ModelSwap { model_id } => {
                Ok(NodeOutput { 
                    data: serde_json::json!({"model": model_id, "status": "swapped"}), 
                    success: true 
                })
            }
            Self::Alert { message, channel } => {
                Ok(NodeOutput { 
                    data: serde_json::json!({"alert": message, "channel": channel}), 
                    success: true 
                })
            }
            Self::ShellExec { command } => {
                let output = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .output()
                    .map_err(|e| e.to_string())?;
                Ok(NodeOutput { 
                    data: serde_json::json!({
                        "stdout": String::from_utf8_lossy(&output.stdout),
                        "stderr": String::from_utf8_lossy(&output.stderr)
                    }), 
                    success: output.status.success() 
                })
            }
            Self::HttpRequest { method, url, headers: _headers } => {
                let builder = reqwest::Client::new().request(
                    method.parse().unwrap_or(reqwest::Method::GET), 
                    url
                );
                let resp = builder.send().await.map_err(|e| e.to_string())?;
                let status = resp.status().as_u16();
                let body = resp.text().await.map_err(|e| e.to_string())?;
                Ok(NodeOutput { 
                    data: serde_json::json!({"body": body, "status": status}), 
                    success: true 
                })
            }
            Self::Condition { expression } => {
                // Simplified condition evaluation
                Ok(NodeOutput { 
                    data: serde_json::json!({"result": true, "expression": expression}), 
                    success: true 
                })
            }
            Self::SelfReplace { binary_path } => {
                Ok(NodeOutput { 
                    data: serde_json::json!({"binary": binary_path, "status": "replaced"}), 
                    success: true 
                })
            }
            Self::AgentTask { task, llm_endpoint, max_steps, timeout_secs } => {
                let harness = AgentHarness::new(
                    llm_endpoint.clone(),
                    default_tools(),
                    *max_steps,
                    Duration::from_secs(*timeout_secs),
                );
                let result = harness.run_task(task).await;
                Ok(NodeOutput {
                    data: serde_json::json!({
                        "task": result.task,
                        "response": result.final_response,
                        "steps": result.steps_executed,
                        "duration_secs": result.execution_duration_secs,
                        "success": result.success,
                    }),
                    success: result.success,
                })
            }
            Self::RouteToSubAgent { intent, fallback_endpoint } => {
                let routes = default_cluster_routes();
                let router = SubAgentRouter::new(routes, fallback_endpoint.clone());
                let decision = router.route(intent);
                let payload = serde_json::json!({"prompt": intent, "max_length": 256});
                match router.dispatch(&decision, payload).await {
                    Ok(response) => Ok(NodeOutput {
                        data: serde_json::json!({
                            "agent": decision.agent_name,
                            "endpoint": decision.endpoint,
                            "keyword": decision.matched_keyword,
                            "response": response,
                        }),
                        success: true,
                    }),
                    Err(e) => Ok(NodeOutput {
                        data: serde_json::json!({
                            "agent": decision.agent_name,
                            "error": e,
                        }),
                        success: false,
                    }),
                }
            }
        }
    }
}
