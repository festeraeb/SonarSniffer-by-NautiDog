//! HTTP client for local/remote inference endpoints.
//!
//! - **Default:** OpenAI-compatible `/v1/completions` (llama-server).
//! - **Thinking models (Gemma/R1 on :5001/:5002):** `/v1/chat/completions` with
//!   `reasoning_content` merged when `content` is empty (`[inference]` in cluster_config).
//! - **Kobold only** when `engine` is explicitly `"koboldcpp"` (e.g. M2200 ThinkPad).

use std::time::Duration;

use crate::inference_config;

pub const LLAMA_SERVER_BIN: &str = "/home/cesarops/llama.cpp/build/bin/llama-server";

/// True only when the operator pinned Kobold for this worker/endpoint.
pub fn uses_kobold_generate_api(engine: Option<&str>) -> bool {
    matches!(engine.map(str::trim), Some("koboldcpp"))
}

/// Resolve effective engine for worker spawn (`engine` field may be empty).
pub fn effective_worker_engine(engine_pin: &str, use_native: bool) -> String {
    if use_native {
        "cesarops-inference".to_string()
    } else if engine_pin == "koboldcpp" {
        "koboldcpp".to_string()
    } else if engine_pin == "llama-server" || engine_pin == "llama.cpp" || engine_pin.is_empty() {
        "llama-server".to_string()
    } else {
        engine_pin.to_string()
    }
}

pub async fn complete_prompt(
    client: &reqwest::Client,
    base_url: &str,
    prompt: &str,
    max_tokens: u32,
    temperature: f32,
    stop_sequences: Vec<String>,
    engine: Option<&str>,
) -> Result<String, String> {
    if uses_kobold_generate_api(engine) {
        kobold_generate(client, base_url, prompt, max_tokens, temperature, stop_sequences).await
    } else {
        let cfg = inference_config::load();
        if cfg.endpoint_uses_chat(base_url) {
            openai_chat_completions(
                client,
                base_url,
                prompt,
                max_tokens,
                temperature,
                stop_sequences,
                &cfg,
            )
            .await
        } else {
            openai_completions(client, base_url, prompt, max_tokens, temperature, stop_sequences).await
        }
    }
}

/// Merge llama-server `content` + `reasoning_content` into one string for Forge.
pub fn merge_assistant_fields(content: &str, reasoning: Option<&str>, merge_reasoning: bool) -> String {
    let c = content.trim();
    let r = reasoning.map(str::trim).unwrap_or("");
    if !c.is_empty() {
        if merge_reasoning && !r.is_empty() && !c.contains(r) {
            // Tool calls often live in reasoning while content holds the short answer.
            if c.len() < 120 && (r.contains("<tool_call>") || r.contains("```")) {
                return format!("{c}\n{r}");
            }
        }
        return content.to_string();
    }
    if merge_reasoning && !r.is_empty() {
        return r.to_string();
    }
    content.to_string()
}

/// Extract assistant text from an OpenAI-style choice object.
pub fn extract_choice_text(choice: &serde_json::Value, merge_reasoning: bool) -> String {
    if let Some(text) = choice.get("text").and_then(|t| t.as_str()) {
        return text.to_string();
    }
    if let Some(msg) = choice.get("message") {
        let content = msg.get("content").and_then(|t| t.as_str()).unwrap_or("");
        let reasoning = msg.get("reasoning_content").and_then(|t| t.as_str());
        return merge_assistant_fields(content, reasoning, merge_reasoning);
    }
    String::new()
}

/// Parse ChatML / Gemma turn prompts into chat `messages` (drops trailing assistant prefill).
pub fn parse_prompt_to_messages(prompt: &str) -> Vec<serde_json::Value> {
    if prompt.contains("<start_of_turn>") {
        return parse_gemma_turns(prompt);
    }
    if prompt.contains("<|im_start|>") {
        return parse_chatml_turns(prompt);
    }
    vec![serde_json::json!({"role": "user", "content": prompt})]
}

fn parse_chatml_turns(prompt: &str) -> Vec<serde_json::Value> {
    let re = regex::Regex::new(
        r"(?s)<\|im_start\|>(system|user|assistant)\n(.*?)(?:<\|im_end\|>|<\|redacted_im_end\|>)",
    )
    .expect("chatml regex");
    let mut out = Vec::new();
    for cap in re.captures_iter(prompt) {
        let role = cap.get(1).map(|m| m.as_str()).unwrap_or("user");
        let content = cap.get(2).map(|m| m.as_str()).unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        out.push(serde_json::json!({"role": role, "content": content}));
    }
    out
}

fn parse_gemma_turns(prompt: &str) -> Vec<serde_json::Value> {
    let re = regex::Regex::new(r"(?s)<start_of_turn>(user|model)\n(.*?)\n<end_of_turn>")
        .expect("gemma regex");
    let mut out = Vec::new();
    for cap in re.captures_iter(prompt) {
        let turn = cap.get(1).map(|m| m.as_str()).unwrap_or("user");
        let content = cap.get(2).map(|m| m.as_str()).unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let role = if turn == "model" { "assistant" } else { "user" };
        out.push(serde_json::json!({"role": role, "content": content}));
    }
    // Trailing `<start_of_turn>model` without body is generation prefill — omit.
    out
}

async fn openai_chat_completions(
    client: &reqwest::Client,
    base_url: &str,
    prompt: &str,
    max_tokens: u32,
    temperature: f32,
    stop_sequences: Vec<String>,
    cfg: &inference_config::InferenceConfig,
) -> Result<String, String> {
    let url = format!(
        "{}/v1/chat/completions",
        inference_config::normalize_endpoint(base_url)
    );
    let messages = parse_prompt_to_messages(prompt);
    let payload = serde_json::json!({
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": temperature,
        "stop": stop_sequences,
        "stream": false,
    });

    let resp = client
        .post(&url)
        .json(&payload)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("{}: {}", url, e))?;

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    if let Some(err) = body.get("error") {
        return Err(format!("{}: {}", url, err));
    }

    let text = body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .map(|c| extract_choice_text(c, cfg.merge_reasoning_content))
        .unwrap_or_default();

    Ok(text)
}

async fn openai_completions(
    client: &reqwest::Client,
    base_url: &str,
    prompt: &str,
    max_tokens: u32,
    temperature: f32,
    stop_sequences: Vec<String>,
) -> Result<String, String> {
    let url = format!("{}/v1/completions", base_url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "prompt": prompt,
        "max_tokens": max_tokens,
        "temperature": temperature,
        "stop": stop_sequences,
        "stream": false,
    });

    let resp = client
        .post(&url)
        .json(&payload)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("{}: {}", url, e))?;

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    if let Some(err) = body.get("error") {
        return Err(format!("{}: {}", url, err));
    }

    let cfg = inference_config::load();
    let text = body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .map(|c| extract_choice_text(c, cfg.merge_reasoning_content))
        .unwrap_or_default();

    Ok(text)
}

async fn kobold_generate(
    client: &reqwest::Client,
    base_url: &str,
    prompt: &str,
    max_tokens: u32,
    temperature: f32,
    stop_sequences: Vec<String>,
) -> Result<String, String> {
    let url = format!("{}/api/v1/generate", base_url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "prompt": prompt,
        "max_length": max_tokens,
        "temperature": temperature,
        "top_p": 0.95,
        "rep_pen": 1.1,
        "stop_sequence": stop_sequences,
    });

    let resp = client
        .post(&url)
        .json(&payload)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("{}: {}", url, e))?;

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    let text = body
        .get("results")
        .and_then(|r| r.as_array())
        .and_then(|a| a.first())
        .and_then(|r| r.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    Ok(text)
}

/// Build a detached llama-server launch command for a local [[worker]] entry.
pub fn llama_server_spawn_cmd(
    model: &str,
    port: i64,
    backend: &str,
    gpulayers: i64,
    contextsize: i64,
    threads: i64,
    tensor_split: &[String],
    worker_idx: usize,
) -> String {
    let dev = if backend == "vulkan" {
        "Vulkan0,Vulkan1".to_string()
    } else {
        "CUDA0,CUDA1".to_string()
    };

    let ts = if tensor_split.is_empty() {
        "50,50".to_string()
    } else {
        tensor_split.join(",")
    };

    let ngl = if gpulayers <= 0 || gpulayers >= 99 {
        "99".to_string()
    } else {
        gpulayers.to_string()
    };

    let mtp_suffix = if model.contains("MTP") || model.contains("mtp") {
        " --spec-type draft-mtp --spec-draft-n-max 2"
    } else {
        ""
    };
    format!(
        "setsid {bin} -m {model} --host 0.0.0.0 --port {port} \
         -dev {dev} -sm layer -ts {ts} -ngl {ngl} -c {ctx} -t {threads}{mtp} \
         > /tmp/worker_{idx}.log 2>&1 < /dev/null & disown",
        bin = LLAMA_SERVER_BIN,
        model = model,
        port = port,
        dev = dev,
        ts = ts,
        ngl = ngl,
        ctx = contextsize,
        threads = threads,
        mtp = mtp_suffix,
        idx = worker_idx,
    )
}

/// Shell snippet to stop inference on P100 ports (llama + kobold).
pub const STOP_P100_INFERENCE: &str =
    "fuser -k 5001/tcp 2>/dev/null ; fuser -k 5002/tcp 2>/dev/null ; \
     pkill -f 'llama-server.*--port 500[12]' 2>/dev/null ; \
     pkill -f 'koboldcpp.*--port 500[12]' 2>/dev/null ; true";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_reasoning_when_content_empty() {
        let out = merge_assistant_fields("", Some("plan then OK"), true);
        assert_eq!(out, "plan then OK");
    }

    #[test]
    fn merge_prefers_content() {
        let out = merge_assistant_fields("OK", Some("thinking"), true);
        assert_eq!(out, "OK");
    }

    #[test]
    fn parse_chatml_drops_empty_assistant() {
        let p = "<|im_start|>system\nsys<|im_end|>\n<|im_start|>user\nhi<|im_end|>\n<|im_start|>assistant\n";
        let msgs = parse_prompt_to_messages(p);
        assert_eq!(msgs.len(), 2);
    }
}
