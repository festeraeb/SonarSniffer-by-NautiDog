//! Autonomous agent execution loop.
//! Drives inference → tool call parsing → tool execution → context injection.
//! Supports cron-scheduled overnight tasks.

use std::time::{Instant, Duration};
use serde::{Serialize, Deserialize};
use serde_json::Value;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameter_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub task: String,
    pub final_response: String,
    pub steps_executed: u32,
    pub execution_duration_secs: u64,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

/// Schedule for overnight/automated runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Schedule {
    Once,
    Cron(String),
}

/// Configurable agent harness — talks to whichever LLM endpoint is active.
pub struct AgentHarness {
    pub llm_endpoint: String,
    pub tools: Vec<ToolDef>,
    pub max_iterations: u32,
    pub timeout: Duration,
    pub detection_endpoint: Option<String>,
}

impl AgentHarness {
    pub fn new(
        llm_endpoint: String,
        tools: Vec<ToolDef>,
        max_iterations: u32,
        timeout: Duration,
    ) -> Self {
        Self {
            llm_endpoint,
            tools,
            max_iterations,
            timeout,
            detection_endpoint: None,
        }
    }

    pub fn with_detection(mut self, endpoint: String) -> Self {
        self.detection_endpoint = Some(endpoint);
        self
    }

    /// Run the autonomous agent loop. Async — fits into the tokio runtime.
    pub async fn run_task(&self, task_prompt: &str) -> AgentResult {
        let start_time = Instant::now();

        // Build initial context with system prompt + user task
        let mut context = format!(
            "<|im_start|>system\nYou are an autonomous agent in the Cesarops Cluster. \
             You have tools: shell_exec, file_read, file_write, http_request, scan_dispatch. \
             Call them with: <tool_call>{{\"name\": \"tool_name\", \"arguments\": {{...}}}}</tool_call>\n\
             When done, respond without a tool call.<|im_end|>\n\
             <|im_start|>user\n{}<|im_end|>\n\
             <|im_start|>assistant\n",
            task_prompt
        );

        let mut iteration = 0u32;
        let mut final_text = String::new();
        let mut success = false;

        while iteration < self.max_iterations {
            if start_time.elapsed() > self.timeout {
                final_text = format!(
                    "Timeout after {} seconds ({} iterations)",
                    self.timeout.as_secs(), iteration
                );
                warn!("{}", final_text);
                break;
            }

            iteration += 1;
            info!("Agent step {}/{}", iteration, self.max_iterations);

            // Query the active LLM
            match self.query_llm(&context).await {
                Ok(response) => {
                    final_text = response.clone();
                    context.push_str(&response);
                    context.push_str("<|im_end|>\n");

                    // Check for tool calls
                    if let Some(tool_call) = parse_tool_call(&response) {
                        info!("Tool call: {} args={}", tool_call.name, tool_call.arguments);
                        let result = self.execute_tool(&tool_call).await;
                        info!("Tool result: {} bytes", result.len());

                        // Inject tool output back into context
                        context.push_str(&format!(
                            "<|im_start|>tool\n{}<|im_end|>\n<|im_start|>assistant\n",
                            result
                        ));
                    } else {
                        // No tool call — model is done
                        success = true;
                        break;
                    }
                }
                Err(e) => {
                    final_text = format!("LLM query failed: {}", e);
                    warn!("{}", final_text);
                    break;
                }
            }
        }

        AgentResult {
            task: task_prompt.to_string(),
            final_response: final_text,
            steps_executed: iteration,
            execution_duration_secs: start_time.elapsed().as_secs(),
            success,
        }
    }

    async fn query_llm(&self, prompt_context: &str) -> Result<String, String> {
        let client = reqwest::Client::new();

        let body = serde_json::json!({
            "prompt": prompt_context,
            "max_length": 512,
            "temperature": 0.1,
            "top_p": 0.95,
            "use_chat_template": false
        });

        let res = client
            .post(&self.llm_endpoint)
            .json(&body)
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| format!("LLM endpoint unreachable: {}", e))?;

        if !res.status().is_success() {
            return Err(format!("LLM returned HTTP {}", res.status()));
        }

        let parsed: Value = res.json().await
            .map_err(|e| format!("Invalid JSON from LLM: {}", e))?;

        // KoboldCPP format: {"results": [{"text": "..."}]}
        if let Some(text) = parsed.get("results")
            .and_then(|r| r.as_array())
            .and_then(|arr| arr.first())
            .and_then(|r| r.get("text"))
            .and_then(|t| t.as_str())
        {
            return Ok(text.to_string());
        }

        // OpenAI format: {"choices": [{"message": {"content": "..."}}]}
        if let Some(text) = parsed.get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
        {
            return Ok(text.to_string());
        }

        Err(format!("Unrecognized LLM response format: {}", parsed))
    }

    async fn execute_tool(&self, call: &ToolCall) -> String {
        match call.name.as_str() {
            "shell_exec" => {
                let cmd = call.arguments.get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or("echo 'no command'");
                match tokio::process::Command::new("bash")
                    .arg("-c")
                    .arg(cmd)
                    .output()
                    .await
                {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        if stderr.is_empty() {
                            stdout.to_string()
                        } else {
                            format!("{}\nSTDERR: {}", stdout, stderr)
                        }
                    }
                    Err(e) => format!("shell_exec failed: {}", e),
                }
            }

            "file_read" => {
                let path = call.arguments.get("path")
                    .and_then(|p| p.as_str())
                    .unwrap_or("");
                match tokio::fs::read_to_string(path).await {
                    Ok(contents) => {
                        // Cap at 4k to avoid blowing context
                        if contents.len() > 4096 {
                            format!("{}...\n[truncated, {} bytes total]", &contents[..4096], contents.len())
                        } else {
                            contents
                        }
                    }
                    Err(e) => format!("file_read failed: {}", e),
                }
            }

            "file_write" => {
                let path = call.arguments.get("path")
                    .and_then(|p| p.as_str())
                    .unwrap_or("");
                let content = call.arguments.get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                match tokio::fs::write(path, content).await {
                    Ok(_) => format!("Written {} bytes to {}", content.len(), path),
                    Err(e) => format!("file_write failed: {}", e),
                }
            }

            "http_request" => {
                let url = call.arguments.get("url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("");
                let method = call.arguments.get("method")
                    .and_then(|m| m.as_str())
                    .unwrap_or("GET");

                let client = reqwest::Client::new();
                let req = match method.to_uppercase().as_str() {
                    "POST" => {
                        let body = call.arguments.get("body").cloned().unwrap_or(Value::Null);
                        client.post(url).json(&body)
                    }
                    _ => client.get(url),
                };

                match req.timeout(Duration::from_secs(30)).send().await {
                    Ok(res) => {
                        let status = res.status().as_u16();
                        let body = res.text().await.unwrap_or_default();
                        if body.len() > 4096 {
                            format!("HTTP {} — {}...[truncated]", status, &body[..4096])
                        } else {
                            format!("HTTP {} — {}", status, body)
                        }
                    }
                    Err(e) => format!("http_request failed: {}", e),
                }
            }

            "scan_dispatch" => {
                let endpoint = self.detection_endpoint.as_deref()
                    .unwrap_or("http://127.0.0.1:8080/scan");
                let client = reqwest::Client::new();
                match client.post(endpoint)
                    .json(&call.arguments)
                    .timeout(Duration::from_secs(60))
                    .send()
                    .await
                {
                    Ok(res) => res.text().await.unwrap_or_else(|_| "scan dispatched".to_string()),
                    Err(e) => format!("scan_dispatch failed: {}", e),
                }
            }

            other => format!("Unknown tool: {}", other),
        }
    }
}

/// Parse a <tool_call>...</tool_call> block from LLM output.
fn parse_tool_call(text: &str) -> Option<ToolCall> {
    let start = text.find("<tool_call>")?;
    let end = text.find("</tool_call>")?;
    if end <= start {
        return None;
    }
    let json_slice = &text[start + 11..end].trim();
    serde_json::from_str::<ToolCall>(json_slice).ok()
}

/// Default tool definitions for the agent.
pub fn default_tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "shell_exec".to_string(),
            description: "Execute a bash command on the local node".to_string(),
            parameter_schema: serde_json::json!({"command": "string"}),
        },
        ToolDef {
            name: "file_read".to_string(),
            description: "Read a file from disk".to_string(),
            parameter_schema: serde_json::json!({"path": "string"}),
        },
        ToolDef {
            name: "file_write".to_string(),
            description: "Write content to a file".to_string(),
            parameter_schema: serde_json::json!({"path": "string", "content": "string"}),
        },
        ToolDef {
            name: "http_request".to_string(),
            description: "Make an HTTP request".to_string(),
            parameter_schema: serde_json::json!({"url": "string", "method": "string", "body": "object"}),
        },
        ToolDef {
            name: "scan_dispatch".to_string(),
            description: "Dispatch a detection scan to the pipeline".to_string(),
            parameter_schema: serde_json::json!({"region": "string", "tiles": "array"}),
        },
    ]
}
