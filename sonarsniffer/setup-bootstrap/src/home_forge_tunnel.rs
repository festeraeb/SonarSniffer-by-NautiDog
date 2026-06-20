use std::process::Command;
use tungstenite::{connect, Message};
use url::Url;
use serde_json::Value;

#[cfg(windows)]
pub fn run_diagnostic_tunnel(telemetry_payload: Value) -> Result<(), Box<dyn std::error::Error>> {
    let url_str = std::env::var("N8N_TUNNEL_URL")
        .unwrap_or_else(|_| "wss://api.cesarops.com/diagnostics/tunnel".to_string());
    let url = Url::parse(&url_str)?;
    
    // Connect to the n8n websocket webhook
    println!("Opening diagnostic tunnel to CesarOps Cloud Agent...");
    let (mut socket, response) = match connect(url) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Failed to connect to diagnostic tunnel: {}", e);
            return Err(e.into());
        }
    };
    
    println!("Connected to cloud agent (HTTP {}).", response.status());

    // Send the telemetry payload
    socket.write_message(Message::Text(telemetry_payload.to_string()))?;

    // Wait for the cloud agent to stream back a Fix Payload
    loop {
        let msg = socket.read_message()?;
        match msg {
            Message::Text(text) => {
                if let Ok(fix_payload) = serde_json::from_str::<Value>(&text) {
                    if let Some(action) = fix_payload.get("action").and_then(|a| a.as_str()) {
                        if action == "run_powershell" {
                            if let Some(script) = fix_payload.get("script").and_then(|s| s.as_str()) {
                                println!("Executing automated cloud fix payload...");
                                let _ = execute_powershell_fix(script);
                                // Acknowledge completion and break out of loop
                                let _ = socket.write_message(Message::Text(r#"{"status": "fix_applied"}"#.to_string()));
                                break;
                            }
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    
    Ok(())
}

#[cfg(windows)]
fn execute_powershell_fix(script: &str) -> Result<(), std::io::Error> {
    let ps = std::env::var("SystemRoot")
        .map(|w| format!("{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe", w))
        .unwrap_or_else(|_| "powershell.exe".to_string());
        
    let status = Command::new(&ps)
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
        .status()?;
        
    if !status.success() {
        eprintln!("Fix payload execution failed with status: {}", status);
    }
    
    Ok(())
}

#[cfg(not(windows))]
pub fn run_diagnostic_tunnel(_telemetry_payload: Value) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
