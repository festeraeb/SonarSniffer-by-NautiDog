//! Delegate tool execution to a remote cesarops-mcp-worker (:8090).

use tracing::warn;

const DELEGATABLE: &[&str] = &[
    "think_harder",
    "remember",
    "read_file",
    "write_file",
    "cargo_check",
    "run_command",
];

pub fn mcp_worker_base() -> Option<String> {
    std::env::var("MCP_WORKER_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            if std::env::var("MCP_DELEGATE_TOOLS")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
            {
                Some("http://127.0.0.1:8090".to_string())
            } else {
                None
            }
        })
}

pub fn should_delegate(tool: &str) -> bool {
    mcp_worker_base().is_some() && DELEGATABLE.contains(&tool)
}

/// POST /tool/{name} on the MCP worker.
pub async fn execute_on_mcp_at(
    base: &str,
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<String, String> {
    let url = format!("{}/tool/{}", base.trim_end_matches('/'), tool);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .post(&url)
        .json(arguments)
        .send()
        .await
        .map_err(|e| format!("MCP worker unreachable at {}: {}", base, e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("MCP tool {} HTTP {}: {}", tool, status, body));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("MCP response parse error: {}", e))?;

    if let Some(r) = json.get("result").and_then(|v| v.as_str()) {
        Ok(r.to_string())
    } else {
        Ok(json.to_string())
    }
}

pub async fn execute_on_mcp(tool: &str, arguments: &serde_json::Value) -> Result<String, String> {
    let base = mcp_worker_base().ok_or_else(|| "MCP_WORKER_URL not set".to_string())?;
    execute_on_mcp_at(&base, tool, arguments).await
}

pub fn is_delegatable_tool(tool: &str) -> bool {
    DELEGATABLE.contains(&tool)
}
