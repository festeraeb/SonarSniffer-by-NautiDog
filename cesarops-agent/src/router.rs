//! Sub-agent intent router.
//! Maps user intent to cluster worker nodes via keyword matching or LLM classification.
//! Dispatches payloads over HTTP to the target hardware node.

use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::time::Duration;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    /// Keywords that trigger this route (matched case-insensitive against intent)
    pub keywords: Vec<String>,
    /// Human-readable name for this sub-agent
    pub agent_name: String,
    /// HTTP endpoint for this worker node (e.g. "http://100.72.182.79:5003/api/v1/generate")
    pub endpoint: String,
    /// Additional context passed alongside the dispatched payload
    pub tool_context: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub agent_name: String,
    pub endpoint: String,
    pub matched_keyword: Option<String>,
    pub context_frame: Value,
}

pub struct SubAgentRouter {
    pub routes: Vec<Route>,
    /// Fallback endpoint when no keyword matches (master node)
    pub fallback_endpoint: String,
}

impl SubAgentRouter {
    pub fn new(routes: Vec<Route>, fallback_endpoint: String) -> Self {
        Self { routes, fallback_endpoint }
    }

    /// Route based on keyword matching against the intent string.
    pub fn route(&self, intent: &str) -> RoutingDecision {
        let lower = intent.to_lowercase();

        for route in &self.routes {
            for keyword in &route.keywords {
                if lower.contains(&keyword.to_lowercase()) {
                    info!("Routed to {} via keyword '{}'", route.agent_name, keyword);
                    return RoutingDecision {
                        agent_name: route.agent_name.clone(),
                        endpoint: route.endpoint.clone(),
                        matched_keyword: Some(keyword.clone()),
                        context_frame: route.tool_context.clone(),
                    };
                }
            }
        }

        // No match — fall back to master
        info!("No keyword match, routing to master fallback");
        RoutingDecision {
            agent_name: "Master_Orchestrator".to_string(),
            endpoint: self.fallback_endpoint.clone(),
            matched_keyword: None,
            context_frame: serde_json::json!({"strategy": "full_context_pass"}),
        }
    }

    /// Dispatch a payload to the routed endpoint.
    pub async fn dispatch(&self, decision: &RoutingDecision, payload: Value) -> Result<Value, String> {
        let client = reqwest::Client::new();

        let body = serde_json::json!({
            "prompt": payload.get("prompt").and_then(|p| p.as_str()).unwrap_or(""),
            "max_length": payload.get("max_length").and_then(|m| m.as_u64()).unwrap_or(256),
            "temperature": payload.get("temperature").and_then(|t| t.as_f64()).unwrap_or(0.3),
            "context": decision.context_frame
        });

        info!("Dispatching to {} at {}", decision.agent_name, decision.endpoint);

        let res = client
            .post(&decision.endpoint)
            .json(&body)
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| format!("Dispatch to {} failed: {}", decision.agent_name, e))?;

        if !res.status().is_success() {
            return Err(format!("{} returned HTTP {}", decision.agent_name, res.status()));
        }

        res.json::<Value>().await
            .map_err(|e| format!("Invalid response from {}: {}", decision.agent_name, e))
    }
}

/// Build the default cluster routes from your current hardware layout.
/// Update endpoints here as you add/move nodes.
pub fn default_cluster_routes() -> Vec<Route> {
    vec![
        Route {
            keywords: vec!["stitch".into(), "drift".into(), "unwarp".into(), "curvelet".into(), "navigation".into()],
            agent_name: "Nav_Stitch (Node C / 1060)".to_string(),
            endpoint: "http://100.105.77.74:5570/analyze".to_string(),
            tool_context: serde_json::json!({
                "binary": "satellite_stitch",
                "assets": "swath_grid_slices",
                "precision": "f32_curvelet"
            }),
        },
        Route {
            keywords: vec!["shimmer".into(), "mass".into(), "andaste".into(), "optical".into(), "tonnage".into()],
            agent_name: "Shimmer_Mass (Node B / T440)".to_string(),
            endpoint: "http://127.0.0.1:5002/api/v1/generate".to_string(),
            tool_context: serde_json::json!({
                "binary": "optical_mass",
                "assets": "icesat2_blue_light_matrix",
                "hardware": "p100_pcie"
            }),
        },
        Route {
            keywords: vec!["galvanic".into(), "plume".into(), "rosa".into(), "electrolyte".into(), "conductivity".into()],
            agent_name: "Chemical_Plume (Node A)".to_string(),
            endpoint: "http://127.0.0.1:5002/api/v1/generate".to_string(),
            tool_context: serde_json::json!({
                "binary": "galvanic_battery",
                "assets": "conductivity_slices",
                "target": "lead_keel_spontaneous_potential"
            }),
        },
    ]
}
