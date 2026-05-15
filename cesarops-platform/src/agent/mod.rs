pub fn init() { tracing::info!("agent module loaded"); }
pub async fn dispatch_task(endpoint: &str, prompt: &str) -> String {
    let client = reqwest::Client::new();
    match client.post(format!("{}/api/v1/generate", endpoint))
        .json(&serde_json::json!({"prompt": prompt, "max_length": 2048, "temperature": 0.3}))
        .send().await {
        Ok(resp) => resp.text().await.unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}
