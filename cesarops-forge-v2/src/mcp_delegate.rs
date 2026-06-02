//! Delegate tool execution to a remote cesarops-mcp-worker (:8090).

const DELEGATABLE: &[&str] = &[
    "think_harder",
    "remember",
    "read_file",
    "write_file",
    "cargo_check",
    "run_command",
    "scan_region",
    "magnetic_dipole_detect",
    "download_satellite_window",
    "weather_window",
    "detection_health",
    "detection_scan",
    "detection_poll",
    "sat_mission",
    "sat_read_mission_report",
    // SymForge-compatible coding tools
    "search_symbols",
    "get_symbol",
    "get_file_context",
    "search_text",
    "replace_symbol_body",
    "edit_within_symbol",
    "insert_symbol",
    "delete_symbol",
    "batch_edit",
    "batch_rename",
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

pub fn delegatable_tools() -> &'static [&'static str] {
    DELEGATABLE
}

pub async fn fetch_mcp_tools_at(base: &str) -> Result<Vec<String>, String> {
    let url = format!("{}/capabilities", base.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("MCP capabilities unreachable at {}: {}", base, e))?;
    if !resp.status().is_success() {
        return Err(format!("MCP capabilities HTTP {}", resp.status()));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("MCP capabilities parse error: {}", e))?;
    let tools = json
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(tools)
}
