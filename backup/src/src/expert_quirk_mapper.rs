use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a tool call received from the user or a lower-tier model.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

/// Defines a transformation rule to fix "quirks" in agent expectations.
pub enum QuirkRule {
    /// Renames a tool call (e.g., "find_at" -> "get_coord")
    RenameTool { old_name: &'static str, new_name: &'static str },
    
    /// Swaps two argument positions (e.g., lon/lat order)
    SwapArgs { arg1: &'static str, arg2: &'static str },
    
    /// Injects a default value if missing
    InjectDefault { key: &'static str, value: f64 },
}

/// Registry of quirks for different agents/deployments.
/// Stored statically or in InferenceArena for zero-allocation access.
pub struct QuirkRegistry {
    rules: Vec<QuirkRule>,
}

impl QuirkRegistry {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a new quirk rule to the registry.
    pub fn add_rule(&mut self, rule: QuirkRule) {
        self.rules.push(rule);
    }

    /// Apply all relevant rules to a given tool call.
    pub fn apply_quirks(&self, mut call: ToolCall) -> ToolCall {
        for rule in &self.rules {
            match rule {
                QuirkRule::RenameTool { old_name, new_name } => {
                    if call.name == *old_name {
                        call.name = new_name.to_string();
                    }
                }
                QuirkRule::SwapArgs { arg1, arg2 } => {
                    if let Some(val1) = call.arguments.remove(*arg1) {
                        if let Some(val2) = call.arguments.remove(*arg2) {
                            call.arguments.insert(arg1.to_string(), val2);
                            call.arguments.insert(arg2.to_string(), val1);
                        }
                    }
                }
                QuirkRule::InjectDefault { key, value } => {
                    if !call.arguments.contains_key(key) {
                        call.arguments.insert(key.to_string(), serde_json::json!(value));
                    }
                }
            }
        }
        call
    }
}

/// ExpertQuirkMapper Module
/// Acts as a pre-processor on the P1000 node to intercept and fix incoming tasks.
pub struct ExpertQuirkMapper {
    registry: QuirkRegistry,
}

impl ExpertQuirkMapper {
    pub fn new(registry: QuirkRegistry) -> Self {
        Self { registry }
    }

    /// Process an incoming tool call, applying registered quirks.
    pub fn translate(&self, input: ToolCall) -> ToolCall {
        self.registry.apply_quirks(input)
    }
}

// Example usage / Setup
pub fn setup_default_registry() -> QuirkRegistry {
    let mut reg = QuirkRegistry::new();
    
    // Rule for SAR Agent: Expecting get_coord(lat, lon), but user sends find_at(lon, lat)
    reg.add_rule(QuirkRule::RenameTool { old_name: "find_at", new_name: "get_coord" });
    reg.add_rule(QuirkRule::SwapArgs { arg1: "lon", arg2: "lat" });
    
    // Rule for Depth Agent: Ensure depth is always positive (inject default if missing)
    reg.add_rule(QuirkRule::InjectDefault { key: "min_depth", value: 0.0 });
    
    reg
}